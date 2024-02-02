use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde_json::json;
use simplelog::*;

use ffplayout_lib::utils::{
    controller::PlayerControl, gen_dummy, get_delta, is_close, is_remote,
    json_serializer::read_json, loop_filler, loop_image, modified_time, seek_and_length,
    time_in_seconds, Media, MediaProbe, PlayoutConfig, PlayoutStatus, IMAGE_FORMAT,
};

/// Struct for current playlist.
///
/// Here we prepare the init clip and build a iterator where we pull our clips.
#[derive(Debug)]
pub struct CurrentProgram {
    config: PlayoutConfig,
    start_sec: f64,
    end_sec: f64,
    json_mod: Option<String>,
    json_path: Option<String>,
    json_date: String,
    player_control: PlayerControl,
    current_node: Media,
    is_terminated: Arc<AtomicBool>,
    playout_stat: PlayoutStatus,
    last_json_path: Option<String>,
    last_node_ad: bool,
}

/// Prepare a playlist iterator.
impl CurrentProgram {
    pub fn new(
        config: &PlayoutConfig,
        playout_stat: PlayoutStatus,
        is_terminated: Arc<AtomicBool>,
        player_control: &PlayerControl,
    ) -> Self {
        Self {
            config: config.clone(),
            start_sec: config.playlist.start_sec.unwrap(),
            end_sec: config.playlist.length_sec.unwrap(),
            json_mod: None,
            json_path: None,
            json_date: String::new(),
            player_control: player_control.clone(),
            current_node: Media::new(0, "", false),
            is_terminated,
            playout_stat,
            last_json_path: None,
            last_node_ad: false,
        }
    }

    // Check if playlist file got updated, and when yes we reload it and setup everything in place.
    fn load_or_update_playlist(&mut self, seek: bool) {
        let mut get_current = false;
        let mut reload = false;

        if let Some(path) = self.json_path.clone() {
            if (Path::new(&path).is_file() || is_remote(&path))
                && self.json_mod != modified_time(&path)
            {
                info!("Reload playlist <b><magenta>{path}</></b>");
                self.playout_stat.list_init.store(true, Ordering::SeqCst);
                get_current = true;
                reload = true;
            }
        } else {
            get_current = true;
        }

        if get_current {
            let json = read_json(
                &self.config,
                &self.player_control,
                self.json_path.clone(),
                self.is_terminated.clone(),
                seek,
                false,
            );

            if !reload {
                if let Some(file) = &json.current_file {
                    info!("Read playlist: <b><magenta>{file}</></b>");
                }
            }

            self.json_path = json.current_file;
            self.json_mod = json.modified;
            *self.player_control.current_list.lock().unwrap() = json.program;

            if self.json_path.is_none() {
                trace!("missing playlist");

                self.current_node = Media::new(0, "", false);
                self.playout_stat.list_init.store(true, Ordering::SeqCst);
                self.player_control.current_index.store(0, Ordering::SeqCst);
            }
        }
    }

    // Check if day is past and it is time for a new playlist.
    fn check_for_playlist(&mut self, seek: bool) -> bool {
        let (delta, total_delta) = get_delta(&self.config, &time_in_seconds());
        let mut next = false;

        let duration = if self.current_node.duration >= self.current_node.out {
            self.current_node.duration
        } else {
            // maybe out is longer to be able to loop
            self.current_node.out
        };

        trace!(
            "delta: {delta}, total_delta: {total_delta}, current index: {}",
            self.current_node.index.unwrap_or_default()
        );

        let mut next_start =
            self.current_node.begin.unwrap_or_default() - self.start_sec + duration + delta;

        if self.player_control.current_index.load(Ordering::SeqCst)
            == self.player_control.current_list.lock().unwrap().len() - 1
        {
            next_start += self.config.general.stop_threshold;
        }

        trace!("next_start: {next_start}, end_sec: {}", self.end_sec);

        // Check if we over the target length or we are close to it, if so we load the next playlist.
        if next_start >= self.end_sec
            || is_close(total_delta, 0.0, 2.0)
            || is_close(total_delta, self.end_sec, 2.0)
        {
            trace!("get next day");
            next = true;

            let json = read_json(
                &self.config,
                &self.player_control,
                None,
                self.is_terminated.clone(),
                false,
                true,
            );

            if let Some(file) = &json.current_file {
                info!("Read next playlist: <b><magenta>{file}</></b>");
            }

            self.playout_stat.list_init.store(false, Ordering::SeqCst);
            self.set_status(json.date.clone());

            self.json_path = json.current_file.clone();
            self.json_mod = json.modified;
            self.json_date = json.date;

            *self.player_control.current_list.lock().unwrap() = json.program;
            self.player_control.current_index.store(0, Ordering::SeqCst);
        } else {
            self.load_or_update_playlist(seek)
        }

        next
    }

