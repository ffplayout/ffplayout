use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use serde_json::json;
use simplelog::*;

use ffplayout_lib::utils::{
    check_sync, gen_dummy, get_delta, get_sec, is_close, is_remote, json_serializer::read_json,
    loop_filler, loop_image, modified_time, seek_and_length, valid_source, Media, MediaProbe,
    PlayoutConfig, PlayoutStatus, DUMMY_LEN, IMAGE_FORMAT,
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
    pub nodes: Arc<Mutex<Vec<Media>>>,
    current_node: Media,
    index: Arc<AtomicUsize>,
    is_terminated: Arc<AtomicBool>,
    playout_stat: PlayoutStatus,
}

impl CurrentProgram {
    pub fn new(
        config: &PlayoutConfig,
        playout_stat: PlayoutStatus,
        is_terminated: Arc<AtomicBool>,
        current_list: Arc<Mutex<Vec<Media>>>,
        global_index: Arc<AtomicUsize>,
    ) -> Self {
        let json = read_json(config, None, is_terminated.clone(), true, 0.0);

        if let Some(file) = &json.current_file {
            info!("Read Playlist: <b><magenta>{}</></b>", file);
        }

        *current_list.lock().unwrap() = json.program;
        *playout_stat.current_date.lock().unwrap() = json.date.clone();

        if *playout_stat.date.lock().unwrap() != json.date {
            let data = json!({
                "time_shift": 0.0,
                "date": json.date,
            });

            let json: String = serde_json::to_string(&data).expect("Serialize status data failed");
            if let Err(e) = fs::write(config.general.stat_file.clone(), &json) {
                error!("Unable to write status file: {e}");
            };
        }

        Self {
            config: config.clone(),
            start_sec: json.start_sec.unwrap(),
            json_mod: json.modified,
            json_path: json.current_file,
            json_date: json.date,
            nodes: current_list,
            current_node: Media::new(0, String::new(), false),
            index: global_index,
            is_terminated,
            playout_stat,
        }
    }

    // Check if playlist file got updated, and when yes we reload it and setup everything in place.
    fn check_update(&mut self, seek: bool) {
        if self.json_path.is_none() {
            let json = read_json(&self.config, None, self.is_terminated.clone(), seek, 0.0);

            if let Some(file) = &json.current_file {
                info!("Read Playlist: <b><magenta>{}</></b>", file);
            }

            self.json_path = json.current_file;
            self.json_mod = json.modified;
            *self.nodes.lock().unwrap() = json.program;
        } else if Path::new(&self.json_path.clone().unwrap()).is_file()
            || is_remote(&self.json_path.clone().unwrap())
        {
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
                    0.0,
                );

                self.json_mod = json.modified;
                *self.nodes.lock().unwrap() = json.program;

                self.playout_stat.list_init.store(true, Ordering::SeqCst);
            }
        } else {
            error!(
                "Playlist <b><magenta>{}</></b> not exists!",
                self.json_path.clone().unwrap()
            );
            let mut media = Media::new(0, String::new(), false);
            media.begin = Some(get_sec());
            media.duration = DUMMY_LEN;
            media.out = DUMMY_LEN;

            self.json_path = None;
            *self.nodes.lock().unwrap() = vec![media.clone()];
            self.current_node = media;
            self.playout_stat.list_init.store(true, Ordering::SeqCst);
            self.index.store(0, Ordering::SeqCst);
        }
    }

    // Check if day is past and it is time for a new playlist.
    fn check_for_next_playlist(&mut self) {
        let current_time = get_sec();
        let start_sec = self.config.playlist.start_sec.unwrap();
        let target_length = self.config.playlist.length_sec.unwrap();
        let (delta, total_delta) = get_delta(&self.config, &current_time);
        let mut duration = self.current_node.out;

        if self.current_node.duration > self.current_node.out {
            duration = self.current_node.duration
        }

        let mut next_start = self.current_node.begin.unwrap() - start_sec + duration + delta;

        if self.index.load(Ordering::SeqCst) == self.nodes.lock().unwrap().len() - 1 {
            next_start += self.config.general.stop_threshold;
        }

        if next_start >= target_length
            || is_close(total_delta, 0.0, 2.0)
            || is_close(total_delta, target_length, 2.0)
        {
            let json = read_json(
                &self.config,
                None,
                self.is_terminated.clone(),
                false,
                next_start,
            );

            if let Some(file) = &json.current_file {
                info!("Read Playlist: <b><magenta>{}</></b>", file);
            }

            let data = json!({
                "time_shift": 0.0,
                "date": json.date,
            });

            *self.playout_stat.current_date.lock().unwrap() = json.date.clone();
            *self.playout_stat.time_shift.lock().unwrap() = 0.0;
            let status_data: String =
                serde_json::to_string(&data).expect("Serialize status data failed");

            if let Err(e) = fs::write(self.config.general.stat_file.clone(), &status_data) {
                error!("Unable to write status file: {e}");
            };

            self.json_path = json.current_file.clone();
            self.json_mod = json.modified;
            self.json_date = json.date;
            *self.nodes.lock().unwrap() = json.program;
            self.index.store(0, Ordering::SeqCst);

            if json.current_file.is_none() {
                self.playout_stat.list_init.store(true, Ordering::SeqCst);
            }
        }
    }

    // Check if last and/or next clip is a advertisement.
    fn last_next_ad(&mut self) {
        let index = self.index.load(Ordering::SeqCst);
        let current_list = self.nodes.lock().unwrap();

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

        for (i, item) in self.nodes.lock().unwrap().iter_mut().enumerate() {
            if item.begin.unwrap() + item.out - item.seek > time_sec {
                self.playout_stat.list_init.store(false, Ordering::SeqCst);
                self.index.store(i, Ordering::SeqCst);

                break;
            }
        }
    }

    // Prepare init clip.
    fn init_clip(&mut self) {
        self.get_current_clip();

        if !self.playout_stat.list_init.load(Ordering::SeqCst) {
            let time_sec = self.get_current_time();
            let index = self.index.fetch_add(1, Ordering::SeqCst);

            // de-instance node to preserve original values in list
            let mut node_clone = self.nodes.lock().unwrap()[index].clone();

            node_clone.seek = time_sec - node_clone.begin.unwrap();
            self.current_node =
                handle_list_init(&self.config, node_clone, &self.playout_stat.chain);
        }
    }
}

