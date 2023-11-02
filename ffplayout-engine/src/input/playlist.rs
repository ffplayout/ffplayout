use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use serde_json::json;
use simplelog::*;

use ffplayout_lib::utils::{
    controller::PlayerControl, gen_dummy, get_delta, get_sec, is_close, is_remote,
    json_serializer::read_json, loop_filler, loop_image, modified_time, seek_and_length,
    valid_source, Media, MediaProbe, PlayoutConfig, PlayoutStatus, DUMMY_LEN, IMAGE_FORMAT,
};

/// Struct for current playlist.
///
/// Here we prepare the init clip and build a iterator where we pull our clips.
#[derive(Debug)]
pub struct CurrentProgram {
    config: PlayoutConfig,
    start_sec: f64,
    json_mod: Option<String>,
    json_path: Option<String>,
    json_date: String,
    player_control: PlayerControl,
    current_node: Media,
    is_terminated: Arc<AtomicBool>,
    playout_stat: PlayoutStatus,
}

/// Prepare a playlist iterator.
impl CurrentProgram {
    pub fn new(
        config: &PlayoutConfig,
        playout_stat: PlayoutStatus,
        is_terminated: Arc<AtomicBool>,
        player_control: &PlayerControl,
    ) -> Self {
        let json = read_json(config, None, is_terminated.clone(), true, false);

        if let Some(file) = &json.current_file {
            info!("Read Playlist: <b><magenta>{}</></b>", file);
        }

        *player_control.current_list.lock().unwrap() = json.program;
        *playout_stat.current_date.lock().unwrap() = json.date.clone();

        if *playout_stat.date.lock().unwrap() != json.date {
            let data = json!({
                "time_shift": 0.0,
                "date": json.date,
            });

            let json: String = serde_json::to_string(&data).expect("Serialize status data failed");
            if let Err(e) = fs::write(config.general.stat_file.clone(), json) {
                error!("Unable to write status file: {e}");
            };
        }

        Self {
            config: config.clone(),
            start_sec: json.start_sec.unwrap(),
            json_mod: json.modified,
            json_path: json.current_file,
            json_date: json.date,
            player_control: player_control.clone(),
            current_node: Media::new(0, "", false),
            is_terminated,
            playout_stat,
        }
    }

    // Check if playlist file got updated, and when yes we reload it and setup everything in place.
    fn check_update(&mut self, seek: bool) {
        if self.json_path.is_none() {
            // If the playlist was missing, we check here to see if it came back.
            let json = read_json(&self.config, None, self.is_terminated.clone(), seek, false);

            if let Some(file) = &json.current_file {
                info!("Read Playlist: <b><magenta>{file}</></b>");
            }

            self.json_path = json.current_file;
            self.json_mod = json.modified;
            *self.player_control.current_list.lock().unwrap() = json.program;
        } else if Path::new(&self.json_path.clone().unwrap()).is_file()
            || is_remote(&self.json_path.clone().unwrap())
        {
            // If the playlist exists, we check here if it has been modified.
            let mod_time = modified_time(&self.json_path.clone().unwrap());

            if self.json_mod != mod_time {
                // when playlist has changed, reload it
                info!(
                    "Reload playlist <b><magenta>{}</></b>",
                    self.json_path.clone().unwrap()
                );

                let json = read_json(
                    &self.config,
                    self.json_path.clone(),
                    self.is_terminated.clone(),
                    false,
                    false,
                );

                self.json_mod = json.modified;
                *self.player_control.current_list.lock().unwrap() = json.program;

                self.playout_stat.list_init.store(true, Ordering::SeqCst);
            }
        } else {
            // If the playlist disappears after normal run, we end up here.
            trace!("check_update, missing playlist");
            error!(
                "Playlist <b><magenta>{}</></b> not exist!",
                self.json_path.clone().unwrap()
            );

            let media = Media::new(0, "", false);

            self.json_mod = None;
            self.json_path = None;
            self.current_node = media.clone();
            self.playout_stat.list_init.store(true, Ordering::SeqCst);
            self.player_control.current_index.store(0, Ordering::SeqCst);
            *self.player_control.current_list.lock().unwrap() = vec![media];
        }
    }