    fn set_status(&mut self, date: String) {
        *self.playout_stat.current_date.lock().unwrap() = date.clone();
        *self.playout_stat.time_shift.lock().unwrap() = 0.0;

        if let Err(e) = fs::write(
            &self.config.general.stat_file,
            serde_json::to_string(&json!({
                "time_shift": 0.0,
                "date": date,
            }))
            .unwrap(),
        ) {
            error!("Unable to write status file: {e}");
        };
    }

    // Check if last and/or next clip is a advertisement.
    fn last_next_ad(&mut self) {
        let index = self.player_control.current_index.load(Ordering::SeqCst);
        let current_list = self.player_control.current_list.lock().unwrap();

        if index + 1 < current_list.len() && &current_list[index + 1].category == "advertisement" {
            self.current_node.next_ad = true;
        }

        if index > 0
            && index < current_list.len()
            && &current_list[index - 1].category == "advertisement"
        {
            self.current_node.last_ad = true;
        }
    }

    // Get current time and when we are before start time,
    // we add full seconds of a day to it.
    fn get_current_time(&mut self) -> f64 {
        let mut time_sec = time_in_seconds();

        if time_sec < self.start_sec {
            time_sec += self.config.playlist.length_sec.unwrap()
        }

        time_sec
    }

    // On init or reload we need to seek for the current clip.
    fn get_current_clip(&mut self) {
        let mut time_sec = self.get_current_time();
        let shift = self.playout_stat.time_shift.lock().unwrap();

        if *self.playout_stat.current_date.lock().unwrap()
            == *self.playout_stat.date.lock().unwrap()
            && *shift != 0.0
        {
            info!("Shift playlist start for <yellow>{}</> seconds", *shift);
            time_sec += *shift;
        }

        for (i, item) in self
            .player_control
            .current_list
            .lock()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            if item.begin.unwrap() + item.out - item.seek > time_sec {
                self.playout_stat.list_init.store(false, Ordering::SeqCst);
                self.player_control.current_index.store(i, Ordering::SeqCst);

                break;
            }
        }
    }

    // Prepare init clip.
    fn init_clip(&mut self) -> bool {
        trace!("init_clip");
        self.get_current_clip();
        let mut is_filler = false;

        if !self.playout_stat.list_init.load(Ordering::SeqCst) {
            let time_sec = self.get_current_time();
            let index = self
                .player_control
                .current_index
                .fetch_add(1, Ordering::SeqCst);
            let nodes = self.player_control.current_list.lock().unwrap();
            let last_index = nodes.len() - 1;

            // de-instance node to preserve original values in list
            let mut node_clone = nodes[index].clone();

            trace!("Clip from init: {}", node_clone.source);

            // Important! When no manual drop is happen here, lock is still active in handle_list_init
            drop(nodes);

            node_clone.seek += time_sec
                - (node_clone.begin.unwrap() - *self.playout_stat.time_shift.lock().unwrap());

            self.current_node = handle_list_init(
                &self.config,
                node_clone,
                &self.playout_stat,
                &self.player_control,
                last_index,
            );

            if self
                .current_node
                .source
                .contains(&self.config.storage.path.to_string_lossy().to_string())
            {
                is_filler = true;
            }
        }

        is_filler
    }

    fn fill_end(&mut self, total_delta: f64) {
        // Fill end from playlist

        let index = self.player_control.current_index.load(Ordering::SeqCst);
        let mut media = Media::new(index, "", false);
        media.begin = Some(time_in_seconds());
        media.duration = total_delta;
        media.out = total_delta;

        self.current_node = gen_source(
            &self.config,
            media,
            &self.playout_stat,
            &self.player_control,
            0,
        );

        self.player_control
            .current_list
            .lock()
            .unwrap()
            .push(self.current_node.clone());
        self.last_next_ad();

        self.current_node.last_ad = self.last_node_ad;
        self.current_node
            .add_filter(&self.config, &self.playout_stat.chain);

        self.player_control
            .current_index
            .fetch_add(1, Ordering::SeqCst);
    }
}

