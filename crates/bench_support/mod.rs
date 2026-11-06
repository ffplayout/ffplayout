//! Shared measurement protocol for native libc comparisons. One case per process.
use std::{hint::black_box, mem::MaybeUninit, time::Instant};

use anyhow::{Result, ensure};
use clap::Parser;

#[derive(Parser)]
pub struct Args {
    #[arg(long, hide = true)]
    pub bench: bool,
    #[arg(long)]
    pub list: bool,
    #[arg(long)]
    pub case: Option<String>,
    #[arg(long, default_value_t = 15)]
    pub samples: usize,
    #[arg(long, default_value_t = 10)]
    pub iterations: usize,
    #[arg(long, default_value_t = 3)]
    pub warmup: usize,
    #[arg(long, default_value_t = 4)]
    pub threads: usize,
}

impl Args {
    pub fn selected(&self, cases: &[&str]) -> Result<Option<String>> {
        ensure!(
            self.samples >= 3 && self.iterations > 0 && self.threads > 0,
            "invalid measurement parameters"
        );

        if self.list {
            for case in cases {
                println!("{case}");
            }

            return Ok(None);
        }

        let case = self.case.as_deref().unwrap_or(cases[0]);

        ensure!(cases.contains(&case), "unknown benchmark case: {case}");

        Ok(Some(case.to_owned()))
    }
}

#[cfg(unix)]
fn usage() -> Result<(f64, f64)> {
    let mut usage = MaybeUninit::<libc::rusage>::uninit();

    // SAFETY: getrusage writes a correctly sized, writable rusage on success.
    ensure!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0,
        "getrusage failed"
    );

    // SAFETY: the successful call above initialized the structure.
    let usage = unsafe { usage.assume_init() };
    let cpu = usage.ru_utime.tv_sec as f64
        + usage.ru_utime.tv_usec as f64 / 1e6
        + usage.ru_stime.tv_sec as f64
        + usage.ru_stime.tv_usec as f64 / 1e6;
    let rss = usage.ru_maxrss as f64
        / if cfg!(target_os = "macos") {
            1_048_576.0
        } else {
            1024.0
        };

    Ok((cpu, rss))
}

#[cfg(not(unix))]
fn usage() -> Result<(f64, f64)> {
    anyhow::bail!("libc comparisons require Unix")
}

#[cfg(target_os = "linux")]
fn peak_rss_mib() -> Result<f64> {
    // ru_maxrss can retain the launching Python process's pre-exec high-water
    // mark. VmHWM belongs to this executable's new address space instead.
    let status = std::fs::read_to_string("/proc/self/status")?;
    let kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.split_whitespace().next())
        .ok_or_else(|| anyhow::anyhow!("VmHWM is unavailable in /proc/self/status"))?
        .parse::<f64>()?;

    Ok(kib / 1024.0)
}

#[cfg(not(target_os = "linux"))]
fn peak_rss_mib() -> Result<f64> {
    Ok(usage()?.1)
}

pub fn measure(
    args: &Args,
    case: &str,
    work_units: usize,
    mut operation: impl FnMut() -> Result<u64>,
) -> Result<()> {
    for _ in 0..args.warmup {
        black_box(operation()?);
    }

    let mut wall = Vec::with_capacity(args.samples);
    let mut cpu = Vec::with_capacity(args.samples);
    let mut checksum = 0_u64;

    for _ in 0..args.samples {
        let before = usage()?.0;
        let start = Instant::now();

        for _ in 0..args.iterations {
            checksum = checksum.wrapping_add(black_box(operation()?));
        }

        let elapsed = start.elapsed().as_secs_f64();
        let consumed = usage()?.0 - before;

        wall.push(elapsed * 1000.0 / args.iterations as f64);
        cpu.push(consumed * 1000.0 / args.iterations as f64);
    }

    wall.sort_by(f64::total_cmp);
    cpu.sort_by(f64::total_cmp);

    let median = args.samples / 2;
    let p95 = (args.samples * 95).div_ceil(100).saturating_sub(1);
    let ffmpeg = ffmpeg_next::util::version();
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "glibc"
    };

    println!(
        "{{\"case\":\"{case}\",\"libc\":\"{libc}\",\"samples\":{},\"iterations\":{},\"threads\":{},\"work_units\":{work_units},\"wall_median_ms\":{:.9},\"wall_p95_ms\":{:.9},\"cpu_median_ms\":{:.9},\"peak_rss_mib\":{:.3},\"checksum\":{checksum},\"avutil_version\":{ffmpeg}}}",
        args.samples,
        args.iterations,
        args.threads,
        wall[median],
        wall[p95],
        cpu[median],
        peak_rss_mib()?
    );

    Ok(())
}