    // Check if day is past and it is time for a new playlist.
    fn check_for_next_playlist(&mut self) -> bool {
        let current_time = get_sec();
        let start_sec = self.config.playlist.start_sec.unwrap();
        let target_length = self.config.playlist.length_sec.unwrap();
        let (delta, total_delta) = get_delta(&self.config, &current_time);
        let mut duration = self.current_node.out;
        let mut next = false;

        if self.current_node.duration > self.current_node.out {
            duration = self.current_node.duration
        }

        trace!("delta: {delta}, total_delta: {total_delta}");

        let mut next_start =
            self.current_node.begin.unwrap_or_default() - start_sec + duration + delta;

        if self.player_control.current_index.load(Ordering::SeqCst)
            == self.player_control.current_list.lock().unwrap().len() - 1
        {
            next_start += self.config.general.stop_threshold;
        }

        trace!("next_start: {next_start}, target_length: {target_length}");

        // Check if we over the target length or we are close to it, if so we load the next playlist.
        if next_start >= target_length
            || is_close(total_delta, 0.0, 2.0)
            || is_close(total_delta, target_length, 2.0)
        {
            trace!("get next day");
            next = true;

            let json = read_json(&self.config, None, self.is_terminated.clone(), false, true);

            if let Some(file) = &json.current_file {
                info!("Read next Playlist: <b><magenta>{}</></b>", file);
            }

            let data = json!({
                "time_shift": 0.0,
                "date": json.date,
            });

            *self.playout_stat.current_date.lock().unwrap() = json.date.clone();
            *self.playout_stat.time_shift.lock().unwrap() = 0.0;
            let status_data: String =
                serde_json::to_string(&data).expect("Serialize status data failed");

            if let Err(e) = fs::write(self.config.general.stat_file.clone(), status_data) {
                error!("Unable to write status file: {e}");
            };

            self.json_path = json.current_file.clone();
            self.json_mod = json.modified;
            self.json_date = json.date;
            *self.player_control.current_list.lock().unwrap() = json.program;
            self.player_control.current_index.store(0, Ordering::SeqCst);

            if json.current_file.is_none() {
                self.playout_stat.list_init.store(true, Ordering::SeqCst);
            } else {
                self.playout_stat.list_init.store(false, Ordering::SeqCst);
            }
        }

        next
    }

    // Check if last and/or next clip is a advertisement.
    fn last_next_ad(&mut self) {
        let index = self.player_control.current_index.load(Ordering::SeqCst);
        let current_list = self.player_control.current_list.lock().unwrap();

        if index + 1 < current_list.len() && &current_list[index + 1].category == "advertisement" {
            self.current_node.next_ad = Some(true);
        }

        if index > 0
            && index < current_list.len()
            && &current_list[index - 1].category == "advertisement"
        {
            self.current_node.last_ad = Some(true);
        }
    }

    // Get current time and when we are before start time,
    // we add full seconds of a day to it.
    fn get_current_time(&mut self) -> f64 {
        let mut time_sec = get_sec();

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
    fn init_clip(&mut self) {
        trace!("init_clip");
        self.get_current_clip();

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
            node_clone.seek = time_sec
                - (node_clone.begin.unwrap() - *self.playout_stat.time_shift.lock().unwrap());

            self.current_node = handle_list_init(
                &self.config,
                node_clone,
                &self.playout_stat.chain,
                &self.player_control,
                last_index,
            );
        }
    }
}

/// Build the playlist iterator
impl Iterator for CurrentProgram {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        self.check_update(self.playout_stat.list_init.load(Ordering::SeqCst));