/// Build the playlist iterator
impl Iterator for CurrentProgram {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        self.last_json_path = self.json_path.clone();
        self.last_node_ad = self.current_node.last_ad;
        self.check_for_playlist(self.playout_stat.list_init.load(Ordering::SeqCst));

        if self.playout_stat.list_init.load(Ordering::SeqCst) {
            trace!("Init playlist, from next iterator");
            let mut init_clip_is_filler = false;

            if self.json_path.is_some() {
                init_clip_is_filler = self.init_clip();
            }

            if self.playout_stat.list_init.load(Ordering::SeqCst) && !init_clip_is_filler {
                // On init load, playlist could be not long enough, or clips are not found
                // so we fill the gap with a dummy.
                trace!("Init clip is no filler");

                let mut current_time = time_in_seconds();
                let (_, total_delta) = get_delta(&self.config, &current_time);

                if self.start_sec > current_time {
                    current_time += self.end_sec + 1.0;
                }

                let mut last_index = 0;
                let length = self.player_control.current_list.lock().unwrap().len();

                if length > 0 {
                    last_index = length - 1;
                }

                let mut media = Media::new(length, "", false);
                media.begin = Some(current_time);
                media.duration = total_delta;
                media.out = total_delta;

                self.current_node = gen_source(
                    &self.config,
                    media,
                    &self.playout_stat,
                    &self.player_control,
                    last_index,
                );
            }

            self.last_next_ad();

            return Some(self.current_node.clone());
        }

        if self.player_control.current_index.load(Ordering::SeqCst)
            < self.player_control.current_list.lock().unwrap().len()
        {
            // get next clip from current playlist

            let mut is_last = false;
            let index = self.player_control.current_index.load(Ordering::SeqCst);
            let node_list = self.player_control.current_list.lock().unwrap();
            let node = node_list[index].clone();
            let last_index = node_list.len() - 1;
            drop(node_list);

            if index == last_index {
                is_last = true
            }

            self.current_node = timed_source(
                node,
                &self.config,
                is_last,
                &self.playout_stat,
                &self.player_control,
                last_index,
            );

            self.last_next_ad();
            self.player_control
                .current_index
                .fetch_add(1, Ordering::SeqCst);

            Some(self.current_node.clone())
        } else {
            let (_, total_delta) = get_delta(&self.config, &self.start_sec);

            if !self.config.playlist.infinit
                && self.last_json_path == self.json_path
                && total_delta.abs() > 1.0
            {
                // Playlist is to early finish,
                // and if we have to fill it with a placeholder.
                trace!("Total delta on list end: {total_delta}");

                self.fill_end(total_delta);

                return Some(self.current_node.clone());
            }
            // Get first clip from next playlist.

            let c_list = self.player_control.current_list.lock().unwrap();
            let first_node = c_list[0].clone();

            drop(c_list);

            self.player_control.current_index.store(0, Ordering::SeqCst);
            self.current_node = gen_source(
                &self.config,
                first_node,
                &self.playout_stat,
                &self.player_control,
                0,
            );
            self.last_next_ad();
            self.current_node.last_ad = self.last_node_ad;
            self.player_control.current_index.store(1, Ordering::SeqCst);

            Some(self.current_node.clone())
        }
    }
}

