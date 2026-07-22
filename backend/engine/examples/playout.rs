use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use env_logger::{Builder, Env};
use glob::glob;

use ff_engine::{
    ClipResult, HlsSubtitle, HlsVariant, LogoConfig, OutputConfig, OutputSize, Playout,
    print_media_info, spawn_rtmp_listener,
};

#[derive(Parser, Debug)]
struct Args {
    /// Input files, directories, or glob patterns
    inputs: Vec<String>,

    /// Output file or URL, e.g. out.mp4 or rtmp://host/live/stream
    #[cfg_attr(
        feature = "desktop-base",
        arg(
            short,
            long,
            required_unless_present_any = ["desktop", "hls"],
            conflicts_with_all = ["desktop", "hls"]
        )
    )]
    #[cfg_attr(
        not(feature = "desktop-base"),
        arg(short, long, required_unless_present = "hls", conflicts_with = "hls")
    )]
    output: Option<String>,

    /// Play video and audio in a native desktop window
    #[cfg(feature = "desktop-base")]
    #[arg(long, conflicts_with_all = ["output", "hls"])]
    desktop: bool,

    /// Publish a live HLS playlist, e.g. /var/www/live/index.m3u8
    #[cfg_attr(
        feature = "desktop-base",
        arg(long, value_name = "PLAYLIST", conflicts_with_all = ["output", "desktop"])
    )]
    #[cfg_attr(
        not(feature = "desktop-base"),
        arg(long, value_name = "PLAYLIST", conflicts_with = "output")
    )]
    hls: Option<String>,

    /// Add an adaptive HLS rendition: NAME:WIDTHxHEIGHT:VIDEO_BITRATE[:AUDIO_BITRATE]
    #[arg(
        long = "hls-variant",
        value_name = "NAME:WIDTHxHEIGHT:VIDEO_BITRATE[:AUDIO_BITRATE]",
        requires = "hls"
    )]
    hls_variants: Vec<HlsVariant>,

    /// Include sidecar WebVTT subtitles for HLS. For input video.mp4, video.vtt is used.
    #[arg(long, requires = "hls")]
    hls_vtt_subtitles: bool,

    /// HLS subtitle display name
    #[arg(long, default_value = "Subtitles", requires = "hls")]
    hls_subtitle_name: String,

    /// HLS subtitle language tag
    #[arg(long, default_value = "und", requires = "hls")]
    hls_subtitle_language: String,

    /// Mark the HLS subtitle rendition as default
    #[arg(long, requires = "hls")]
    hls_subtitle_default: bool,

    /// HLS segment duration in seconds
    #[arg(long, value_name = "SECONDS", default_value_t = 6)]
    hls_segment_seconds: u32,

    /// Number of segments kept in the HLS playlist
    #[arg(long, value_name = "COUNT", default_value_t = 60)]
    hls_list_size: u32,

    /// Seek position in seconds for the first input file only
    #[arg(long, value_name = "SECONDS", default_value_t = 0.0)]
    seek: f64,

    /// Output size as WIDTH:HEIGHT. Defaults to 1024:576.
    #[arg(long, value_name = "WIDTH:HEIGHT")]
    size: Option<OutputSize>,

    /// Audio volume multiplier.
    #[arg(long, default_value_t = 1.0)]
    volume: f64,

    /// Logo image path to overlay on video.
    #[arg(long, value_name = "PATH")]
    logo: Option<String>,

    /// Logo scale as WIDTH:HEIGHT. Use -1 to preserve aspect ratio.
    #[arg(long, value_name = "WIDTH:HEIGHT")]
    logo_scale: Option<String>,

    /// Logo opacity from 0.0 to 1.0.
    #[arg(long, default_value_t = 1.0)]
    logo_opacity: f64,

    /// Logo position expression, e.g. W-w-20:H-h-20.
    #[arg(long, default_value = "W-w-20:H-h-20")]
    logo_position: String,

    /// RTMP listen URL for live override, e.g. rtmp://0.0.0.0:1935/live/input
    #[arg(long, value_name = "URL")]
    rtmp_live: Option<String>,

    /// Duration in seconds used when an input is missing or cannot be decoded
    #[arg(long, default_value_t = 10.0)]
    fallback_duration: f64,

    /// Randomize input files.
    #[arg(long)]
    random: bool,
}

impl Args {
    fn desktop(&self) -> bool {
        #[cfg(feature = "desktop-base")]
        {
            self.desktop
        }
        #[cfg(not(feature = "desktop-base"))]
        {
            false
        }
    }
}

fn init_logger() {
    let env = Env::default()
        .filter_or("MY_LOG_LEVEL", "trace")
        .write_style_or("MY_LOG_STYLE", "always");

    Builder::from_env(env)
        .format_timestamp(None)
        .format_level(true)
        .format_target(false)
        .init();
}

