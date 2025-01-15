use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use log::*;
use tokio::sync::Mutex;

use crate::player::{
    controller::ChannelManager,
    utils::{
        gen_dummy, get_delta, is_close, is_remote,
        json_serializer::{read_json, set_defaults},
        loop_filler, loop_image, modified_time,
        probe::MediaProbe,
        seek_and_length, time_in_seconds, JsonPlaylist, Media,
    },
};
use crate::utils::{
    config::{PlayoutConfig, IMAGE_FORMAT},
    logging::Target,
    s3_utils::S3_DEFAULT_PRESIGNEDURL_EXP,
};
use crate::{db::handles, utils::s3_utils};

/// Struct for current playlist.
///
/// Here we prepare the init clip and build a iterator where we pull our clips.
#[derive(Debug)]
pub struct CurrentProgram {
    id: i32,
    config: PlayoutConfig,
    manager: ChannelManager,
    start_sec: f64,
    length_sec: f64,
    json_playlist: JsonPlaylist,
    current_node: Media,
    is_terminated: Arc<AtomicBool>,
    last_json_path: Option<String>,
    last_node_ad: bool,
}

/// Prepare a playlist iterator.
impl CurrentProgram {
    pub async fn new(manager: ChannelManager) -> Self {
        let config = manager.config.lock().await.clone();
        let is_terminated = manager.is_terminated.clone();

        Self {
            id: config.general.channel_id,
            config: config.clone(),
            manager,
            start_sec: config.playlist.start_sec.unwrap(),
            length_sec: config.playlist.length_sec.unwrap(),
            json_playlist: JsonPlaylist::new(
                "1970-01-01".to_string(),
                config.playlist.start_sec.unwrap(),
            ),
            current_node: Media::default(),
            is_terminated,
            last_json_path: None,
            last_node_ad: false,
        }
    }

    // Check if there is no current playlist or file got updated,
    // and when is so load/reload it.
    async fn load_or_update_playlist(&mut self, seek: bool) {
        let mut get_current = false;
        let mut reload = false;

        if let Some(path) = self.json_playlist.path.clone() {
            if (Path::new(&path).is_file() || is_remote(&path))
                && self.json_playlist.modified != modified_time(&path).await
            {
                info!(target: Target::file_mail(), channel = self.id; "Reload playlist <b><magenta>{path}</></b>");
                self.manager.list_init.store(true, Ordering::SeqCst);
                get_current = true;
                reload = true;
            }
        } else {
            get_current = true;
        }

        if get_current {
            self.json_playlist = read_json(
                &mut self.config,
                self.manager.current_list.clone(),
                self.json_playlist.path.clone(),
                self.is_terminated.clone(),
                seek,
                false,
            )
            .await;

            if !reload {
                if let Some(file) = &self.json_playlist.path {
                    info!(target: Target::file_mail(), channel = self.id; "Read playlist: <b><magenta>{file}</></b>");
                }

                if *self
                    .manager
                    .channel
                    .lock()
                    .await
                    .last_date
                    .clone()
                    .unwrap_or_default()
                    != self.json_playlist.date
                {
                    self.set_status(self.json_playlist.date.clone()).await;
                }

                self.manager
                    .current_date
                    .lock()
                    .await
                    .clone_from(&self.json_playlist.date);
            }

            self.manager
                .current_list
                .lock()
                .await
                .clone_from(&self.json_playlist.program);

            if self.json_playlist.path.is_none() {
                trace!("missing playlist");

                self.current_node = Media::default();
                self.manager.list_init.store(true, Ordering::SeqCst);
                self.manager.current_index.store(0, Ordering::SeqCst);
            }
        }
    }