/// Prepare input clip:
///
/// - check begin and length from clip
/// - return clip only if we are in 24 hours time range
fn timed_source(
    node: Media,
    config: &PlayoutConfig,
    last: bool,
    playout_stat: &PlayoutStatus,
    player_control: &PlayerControl,
    last_index: usize,
) -> Media {
    let (delta, total_delta) = get_delta(config, &node.begin.unwrap());
    let mut shifted_delta = delta;
    let mut new_node = node.clone();
    new_node.process = Some(false);

    trace!("Node begin: {}", node.begin.unwrap());
    trace!("timed source is last: {last}");

    if config.playlist.length.contains(':') {
        let time_shift = playout_stat.time_shift.lock().unwrap();

        if *playout_stat.current_date.lock().unwrap() == *playout_stat.date.lock().unwrap()
            && *time_shift != 0.0
        {
            shifted_delta = delta - *time_shift;

            debug!("Delta: <yellow>{shifted_delta:.3}</>, shifted: <yellow>{delta:.3}</>");
        } else {
            debug!("Delta: <yellow>{shifted_delta:.3}</>");
        }

        if config.general.stop_threshold > 0.0
            && shifted_delta.abs() > config.general.stop_threshold
        {
            error!("Clip begin out of sync for <yellow>{delta:.3}</> seconds.");

            new_node.cmd = None;

            return new_node;
        }
    }

    if (total_delta > node.out - node.seek && !last)
        || node.index.unwrap() < 2
        || !config.playlist.length.contains(':')
    {
        // when we are in the 24 hour range, get the clip
        new_node.process = Some(true);
        new_node = gen_source(config, node, playout_stat, player_control, last_index);
    } else if total_delta <= 0.0 {
        info!("Begin is over play time, skip: {}", node.source);
    } else if total_delta < node.duration - node.seek || last {
        new_node = handle_list_end(
            config,
            node,
            total_delta,
            playout_stat,
            player_control,
            last_index,
        );
    }

    new_node
}