/// Build the playlist iterator
impl Iterator for CurrentProgram {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        self.check_update(self.playout_stat.list_init.load(Ordering::SeqCst));

        if self.playout_stat.list_init.load(Ordering::SeqCst) {
            if self.json_path.is_some() {
                self.init_clip();
            }

            if self.playout_stat.list_init.load(Ordering::SeqCst) {
                // On init load, playlist could be not long enough,
                // so we check if we can take the next playlist already,
                // or we fill the gap with a dummy.
                let last_index = self.nodes.lock().unwrap().len() - 1;
                self.current_node = self.nodes.lock().unwrap()[last_index].clone();
                let new_node = self.nodes.lock().unwrap()[last_index].clone();
                let new_length = new_node.begin.unwrap() + new_node.duration;

                self.check_for_next_playlist();

                if new_length
                    >= self.config.playlist.length_sec.unwrap()
                        + self.config.playlist.start_sec.unwrap()
                {
                    self.init_clip();
                } else {
                    // fill missing length from playlist
                    let mut current_time = get_sec();
                    let (_, total_delta) = get_delta(&self.config, &current_time);
                    let mut duration = DUMMY_LEN;

                    if DUMMY_LEN > total_delta {
                        duration = total_delta;
                        self.playout_stat.list_init.store(false, Ordering::SeqCst);
                    }

                    if self.config.playlist.start_sec.unwrap() > current_time {
                        current_time += self.config.playlist.length_sec.unwrap() + 1.0;
                    }

                    let mut media = Media::new(0, String::new(), false);
                    media.begin = Some(current_time);
                    media.duration = duration;
                    media.out = duration;

                    self.current_node = gen_source(&self.config, media, &self.playout_stat.chain);
                    let mut nodes = self.nodes.lock().unwrap();
                    nodes.push(self.current_node.clone());
                    self.index.store(nodes.len(), Ordering::SeqCst);
                }
            }

            self.last_next_ad();

            return Some(self.current_node.clone());
        }

        if self.index.load(Ordering::SeqCst) < self.nodes.lock().unwrap().len() {
            self.check_for_next_playlist();
            let mut is_last = false;
            let index = self.index.load(Ordering::SeqCst);
            let nodes = self.nodes.lock().unwrap();

            if index == nodes.len() - 1 {
                is_last = true
            }

            self.current_node = timed_source(
                nodes[index].clone(),
                &self.config,
                is_last,
                &self.playout_stat,
            );

            drop(nodes);
            self.last_next_ad();
            self.index.fetch_add(1, Ordering::SeqCst);

            Some(self.current_node.clone())
        } else {
            let last_playlist = self.json_path.clone();
            let last_ad = self.current_node.last_ad;
            self.check_for_next_playlist();
            let (_, total_delta) =
                get_delta(&self.config, &self.config.playlist.start_sec.unwrap());

            if !self.config.playlist.infinit
                && last_playlist == self.json_path
                && total_delta.abs() > self.config.general.stop_threshold
            {
                // Test if playlist is to early finish,
                // and if we have to fill it with a placeholder.
                let index = self.index.load(Ordering::SeqCst);
                self.current_node = Media::new(index, String::new(), false);
                self.current_node.begin = Some(get_sec());
                let mut duration = total_delta.abs();

                if duration > DUMMY_LEN {
                    duration = DUMMY_LEN;
                }
                self.current_node.duration = duration;
                self.current_node.out = duration;
                self.current_node = gen_source(
                    &self.config,
                    self.current_node.clone(),
                    &self.playout_stat.chain,
                );
                self.nodes.lock().unwrap().push(self.current_node.clone());
                self.last_next_ad();

                self.current_node.last_ad = last_ad;
                self.current_node
                    .add_filter(&self.config, &self.playout_stat.chain);

                self.index.fetch_add(1, Ordering::SeqCst);

                return Some(self.current_node.clone());
            }

            self.index.store(0, Ordering::SeqCst);
            self.current_node = gen_source(
                &self.config,
                self.nodes.lock().unwrap()[0].clone(),
                &self.playout_stat.chain,
            );
            self.last_next_ad();
            self.current_node.last_ad = last_ad;

            self.index.store(1, Ordering::SeqCst);

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
) -> Media {
    let (delta, total_delta) = get_delta(config, &node.begin.unwrap());
    let mut shifted_delta = delta;
    let mut new_node = node.clone();
    new_node.process = Some(false);

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

        let sync = check_sync(config, shifted_delta);

        if !sync {
            new_node.cmd = None;

            return new_node;
        }
    }