    // Check if day is past and it is time for a new playlist.
    async fn check_for_playlist(&mut self, seek: bool) -> bool {
        let (delta, total_delta) = get_delta(
            &self.config,
            &time_in_seconds(&self.config.channel.timezone),
        );
        let mut next = false;

        let mut duration = self.current_node.out;

        let node_index = self.current_node.index.unwrap_or_default();

        let mut next_start = self.current_node.begin.unwrap_or_default() - self.start_sec + delta;
        let last_index = self.manager.current_list.lock().await.len() - 1;

        if node_index > 0 && node_index == last_index {
            if self.current_node.duration >= self.current_node.out {
                duration = self.current_node.duration;
            }

            next_start += self.config.general.stop_threshold;
        }

        next_start += duration;

        trace!(
            "delta: {delta} | total_delta: {total_delta}, index: {node_index}, last index: {last_index} \n        next_start: {next_start} | length_sec: {} | source {}",
            self.length_sec,
            self.current_node.source
        );

        // Check if we over the target length or we are close to it, if so we load the next playlist.
        if !self.config.playlist.infinit
            && (next_start >= self.length_sec
                || is_close(total_delta, 0.0, 2.0)
                || is_close(total_delta, self.length_sec, 2.0))
        {
            trace!("get next day");
            next = true;

            self.json_playlist = read_json(
                &mut self.config,
                self.manager.current_list.clone(),
                None,
                self.is_terminated.clone(),
                false,
                true,
            )
            .await;

            if let Some(file) = &self.json_playlist.path {
                info!(target: Target::file_mail(), channel = self.id; "Read next playlist: <b><magenta>{file}</></b>");
            }

            self.manager.list_init.store(false, Ordering::SeqCst);
            self.set_status(self.json_playlist.date.clone()).await;

            self.manager
                .current_list
                .lock()
                .await
                .clone_from(&self.json_playlist.program);
            self.manager.current_index.store(0, Ordering::SeqCst);
        } else {
            self.load_or_update_playlist(seek).await;
        }

        next
    }

    async fn set_status(&mut self, date: String) {
        if self.manager.channel.lock().await.last_date != Some(date.clone())
            && self.manager.channel.lock().await.time_shift != 0.0
        {
            info!(target: Target::file_mail(), channel = self.id; "Reset playout status");
        }

        self.manager.current_date.lock().await.clone_from(&date);
        self.manager
            .channel
            .lock()
            .await
            .last_date
            .clone_from(&Some(date.clone()));
        self.manager.channel.lock().await.time_shift = 0.0;
        let db_pool = self.manager.db_pool.clone().unwrap();

        if let Err(e) =
            handles::update_stat(&db_pool, self.config.general.channel_id, Some(date), 0.0).await
        {
            error!(target: Target::file_mail(), channel = self.id; "Unable to write status: {e}");
        };
    }

    // Check if last and/or next clip is a advertisement.
    async fn last_next_ad(&mut self, node: &mut Media) {
        let index = self.manager.current_index.load(Ordering::SeqCst);
        let current_list = self.manager.current_list.lock().await;

        if index + 1 < current_list.len() && &current_list[index + 1].category == "advertisement" {
            node.next_ad = true;
        }

        if index > 0
            && index < current_list.len()
            && &current_list[index - 1].category == "advertisement"
        {
            node.last_ad = true;
        }
    }

    // Get current time and when we are before start time,
    // we add full seconds of a day to it.
    fn get_current_time(&mut self) -> f64 {
        let mut time_sec = time_in_seconds(&self.config.channel.timezone);

        if time_sec < self.start_sec {
            time_sec += 86400.0; // self.config.playlist.length_sec.unwrap();
        }

        time_sec
    }

    // On init or reload we need to seek for the current clip.
    async fn get_current_clip(&mut self) {
        let mut time_sec = self.get_current_time();
        let shift = self.manager.channel.lock().await.time_shift;

        if shift != 0.0 {
            info!(target: Target::file_mail(), channel = self.id; "Shift playlist start for <yellow>{shift:.3}</> seconds");
            time_sec += shift;
        }

        if self.config.playlist.infinit
            && self.json_playlist.length.unwrap() < 86400.0
            && time_sec > self.json_playlist.length.unwrap() + self.start_sec
        {
            self.recalculate_begin(true).await;
        }

        for (i, item) in self.manager.current_list.lock().await.iter().enumerate() {
            if item.begin.unwrap() + item.out - item.seek > time_sec {
                self.manager.list_init.store(false, Ordering::SeqCst);
                self.manager.current_index.store(i, Ordering::SeqCst);

                break;
            }
        }
    }