/// Generate the source CMD, or when clip not exist, get a dummy.
pub fn gen_source(
    config: &PlayoutConfig,
    mut node: Media,
    playout_stat: &PlayoutStatus,
    player_control: &PlayerControl,
    last_index: usize,
) -> Media {
    let node_index = node.index.unwrap_or_default();
    let mut duration = node.out - node.seek;

    if duration < 1.0 {
        warn!("Clip is less then 1 second long (<yellow>{duration:.3}</>), adjust length.");

        duration = 1.2;

        if node.seek > 1.0 {
            node.seek -= 1.0;
        } else {
            node.out += 1.0;
        }
    }

    trace!("Clip new length: {duration}, duration: {}", node.duration);

    if node.probe.is_none() && !node.source.is_empty() {
        if let Err(e) = node.add_probe(true) {
            trace!("{e:?}");
        };
    } else {
        trace!("Node has a probe...")
    }

    // separate if condition, because of node.add_probe() in last condition
    if node.probe.is_some() {
        if node
            .source
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .filter(|c| IMAGE_FORMAT.contains(&c.as_str()))
            .is_some()
        {
            node.cmd = Some(loop_image(&node));
        } else {
            node.cmd = Some(seek_and_length(&mut node));
        }
    } else {
        trace!("clip index: {node_index} | last index: {last_index}");

        // Last index is the index from the last item from the node list.
        if node_index < last_index {
            error!("Source not found: <b><magenta>{}</></b>", node.source);
        }

        let mut filler_list = vec![];

        match player_control.filler_list.try_lock() {
            Ok(list) => filler_list = list.to_vec(),
            Err(e) => error!("Lock filler list error: {e}"),
        }

        if config.storage.filler.is_dir() && !filler_list.is_empty() {
            let filler_index = player_control.filler_index.fetch_add(1, Ordering::SeqCst);
            let mut filler_media = filler_list[filler_index].clone();

            trace!("take filler: {}", filler_media.source);

            // Set list_init to true, to stay in sync.
            playout_stat.list_init.store(true, Ordering::SeqCst);

            if filler_index == filler_list.len() - 1 {
                player_control.filler_index.store(0, Ordering::SeqCst)
            }

            if filler_media.probe.is_none() {
                if let Err(e) = filler_media.add_probe(false) {
                    error!("{e:?}");
                };
            }

            if filler_media.duration > duration {
                filler_media.out = duration;
            }

            node.source = filler_media.source;
            node.seek = 0.0;
            node.out = filler_media.out;
            node.duration = filler_media.duration;
            node.cmd = Some(loop_filler(&node));
            node.probe = filler_media.probe;
        } else {
            match MediaProbe::new(&config.storage.filler.to_string_lossy()) {
                Ok(probe) => {
                    if config
                        .storage
                        .filler
                        .to_string_lossy()
                        .to_string()
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_lowercase())
                        .filter(|c| IMAGE_FORMAT.contains(&c.as_str()))
                        .is_some()
                    {
                        node.source = config.storage.filler.clone().to_string_lossy().to_string();
                        node.cmd = Some(loop_image(&node));
                        node.probe = Some(probe);
                    } else if let Some(filler_duration) = probe
                        .clone()
                        .format
                        .duration
                        .and_then(|d| d.parse::<f64>().ok())
                    {
                        // Create placeholder from config filler.
                        let mut filler_out = filler_duration;

                        if filler_duration > duration {
                            filler_out = duration;
                        }

                        node.source = config.storage.filler.clone().to_string_lossy().to_string();
                        node.seek = 0.0;
                        node.out = filler_out;
                        node.duration = filler_duration;
                        node.cmd = Some(loop_filler(&node));
                        node.probe = Some(probe);
                    } else {
                        // Create colored placeholder.
                        let (source, cmd) = gen_dummy(config, duration);
                        node.source = source;
                        node.cmd = Some(cmd);
                    }
                }
                Err(e) => {
                    // Create colored placeholder.
                    error!("Filler error: {e}");

                    let mut dummy_duration = 60.0;

                    if dummy_duration > duration {
                        dummy_duration = duration;
                    }

                    let (source, cmd) = gen_dummy(config, dummy_duration);
                    node.seek = 0.0;
                    node.out = dummy_duration;
                    node.duration = dummy_duration;
                    node.source = source;
                    node.cmd = Some(cmd);
                }
            }
        }

        warn!(
            "Generate filler with <yellow>{:.2}</> seconds length!",
            node.out
        );
    }

    node.add_filter(config, &playout_stat.chain);

    trace!(
        "return gen_source: {}, seek: {}, out: {}",
        node.source,
        node.seek,
        node.out,
    );

    node
}

/// Handle init clip, but this clip can be the last one in playlist,
/// this we have to figure out and calculate the right length.
fn handle_list_init(
    config: &PlayoutConfig,
    mut node: Media,
    playout_stat: &PlayoutStatus,
    player_control: &PlayerControl,
    last_index: usize,
) -> Media {
    debug!("Playlist init");
    let (_, total_delta) = get_delta(config, &node.begin.unwrap());
    let mut out = node.out;

    if node.out - node.seek > total_delta {
        out = total_delta + node.seek;
    }

    node.out = out;

    gen_source(config, node, playout_stat, player_control, last_index)
}

/// when we come to last clip in playlist,
/// or when we reached total playtime,
/// we end up here
fn handle_list_end(
    config: &PlayoutConfig,
    mut node: Media,
    total_delta: f64,
    playout_stat: &PlayoutStatus,
    player_control: &PlayerControl,
    last_index: usize,
) -> Media {
    debug!("Playlist end");

    let mut out = if node.seek > 0.0 {
        node.seek + total_delta
    } else {
        if node.duration > total_delta {
            warn!("Adjust clip duration to: <yellow>{total_delta:.2}</>");
        }

        total_delta
    };

    // out can't be longer then duration
    if out > node.duration {
        out = node.duration
    }

    if node.duration > total_delta && total_delta > 1.0 && node.duration - node.seek >= total_delta
    {
        node.out = out;
    } else {
        warn!("Playlist is not long enough: <yellow>{total_delta:.2}</> seconds needed");
    }

    node.process = Some(true);

    gen_source(config, node, playout_stat, player_control, last_index)
}