        if self.playout_stat.list_init.load(Ordering::SeqCst) {
            trace!("Init playlist, from next iterator");
            if self.json_path.is_some() {
                self.init_clip();
            }

            if self.playout_stat.list_init.load(Ordering::SeqCst) {
                // On init load, playlist could be not long enough,
                // so we check if we can take the next playlist already,
                // or we fill the gap with a dummy.
                let last_index = self.player_control.current_list.lock().unwrap().len() - 1;
                self.current_node =
                    self.player_control.current_list.lock().unwrap()[last_index].clone();
                let new_node = self.player_control.current_list.lock().unwrap()[last_index].clone();
                let new_length = new_node.begin.unwrap_or_default() + new_node.duration;
                trace!("Init playlist after playlist end");

                let next_playlist = self.check_for_next_playlist();

                if new_length
                    >= self.config.playlist.length_sec.unwrap()
                        + self.config.playlist.start_sec.unwrap()
                {
                    self.init_clip();
                } else if next_playlist
                    && self.player_control.current_list.lock().unwrap().len() > 1
                {
                    let index = self
                        .player_control
                        .current_index
                        .fetch_add(1, Ordering::SeqCst);

                    self.current_node = gen_source(
                        &self.config,
                        self.player_control.current_list.lock().unwrap()[index].clone(),
                        &self.playout_stat.chain,
                        &self.player_control,
                        0,
                    );

                    return Some(self.current_node.clone());
                } else {
                    // fill missing length from playlist
                    let mut current_time = get_sec();
                    let (_, total_delta) = get_delta(&self.config, &current_time);

                    trace!("Total delta on list init: {total_delta}");

                    let out = if DUMMY_LEN > total_delta {
                        total_delta
                    } else {
                        DUMMY_LEN
                    };

                    let duration = out + 0.001;

                    if self.json_path.is_some() {
                        // When playlist is missing, we always need to init the playlist the next iteration.
                        self.playout_stat.list_init.store(true, Ordering::SeqCst);
                    }

                    if self.config.playlist.start_sec.unwrap() > current_time {
                        current_time += self.config.playlist.length_sec.unwrap() + 1.0;
                    }

                    let mut nodes = self.player_control.current_list.lock().unwrap();
                    let index = nodes.len();

                    let mut media = Media::new(index, "", false);
                    media.begin = Some(current_time);
                    media.duration = duration;
                    media.out = out;

                    self.current_node = gen_source(
                        &self.config,
                        media,
                        &self.playout_stat.chain,
                        &self.player_control,
                        last_index,
                    );

                    nodes.push(self.current_node.clone());
                    self.player_control
                        .current_index
                        .store(nodes.len(), Ordering::SeqCst);
                }
            }

            self.last_next_ad();

            return Some(self.current_node.clone());
        }