    // Prepare init clip.
    async fn init_clip(&mut self) -> bool {
        trace!("init_clip");
        self.get_current_clip().await;
        let mut is_filler = false;

        if !self.manager.list_init.load(Ordering::SeqCst) {
            let time_sec = self.get_current_time();
            let index = self.manager.current_index.load(Ordering::SeqCst);
            let nodes = self.manager.current_list.lock().await;
            let last_index = nodes.len() - 1;

            // de-instance node to preserve original values in list
            let mut node_clone = nodes[index].clone();

            // Important! When no manual drop is happen here, lock is still active in handle_list_init
            drop(nodes);

            trace!("Clip from init: {}", node_clone.source);

            node_clone.seek += time_sec
                - (node_clone.begin.unwrap() - self.manager.channel.lock().await.time_shift);

            self.last_next_ad(&mut node_clone).await;

            self.manager.current_index.fetch_add(1, Ordering::SeqCst);

            self.current_node =
                handle_list_init(&self.config, node_clone, &self.manager, last_index).await;

            if self
                .current_node
                .source
                .contains(&self.config.channel.storage.to_string_lossy().to_string())
                || self.current_node.source.contains("color=c=#121212")
            {
                is_filler = true;
            }
        }

        is_filler
    }

    async fn fill_end(&mut self, total_delta: f64) {
        // Fill end from playlist
        let index = self.manager.current_index.load(Ordering::SeqCst);
        let mut media = Media::new(index, "", false).await;
        media.begin = Some(time_in_seconds(&self.config.channel.timezone));
        media.duration = total_delta;
        media.out = total_delta;

        self.last_next_ad(&mut media).await;

        self.current_node = gen_source(&self.config, media, &self.manager, 0).await;

        self.manager
            .current_list
            .lock()
            .await
            .push(self.current_node.clone());

        self.current_node.last_ad = self.last_node_ad;
        self.current_node
            .add_filter(&self.config, &self.manager.filter_chain)
            .await;

        self.manager.current_index.fetch_add(1, Ordering::SeqCst);
    }

    async fn recalculate_begin(&mut self, extend: bool) {
        debug!(target: Target::file_mail(), channel = self.id; "Infinit playlist reaches end, recalculate clip begins. Extend: <yellow>{extend}</>");

        let mut time_sec = time_in_seconds(&self.config.channel.timezone);

        if extend {
            // Calculate the elapsed time since the playlist start
            let elapsed_sec = if time_sec >= self.start_sec {
                time_sec - self.start_sec
            } else {
                time_sec + 86400.0 - self.start_sec
            };

            // Time passed within the current playlist loop
            let time_in_current_loop = elapsed_sec % self.json_playlist.length.unwrap();

            // Adjust the start time so that the playlist starts at the correct point in time
            time_sec -= time_in_current_loop;
        }

        self.json_playlist.start_sec = Some(time_sec);
        set_defaults(&mut self.json_playlist);
        self.manager
            .current_list
            .lock()
            .await
            .clone_from(&self.json_playlist.program);
    }
}

/// Build the playlist iterator
impl async_iterator::Iterator for CurrentProgram {
    type Item = Media;