    if (total_delta > node.out - node.seek && !last)
        || node.index.unwrap() < 2
        || !config.playlist.length.contains(':')
    {
        // when we are in the 24 hour range, get the clip
        new_node = gen_source(config, node, &playout_stat.chain);
        new_node.process = Some(true);
    } else if total_delta <= 0.0 {
        info!("Begin is over play time, skip: {}", node.source);
    } else if total_delta < node.duration - node.seek || last {
        new_node = handle_list_end(config, node, total_delta, &playout_stat.chain);
    }

    new_node
}

/// Generate the source CMD, or when clip not exist, get a dummy.
fn gen_source(
    config: &PlayoutConfig,
    mut node: Media,
    filter_chain: &Arc<Mutex<Vec<String>>>,
) -> Media {
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
        let duration = node.out - node.seek;
        let probe = MediaProbe::new(&config.storage.filler_clip);

        if node.source.is_empty() {
            warn!("Generate filler with <yellow>{duration:.2}</> seconds length!");
        } else {
            error!("Source not found: <b><magenta>{}</></b>", node.source);
        }

        if config
            .storage
            .filler_clip
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .filter(|c| IMAGE_FORMAT.contains(&c.as_str()))
            .is_some()
        {
            node.source = config.storage.filler_clip.clone();
            node.cmd = Some(loop_image(&node));
            node.probe = Some(probe);
        } else if let Some(length) = probe
            .clone()
            .format
            .and_then(|f| f.duration)
            .and_then(|d| d.parse::<f64>().ok())
        {
            // create placeholder from config filler.

            node.source = config.storage.filler_clip.clone();
            node.duration = length;
            node.out = duration;
            node.cmd = Some(loop_filler(&node));
            node.probe = Some(probe);
        } else {
            // create colored placeholder.
            let (source, cmd) = gen_dummy(config, duration);
            node.source = source;
            node.cmd = Some(cmd);
        }
    }

    node.add_filter(config, filter_chain);

    node
}

/// Handle init clip, but this clip can be the last one in playlist,
/// this we have to figure out and calculate the right length.
fn handle_list_init(
    config: &PlayoutConfig,
    mut node: Media,
    filter_chain: &Arc<Mutex<Vec<String>>>,
) -> Media {
    debug!("Playlist init");
    let (_, total_delta) = get_delta(config, &node.begin.unwrap());
    let mut out = node.out;

    if node.out - node.seek > total_delta {
        out = total_delta + node.seek;
    }

    node.out = out;
    gen_source(config, node, filter_chain)
}

/// when we come to last clip in playlist,
/// or when we reached total playtime,
/// we end up here
fn handle_list_end(
    config: &PlayoutConfig,
    mut node: Media,
    total_delta: f64,
    filter_chain: &Arc<Mutex<Vec<String>>>,
) -> Media {
    debug!("Playlist end");

    let mut out = if node.seek > 0.0 {
        node.seek + total_delta
    } else {
        warn!("Clip length is not in time, new duration is: <yellow>{total_delta:.2}</>");
        total_delta
    };

    // out can't be longer then duration
    if out > node.duration {
        out = node.duration
    }

    if node.duration > total_delta && total_delta > 1.0 && node.duration - node.seek >= total_delta
    {
        node.out = out;
    } else if node.duration > total_delta && total_delta < 1.0 {
        warn!(
            "Last clip less then 1 second long, skip: <b><magenta>{}</></b>",
            node.source
        );
        node.out = out;
        node.cmd = Some(seek_and_length(&node));

        node.process = Some(false);

        return node;
    } else {
        warn!("Playlist is not long enough: <yellow>{total_delta:.2}</> seconds needed");
    }

    node.process = Some(true);

    gen_source(config, node, filter_chain)
}