        if self.player_control.current_index.load(Ordering::SeqCst)
            < self.player_control.current_list.lock().unwrap().len()
        {
            self.check_for_next_playlist();
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
            let last_playlist = self.json_path.clone();
            let last_ad = self.current_node.last_ad;
            self.check_for_next_playlist();
            let (_, total_delta) =
                get_delta(&self.config, &self.config.playlist.start_sec.unwrap());

            if !self.config.playlist.infinit
                && last_playlist == self.json_path
                && total_delta.abs() > 1.0
            {
                trace!("Total delta on list end: {total_delta}");

                // Playlist is to early finish,
                // and if we have to fill it with a placeholder.
                let index = self.player_control.current_index.load(Ordering::SeqCst);
                self.current_node = Media::new(index, "", false);
                self.current_node.begin = Some(get_sec());

                let out = if DUMMY_LEN > total_delta {
                    total_delta
                } else {
                    DUMMY_LEN
                };

                let duration = out + 0.001;

                self.current_node.duration = duration;
                self.current_node.out = out;
                self.current_node = gen_source(
                    &self.config,
                    self.current_node.clone(),
                    &self.playout_stat.chain,
                    &self.player_control,
                    0,
                );
                self.player_control
                    .current_list
                    .lock()
                    .unwrap()
                    .push(self.current_node.clone());
                self.last_next_ad();

                self.current_node.last_ad = last_ad;
                self.current_node
                    .add_filter(&self.config, &self.playout_stat.chain);

                self.player_control
                    .current_index
                    .fetch_add(1, Ordering::SeqCst);

                return Some(self.current_node.clone());
            }

            // Get first clip from next playlist.
            self.player_control.current_index.store(0, Ordering::SeqCst);
            self.current_node = gen_source(
                &self.config,
                self.player_control.current_list.lock().unwrap()[0].clone(),
                &self.playout_stat.chain,
                &self.player_control,
                0,
            );
            self.last_next_ad();
            self.current_node.last_ad = last_ad;

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
        new_node = gen_source(
            config,
            node,
            &playout_stat.chain,
            player_control,
            last_index,
        );
    } else if total_delta <= 0.0 {
        info!("Begin is over play time, skip: {}", node.source);
    } else if total_delta < node.duration - node.seek || last {
        new_node = handle_list_end(
            config,
            node,
            total_delta,
            &playout_stat.chain,
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
    filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
    player_control: &PlayerControl,
    last_index: usize,
) -> Media {
    let duration = node.out - node.seek;

    trace!("Clip out: {duration}, duration: {}", node.duration);

    if valid_source(&node.source) {
        node.add_probe();

        if node
            .source
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .filter(|c| IMAGE_FORMAT.contains(&c.as_str()))
            .is_some()
        {
            node.cmd = Some(loop_image(&node));
        } else {
            node.cmd = Some(seek_and_length(&node));
        }
    } else {
        trace!(
            "clip index: {:?} | last index: {:?}",
            node.index.unwrap_or_default(),
            last_index
        );

        // last index is the index from the last item from the node list.
        if node.index.unwrap_or_default() < last_index {
            error!("Source not found: <b><magenta>\"{}\"</></b>", node.source);
        }

        let filler_source = &config.storage.filler;

        if filler_source.is_dir() && !player_control.filler_list.lock().unwrap().is_empty() {
            let filler_index = player_control.filler_index.fetch_add(1, Ordering::SeqCst);
            let mut filler_media = player_control.filler_list.lock().unwrap()[filler_index].clone();

            if filler_index == player_control.filler_list.lock().unwrap().len() - 1 {
                player_control.filler_index.store(0, Ordering::SeqCst)
            }

            if filler_media.probe.is_none() {
                filler_media.add_probe();
            }

            if node.duration > duration && filler_media.duration > duration {
                filler_media.out = duration;
            }

            // If necessary filler clip will be injected to the current list,
            // original clip get new begin and seek value, to keep everything in sync.

            if node.index.unwrap_or_default() < last_index {
                player_control.current_list.lock().unwrap()[node.index.unwrap_or_default()].begin =
                    Some(node.begin.unwrap_or_default() + filler_media.out);

                player_control.current_list.lock().unwrap()[node.index.unwrap_or_default()].seek =
                    node.seek + filler_media.out;
            }

            node.source = filler_media.source;
            node.seek = 0.0;
            node.out = filler_media.out;
            node.duration = filler_media.duration;
            node.cmd = Some(loop_filler(&node));
            node.probe = filler_media.probe;

            if node.out < duration - 1.0 && node.index.unwrap_or_default() < last_index {
                player_control
                    .current_list
                    .lock()
                    .unwrap()
                    .insert(node.index.unwrap_or_default(), node.clone());

                for (i, item) in (*player_control.current_list.lock().unwrap())
                    .iter_mut()
                    .enumerate()
                {
                    item.index = Some(i);
                }
            }
        } else if filler_source.is_file() {
            let probe = MediaProbe::new(&config.storage.filler.to_string_lossy());

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
                .and_then(|f| f.duration)
                .and_then(|d| d.parse::<f64>().ok())
            {
                // Create placeholder from config filler.
                node.source = config.storage.filler.clone().to_string_lossy().to_string();
                node.out = duration;
                node.duration = filler_duration;
                node.cmd = Some(loop_filler(&node));
                node.probe = Some(probe);
            } else {
                // Create colored placeholder.
                let (source, cmd) = gen_dummy(config, duration);
                node.source = source;
                node.cmd = Some(cmd);
            }
        } else {
            // Create colored placeholder.
            let (source, cmd) = gen_dummy(config, duration);
            node.source = source;
            node.cmd = Some(cmd);
        }

        warn!(
            "Generate filler with <yellow>{:.2}</> seconds length!",
            node.out
        );
    }

    node.add_filter(config, filter_chain);

    if duration < 1.0 {
        warn!(
            "Clip is less then 1 second long (<yellow>{duration:.3}</>), skip: <b><magenta>{}</></b>",
            node.source
        );

        node.process = Some(false);
    }

    node
}

/// Handle init clip, but this clip can be the last one in playlist,
/// this we have to figure out and calculate the right length.
fn handle_list_init(
    config: &PlayoutConfig,
    mut node: Media,
    filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
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

    gen_source(config, node, filter_chain, player_control, last_index)
}

/// when we come to last clip in playlist,
/// or when we reached total playtime,
/// we end up here
fn handle_list_end(
    config: &PlayoutConfig,
    mut node: Media,
    total_delta: f64,
    filter_chain: &Option<Arc<Mutex<Vec<String>>>>,
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

    gen_source(config, node, filter_chain, player_control, last_index)
}