    async fn next(&mut self) -> Option<Self::Item> {
        self.last_json_path.clone_from(&self.json_playlist.path);
        self.last_node_ad = self.current_node.last_ad;
        self.check_for_playlist(self.manager.list_init.load(Ordering::SeqCst))
            .await;

        if self.manager.list_init.load(Ordering::SeqCst) {
            trace!("Init playlist, from next iterator");
            let init_clip_is_filler = match self.json_playlist.path {
                None => false,
                Some(_) => self.init_clip().await,
            };

            if self.manager.list_init.load(Ordering::SeqCst) && !init_clip_is_filler {
                // On init load, playlist could be not long enough, or clips are not found
                // so we fill the gap with a dummy.
                trace!("Init clip is no filler");

                let mut current_time = time_in_seconds(&self.config.channel.timezone);
                let (_, total_delta) = get_delta(&self.config, &current_time);

                if self.start_sec > current_time {
                    current_time += self.length_sec + 1.0;
                }

                let mut last_index = 0;
                let length = self.manager.current_list.lock().await.len();

                if length > 0 {
                    last_index = length - 1;
                }

                let mut media = Media::new(length, "", false).await;
                media.begin = Some(current_time);
                media.duration = total_delta;
                media.out = total_delta;

                self.last_next_ad(&mut media).await;

                self.current_node =
                    gen_source(&self.config, media, &self.manager, last_index).await;
            }
        } else if self.manager.current_index.load(Ordering::SeqCst)
            < self.manager.current_list.lock().await.len()
        {
            // get next clip from current playlist

            let mut is_last = false;
            let index = self.manager.current_index.load(Ordering::SeqCst);
            let node_list = self.manager.current_list.lock().await;
            let mut node = node_list[index].clone();
            let last_index = node_list.len() - 1;

            drop(node_list);

            if index == last_index {
                is_last = true;
            }

            self.last_next_ad(&mut node).await;

            self.current_node =
                timed_source(node, &self.config, is_last, &self.manager, last_index).await;

            self.manager.current_index.fetch_add(1, Ordering::SeqCst);
        } else {
            let (_, total_delta) = get_delta(&self.config, &self.start_sec);

            if !self.config.playlist.infinit
                && self.last_json_path == self.json_playlist.path
                && total_delta.abs() > 1.0
            {
                // Playlist is to early finish,
                // and if we have to fill it with a placeholder.
                trace!("Total delta on list end: {total_delta}");

                self.fill_end(total_delta).await;

                return Some(self.current_node.clone());
            }
            // Get first clip from next playlist.

            let c_list = self.manager.current_list.lock().await;
            let mut first_node = c_list[0].clone();

            drop(c_list);

            if self.config.playlist.infinit {
                self.recalculate_begin(false).await;
            }

            self.manager.current_index.store(0, Ordering::SeqCst);
            self.last_next_ad(&mut first_node).await;
            first_node.last_ad = self.last_node_ad;

            self.current_node = gen_source(&self.config, first_node, &self.manager, 0).await;

            self.manager.current_index.store(1, Ordering::SeqCst);
        }

        Some(self.current_node.clone())
    }
}

/// Prepare input clip:
///
/// - check begin and length from clip
/// - return clip only if we are in 24 hours time range
async fn timed_source(
    node: Media,
    config: &PlayoutConfig,
    last: bool,
    manager: &ChannelManager,
    last_index: usize,
) -> Media {
    let id = config.general.channel_id;
    let time_shift = manager.channel.lock().await.time_shift;
    let current_date = manager.current_date.lock().await.clone();
    let last_date = manager.channel.lock().await.last_date.clone();
    let (delta, total_delta) = get_delta(config, &node.begin.unwrap());
    let mut shifted_delta = delta;
    let mut new_node = node.clone();
    new_node.skip = true;

    trace!(
        "Node - begin: {} | source: {}",
        node.begin.unwrap(),
        node.source
    );
    trace!(
        "timed source is last: {last} | current_date: {current_date} | last_date: {last_date:?} | time_shift: {time_shift}"
    );

    if config.playlist.length.contains(':') {
        if Some(current_date) == last_date && time_shift != 0.0 {
            shifted_delta = delta - time_shift;

            debug!(target: Target::file_mail(), channel = id; "Delta: <yellow>{shifted_delta:.3}</>, shifted: <yellow>{delta:.3}</>");
        } else {
            debug!(target: Target::file_mail(), channel = id; "Delta: <yellow>{shifted_delta:.3}</>");
        }

        if config.general.stop_threshold > 0.0
            && shifted_delta.abs() > config.general.stop_threshold
        {
            // Handle summer/winter time changes.
            // It only checks if the time change is one hour backwards or forwards.
            // If this is enough, or if a real change needs to be checked, it needs to show production usage.
            if is_close(shifted_delta.abs(), 3600.0, config.general.stop_threshold) {
                warn!(
                    "A time change seemed to have occurred, apply time shift: <yellow>{shifted_delta:.3}</> seconds."
                );

                let db_pool = manager.db_pool.clone().unwrap();
                manager.channel.lock().await.time_shift = time_shift + shifted_delta;

                if let Err(e) =
                    tokio::runtime::Runtime::new()
                        .unwrap()
                        .block_on(handles::update_stat(
                            &db_pool,
                            id,
                            None,
                            time_shift + shifted_delta,
                        ))
                {
                    error!(target: Target::file_mail(), channel = id; "Unable to write status: {e}");
                };
            } else if manager.is_alive.load(Ordering::SeqCst) {
                error!(target: Target::file_mail(), channel = id; "Clip begin out of sync for <yellow>{delta:.3}</> seconds.");

                new_node.cmd = None;

                return new_node;
            }
        }
    }

    if (total_delta > node.out - node.seek && !last)
        || node.index.unwrap() < 2
        || !config.playlist.length.contains(':')
        || config.playlist.infinit
    {
        // when we are in the 24 hour range, get the clip
        new_node.skip = false;
        new_node = gen_source(config, node, manager, last_index).await;
    } else if total_delta <= 0.0 {
        info!(target: Target::file_mail(), channel = id; "Begin is over play time, skip: {}", node.source);
    } else if total_delta < node.duration - node.seek || last {
        new_node = handle_list_end(config, node, total_delta, manager, last_index).await;
    }

    new_node
}