fn main() -> Result<()> {
    init_logger();

    let args = Args::parse();
    if args.inputs.is_empty() {
        return Err(anyhow!(
            "please provide at least one input file, directory, or glob pattern"
        ));
    }
    let inputs = resolve_inputs(&args.inputs, args.random)?;
    if !args.seek.is_finite() || args.seek < 0.0 {
        return Err(anyhow!("--seek must be a non-negative number"));
    }
    if !args.volume.is_finite() || !(0.0..=1.0).contains(&args.volume) {
        return Err(anyhow!("--volume must be between 0.0 and 1.0"));
    }
    if !args.logo_opacity.is_finite() || !(0.0..=1.0).contains(&args.logo_opacity) {
        return Err(anyhow!("--logo-opacity must be between 0.0 and 1.0"));
    }
    if args.hls_vtt_subtitles && args.hls_variants.is_empty() {
        return Err(anyhow!(
            "--hls-vtt-subtitles requires at least one --hls-variant so subtitles can be linked from master.m3u8"
        ));
    }

    let mut config = OutputConfig::default();
    if let Some(size) = args.size {
        config.width = size.width;
        config.height = size.height;
    }
    config = config.with_volume(args.volume)?;
    config.logo = args.logo.clone().map(|path| LogoConfig {
        path,
        scale: args.logo_scale.clone(),
        opacity: args.logo_opacity,
        position: args.logo_position.clone(),
    });
    let live_config = config.clone();

    let mut playout = if args.desktop() {
        #[cfg(feature = "desktop-base")]
        {
            Playout::open_desktop(config, args.fallback_duration)?
        }
        #[cfg(not(feature = "desktop-base"))]
        {
            return Err(anyhow!(
                "--desktop is not available because this binary was built without the desktop feature"
            ));
        }
    } else if let Some(playlist) = args.hls.as_deref() {
        Playout::open_hls(
            playlist,
            config,
            args.fallback_duration,
            &args.hls_variants,
            args.hls_vtt_subtitles.then(|| HlsSubtitle {
                name: args.hls_subtitle_name.clone(),
                language: args.hls_subtitle_language.clone(),
                default: args.hls_subtitle_default,
            }),
            args.hls_segment_seconds,
            args.hls_list_size,
        )?
    } else {
        Playout::open(
            args.output
                .as_deref()
                .ok_or_else(|| anyhow!("missing output"))?,
            config,
            args.fallback_duration,
        )?
    };

    let mut live = args
        .rtmp_live
        .clone()
        .map(|url| spawn_rtmp_listener(url, live_config));

    for (index, path) in inputs.iter().enumerate() {
        print_media_info(path);
        let seek_seconds = (index == 0 && args.seek > 0.0).then_some(args.seek);
        match playout.play_with_live(path, seek_seconds, &mut live)? {
            ClipResult::Played => {}
            ClipResult::Skipped => {}
            ClipResult::LiveEnded => {}
            ClipResult::Fallback { reason } => {
                log::error!("failed while playing {path}: {reason}; fallback generated");
            }
            ClipResult::Stopped => return Ok(()),
        }
    }

    playout.finish()
}

fn resolve_inputs(inputs: &[String], random: bool) -> Result<Vec<String>> {
    let mut resolved = Vec::new();
    let mut seen = HashSet::new();

    for input in inputs {
        let paths = resolve_input(input)?;
        for path in paths {
            if seen.insert(path.clone()) {
                resolved.push(path_to_string(path)?);
            }
        }
    }

    if random {
        shuffle_inputs(&mut resolved);
    }

    if resolved.is_empty() {
        return Err(anyhow!("input expansion produced no playable files"));
    }

    Ok(resolved)
}

fn shuffle_inputs(inputs: &mut [String]) {
    if inputs.len() < 2 {
        return;
    }

    let mut seed = random_seed();
    for index in (1..inputs.len()).rev() {
        seed = next_random(seed);
        let swap_index = (seed as usize) % (index + 1);
        inputs.swap(index, swap_index);
    }
}

fn random_seed() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = now.as_nanos();
    (nanos as u64) ^ ((nanos >> 64) as u64)
}

fn next_random(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005).wrapping_add(1)
}

fn resolve_input(input: &str) -> Result<Vec<PathBuf>> {
    if contains_glob_pattern(input) {
        let mut matches = Vec::new();
        for entry in glob(input).with_context(|| format!("invalid input glob pattern: {input}"))? {
            let path = entry.with_context(|| format!("failed to read glob match for {input}"))?;
            if path.is_dir() {
                matches.extend(resolve_directory(&path)?);
            } else if is_supported_media_file(&path) {
                matches.push(path);
            }
        }
        matches.sort();
        return Ok(matches);
    }

    let path = PathBuf::from(input);
    if path.is_dir() {
        return resolve_directory(&path);
    }

    Ok(vec![path])
}

fn resolve_directory(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(path)
        .with_context(|| format!("failed to read input directory {}", path.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", path.display()))?;
        let entry_path = entry.path();
        if entry_path.is_file() && is_supported_media_file(&entry_path) {
            files.push(entry_path);
        }
    }
    files.sort();
    Ok(files)
}

fn contains_glob_pattern(input: &str) -> bool {
    input.contains('*') || input.contains('?') || input.contains('[')
}

fn is_supported_media_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "aac"
                    | "avi"
                    | "flac"
                    | "m4a"
                    | "m4v"
                    | "mkv"
                    | "mov"
                    | "mp3"
                    | "mp4"
                    | "mpeg"
                    | "mpg"
                    | "ogg"
                    | "opus"
                    | "ts"
                    | "wav"
                    | "webm"
            )
        })
        .unwrap_or(false)
}

fn path_to_string(path: PathBuf) -> Result<String> {
    path.into_os_string().into_string().map_err(|path| {
        anyhow!(
            "input path is not valid UTF-8: {}",
            PathBuf::from(path).display()
        )
    })
}