async fn duplicate_for_seek_and_loop(node: &mut Media, current_list: &Arc<Mutex<Vec<Media>>>) {
    let mut nodes = current_list.lock().await;
    let index = node.index.unwrap_or_default();

    let mut node_duplicate = node.clone();
    node_duplicate.seek = 0.0;
    let orig_seek = node.seek;
    node.out = node.duration;

    if node.seek > node.duration {
        node.seek %= node.duration;

        node_duplicate.out = node_duplicate.out - orig_seek - (node.out - node.seek);
    } else {
        node_duplicate.out -= node_duplicate.duration;
    }

    if node.seek == node.out {
        node.seek = node_duplicate.seek;
        node.out = node_duplicate.out;
    } else if node_duplicate.out - node_duplicate.seek > 1.2 {
        node_duplicate.begin =
            Some(node_duplicate.begin.unwrap_or_default() + (node.out - node.seek));

        nodes.insert(index + 1, node_duplicate);

        for (i, item) in nodes.iter_mut().enumerate() {
            item.index = Some(i);
        }
    }
}

/// Generate the source CMD, or when clip not exist, get a dummy.
pub async fn gen_source(
    config: &PlayoutConfig,
    mut node: Media,
    manager: &ChannelManager,
    last_index: usize,
) -> Media {
    if config.channel.s3_storage.is_some() {
        // to-do : implementation of the s3 presigned-url
        let cloned_source = node.source.clone();
        let s3_str = config.channel.s3_storage.as_ref().unwrap().clone();
        let bucket = &s3_str.bucket;
        let client = &s3_str.client;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let presigned_url = rt
            .block_on(s3_utils::s3_get_object(
                client,
                bucket,
                &cloned_source,
                S3_DEFAULT_PRESIGNEDURL_EXP as u64,
            ))
            .unwrap_or_default();
        node.source = presigned_url;
    }

    let node_index = node.index.unwrap_or_default();
    let duration = node.out - node.seek;

    if duration < 1.0 {
        warn!(
            target: Target::file_mail(), channel = config.general.channel_id;
            "Skip clip that is less then one second long (<yellow>{duration:.3}</>)."
        );

        // INFO:
        // This part has been changed twice, the last time in January 2024.
        // Better case is that it skips the short clip, especially when reloading a playlist,
        // it prevents the last clip from playing again for 1.2 seconds.
        // But the behavior needs to be observed for a longer time to be sure that it has no side effects.

        // duration = 1.2;

        // if node.seek > 1.0 {
        //     node.seek -= 1.2;
        // } else {
        //     node.out = 1.2;
        // }
        node.skip = true;
    }

    trace!("Clip new length: {duration}, duration: {}", node.duration);
    if node.probe.is_none() && !node.source.is_empty() {
        if let Err(e) = node.add_probe(true).await {
            trace!("{e:?}");
        };
    } else {
        trace!("Node has a probe...");
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
            node.cmd = Some(loop_image(config, &node));
        } else {
            if node.seek > 0.0 && node.out > node.duration {
                warn!(target: Target::file_mail(), channel = config.general.channel_id; "Clip loops and has seek value: duplicate clip to separate loop and seek.");
                duplicate_for_seek_and_loop(&mut node, &manager.current_list).await;
            }

            node.cmd = Some(seek_and_length(config, &mut node));
        }
    } else {
        trace!("clip index: {node_index} | last index: {last_index}");

        // Last index is the index from the last item from the node list.
        if node_index < last_index {
            error!(target: Target::file_mail(), channel = config.general.channel_id; "Source not found: <b><magenta>{}</></b>", node.source);
        }

        let fillers = manager.filler_list.lock().await;

        // Set list_init to true, to stay in sync.
        manager.list_init.store(true, Ordering::SeqCst);

        if config.storage.filler_path.is_dir() && !fillers.is_empty() {
            let mut index = manager.filler_index.fetch_add(1, Ordering::SeqCst);

            if index > fillers.len() - 1 {
                index = 0;
            }

            let mut filler_media = fillers[index].clone();

            trace!("take filler: {}", filler_media.source);

            if index == fillers.len() - 1 {
                // reset index for next round
                manager.filler_index.store(0, Ordering::SeqCst);
            }

            if filler_media.probe.is_none() {
                if let Err(e) = filler_media.add_probe(false).await {
                    error!(target: Target::file_mail(), channel = config.general.channel_id; "{e:?}");
                };
            }

            if filler_media.duration > duration {
                filler_media.out = duration;
            }

            node.source = filler_media.source;
            node.seek = 0.0;
            node.out = filler_media.out;
            node.duration = filler_media.duration;
            node.cmd = Some(loop_filler(config, &node));
            node.probe = filler_media.probe;
        } else {
            match MediaProbe::new(&config.storage.filler_path).await {
                Ok(probe) => {
                    if config
                        .storage
                        .filler_path
                        .to_string_lossy()
                        .to_string()
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_lowercase())
                        .filter(|c| IMAGE_FORMAT.contains(&c.as_str()))
                        .is_some()
                    {
                        node.source = config
                            .storage
                            .filler_path
                            .clone()
                            .to_string_lossy()
                            .to_string();
                        node.cmd = Some(loop_image(config, &node));
                        node.probe = Some(probe);
                    } else if let Some(filler_duration) = probe.clone().format.duration {
                        // Create placeholder from config filler.
                        let filler_out = filler_duration.min(duration);

                        node.source = config
                            .storage
                            .filler_path
                            .clone()
                            .to_string_lossy()
                            .to_string();
                        node.seek = 0.0;
                        node.out = filler_out;
                        node.duration = filler_duration;
                        node.cmd = Some(loop_filler(config, &node));
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
                    error!(target: Target::file_mail(), channel = config.general.channel_id; "Filler error: {e}");

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
            target: Target::file_mail(), channel = config.general.channel_id;
            "Generate filler with <yellow>{:.2}</> seconds length!",
            node.out
        );
    }

    node.add_filter(config, &manager.filter_chain.clone()).await;

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
async fn handle_list_init(
    config: &PlayoutConfig,
    mut node: Media,
    manager: &ChannelManager,
    last_index: usize,
) -> Media {
    debug!(target: Target::file_mail(), channel = config.general.channel_id; "Playlist init");
    let (_, total_delta) = get_delta(config, &node.begin.unwrap());

    if !config.playlist.infinit && node.out - node.seek > total_delta {
        node.out = total_delta + node.seek;
    }

    gen_source(config, node, manager, last_index).await
}

/// when we come to last clip in playlist,
/// or when we reached total playtime,
/// we end up here
async fn handle_list_end(
    config: &PlayoutConfig,
    mut node: Media,
    total_delta: f64,
    manager: &ChannelManager,
    last_index: usize,
) -> Media {
    debug!(target: Target::file_mail(), channel = config.general.channel_id; "Handle last clip from day");

    let out = if node.seek > 0.0 {
        node.seek + total_delta
    } else {
        if node.duration > total_delta {
            warn!(target: Target::file_mail(), channel = config.general.channel_id; "Adjust clip duration to: <yellow>{total_delta:.2}</>");
        }

        total_delta
    };

    if (node.duration > total_delta || node.out > total_delta)
        && (node.duration - node.seek >= total_delta || node.out - node.seek >= total_delta)
        && total_delta > 1.0
    {
        node.out = out;
    } else if total_delta > node.duration {
        warn!(target: Target::file_mail(), channel = config.general.channel_id; "Playlist is not long enough: <yellow>{total_delta:.2}</> seconds needed");
    }

    node.skip = false;

    gen_source(config, node, manager, last_index).await
}
