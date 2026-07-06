use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use log::*;

use crate::{
    db::handles,
    player::{
        controller::ChannelManager,
        utils::{
            JsonPlaylist, Media, get_date, get_delta, is_close, is_remote,
            json_serializer::{read_json, set_defaults},
            modified_time, probe_media, time_in_seconds,
        },
    },
    utils::config::PlayoutConfig,
};

const NEXT_START_THRESHOLD: f64 = 1.5;
const IS_CLOSE_THRESHOLD: f64 = 2.0;

fn placeholder_duration(requested: f64, natural: f64) -> f64 {
    let requested = requested
        .is_finite()
        .then_some(requested)
        .filter(|d| *d > 0.0);
    let natural = natural.is_finite().then_some(natural).filter(|d| *d > 0.0);

    match (requested, natural) {
        (Some(requested), Some(natural)) => requested.min(natural),
        (Some(requested), None) => requested,
        (None, Some(natural)) => natural,
        (None, None) => 0.0,
    }
}

/// Struct for current playlist.
///
/// Here we prepare the init clip and build a iterator where we pull our clips.
#[derive(Debug)]
pub struct CurrentProgram {
    channel_id: i32,
    config: PlayoutConfig,
    manager: ChannelManager,
    date: String,
    start_sec: f64,
    length_sec: f64,
    json_playlist: JsonPlaylist,
    current_node: Media,
    is_alive: Arc<AtomicBool>,
    last_json_path: Option<String>,
    last_node_ad: bool,
}

/// Prepare a playlist iterator.
impl CurrentProgram {
    pub async fn new(manager: ChannelManager) -> Self {
        let config = manager.config.read().await.clone();
        let is_alive = manager.is_alive.clone();
        let date = get_date(
            true,
            config.playlist.start_sec.unwrap(),
            true,
            &config.channel.timezone,
        );

        Self {
            channel_id: config.general.channel_id,
            config: config.clone(),
            manager,
            date: date.clone(),
            start_sec: config.playlist.start_sec.unwrap(),
            length_sec: config.playlist.length_sec.unwrap(),
            json_playlist: JsonPlaylist::new(date, config.playlist.start_sec.unwrap()),
            current_node: Media::default(),
            is_alive,
            last_json_path: None,
            last_node_ad: false,
        }
    }

    // Check if there is no current playlist or file got updated,
    // and when is so load/reload it.
    async fn load_or_update_playlist(&mut self) {
        let mut get_current = false;
        let mut reload = false;

        if let Some(path) = self.json_playlist.path.clone() {
            if (Path::new(&path).is_file() || is_remote(&path))
                && self.json_playlist.modified != modified_time(&path).await
            {
                info!(channel = self.channel_id; "Reload playlist <span class=\"log-addr\">{path}</span>");
                self.manager.list_init.store(true, Ordering::SeqCst);
                get_current = true;
                reload = true;
            }
        } else {
            get_current = true;
        }

        if get_current {
            self.json_playlist = read_json(
                &self.manager,
                &mut self.config,
                self.manager.current_list.clone(),
                self.json_playlist.path.clone(),
                self.is_alive.clone(),
                self.date.clone(),
            )
            .await;

            if !reload {
                if let Some(file) = &self.json_playlist.path {
                    info!(channel = self.channel_id; "Read playlist: <span class=\"log-addr\">{file}</span>");
                }

                let new_date = self.json_playlist.date.clone();

                let last_date_mismatch = {
                    let channel = self.manager.channel.lock().await;
                    channel.last_date.as_ref() != Some(&new_date)
                };

                if last_date_mismatch {
                    self.set_status(&Some(new_date.clone()), 0.0).await;
                }

                self.manager.current_date.lock().await.clone_from(&new_date);
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

            next_start += NEXT_START_THRESHOLD;
        }

        next_start += duration;

        trace!(
            "delta: {delta} | total_delta: {total_delta}, index: {node_index}, last index: {last_index}, init: {} \n        next_start: {next_start} | length_sec: {} | source {}",
            self.manager.list_init.load(Ordering::SeqCst),
            self.length_sec,
            self.current_node.source
        );

        // Check if we over the target length or we are close to it, if so we load the next playlist.
        if !self.config.playlist.infinit
            && (next_start >= self.length_sec
                || is_close(total_delta, 0.0, IS_CLOSE_THRESHOLD)
                || is_close(total_delta, self.length_sec, IS_CLOSE_THRESHOLD)
                || self.date != self.json_playlist.date)
        {
            trace!("get next day");
            next = true;
            self.date = get_date(seek, self.start_sec, true, &self.config.channel.timezone);

            self.json_playlist = read_json(
                &self.manager,
                &mut self.config,
                self.manager.current_list.clone(),
                None,
                self.is_alive.clone(),
                self.date.clone(),
            )
            .await;

            if let Some(file) = &self.json_playlist.path {
                info!(channel = self.channel_id; "Read next playlist: <span class=\"log-addr\">{file}</span>");
            }

            self.manager.list_init.store(false, Ordering::SeqCst);
            self.set_status(&Some(self.json_playlist.date.clone()), 0.0)
                .await;

            self.manager
                .current_list
                .lock()
                .await
                .clone_from(&self.json_playlist.program);
            self.manager.current_index.store(0, Ordering::SeqCst);
        } else {
            if is_close(next_start, self.length_sec, IS_CLOSE_THRESHOLD) {
                self.date = get_date(seek, self.start_sec, true, &self.config.channel.timezone);
            }

            self.load_or_update_playlist().await;
        }

        next
    }

    async fn set_status(&mut self, date: &Option<String>, mut shift: f64) {
        {
            let mut channel = self.manager.channel.lock().await;

            if channel.last_date != *date && channel.time_shift != 0.0 {
                shift = 0.0;
                info!(channel = self.channel_id; "Reset playout status");
            }

            if let Some(d) = date {
                self.manager.current_date.lock().await.clone_from(d);
                channel.last_date.clone_from(date);
            }

            channel.time_shift = shift;
        }

        if let Err(e) =
            handles::update_stat(&self.manager.db_pool, self.channel_id, date, shift).await
        {
            error!(channel = self.channel_id; "Unable to write status: {e}");
        };
    }

    // Check if last and/or next clip is a advertisement.
    async fn last_next_ad(&mut self, node: &mut Media) {
        let index = self.manager.current_index.load(Ordering::SeqCst);
        let list = self.manager.current_list.lock().await;
        let length = list.len();

        if index + 1 < length && &list[index + 1].category == "advertisement" {
            node.next_ad = true;
        }

        if index > 0 && index < length && &list[index - 1].category == "advertisement" {
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

        let time_shift = self.manager.channel.lock().await.time_shift;

        if time_shift != 0.0 {
            info!(

                channel = self.channel_id;
                "Shift playlist start for <span class=\"log-number\">{time_shift:.3}</span> seconds"
            );
            time_sec += time_shift;
        }

        let playlist_len_opt = self.json_playlist.length;

        if self.config.playlist.infinit
            && let Some(playlist_len) = playlist_len_opt
            && playlist_len < 86400.0
            && time_sec > playlist_len + self.start_sec
        {
            self.recalculate_begin(true).await;
        }

        let playlist = self.manager.current_list.lock().await;

        for (i, item) in playlist.iter().enumerate() {
            let start = item
                .begin
                .unwrap_or(self.config.playlist.start_sec.unwrap_or_default());

            let end = start + item.out - item.seek;

            if end > time_sec {
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

            self.handle_list_init(node_clone, last_index).await;

            if self
                .current_node
                .source
                .contains(&self.config.channel.storage.to_string_lossy().to_string())
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
        self.gen_source(media, 0).await;

        self.manager
            .current_list
            .lock()
            .await
            .push(self.current_node.clone());

        self.current_node.last_ad = self.last_node_ad;

        self.manager.current_index.fetch_add(1, Ordering::SeqCst);
    }

    async fn recalculate_begin(&mut self, extend: bool) {
        debug!(channel = self.channel_id; "Infinit playlist reaches end, recalculate clip begins. Extend: <span class=\"log-number\">{extend}</span>");

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
        set_defaults(&self.config, &mut self.json_playlist);
        self.manager
            .current_list
            .lock()
            .await
            .clone_from(&self.json_playlist.program);
    }

    /// Handle init clip, but this clip can be the last one in playlist,
    /// this we have to figure out and calculate the right length.
    async fn handle_list_init(&mut self, mut node: Media, last_index: usize) {
        debug!(channel = self.channel_id; "Playlist init");
        let (_, total_delta) = get_delta(&self.config, &node.begin.unwrap());

        if !self.config.playlist.infinit && node.out - node.seek > total_delta {
            node.out = total_delta + node.seek;
        }

        self.gen_source(node, last_index).await;
    }

    /// when we come to last clip in playlist,
    /// or when we reached total playtime,
    /// we end up here
    async fn handle_list_end(&mut self, mut node: Media, total_delta: f64, last_index: usize) {
        debug!(channel = self.channel_id; "Handle last clip from day");

        let out = if node.seek > 0.0 {
            node.seek + total_delta
        } else {
            if node.duration > total_delta {
                info!(channel = self.channel_id; "Adjust clip duration to: <span class=\"log-number\">{total_delta:.2}</span>");
            }

            total_delta
        };

        if (node.duration > total_delta || node.out > total_delta)
            && (node.duration - node.seek >= total_delta || node.out - node.seek >= total_delta)
            && total_delta > 1.0
        {
            node.out = out;
        } else if total_delta > node.out - node.seek {
            warn!(channel = self.channel_id; "Playlist is not long enough: <span class=\"log-number\">{:.2}</span> seconds needed", total_delta - (node.out - node.seek));
        }

        node.skip = false;

        self.gen_source(node, last_index).await;
    }

    /// Prepare input clip:
    ///
    /// - check begin and length from clip
    /// - return clip only if we are in 24 hours time range
    async fn timed_source(&mut self, mut node: Media, last: bool, last_index: usize) {
        let time_shift = self.manager.channel.lock().await.time_shift;
        let current_date = self.manager.current_date.lock().await.clone();
        let last_date = self.manager.channel.lock().await.last_date.clone();
        let (delta, total_delta) = get_delta(&self.config, &node.begin.unwrap());
        let mut shifted_delta = delta;
        let mut shifted_msg = String::new();

        trace!(
            "Node - begin: {} | source: {}",
            node.begin.unwrap(),
            node.source
        );
        trace!(
            "timed source is last: {last} | current_date: {current_date} | last_date: {last_date:?} | time_shift: {time_shift}"
        );

        if self.config.playlist.length.contains(':') {
            if Some(current_date.clone()) == last_date && time_shift != 0.0 {
                shifted_delta = delta - time_shift;
                shifted_msg = format!("shifted: <span class=\"log-number\">{delta:.3}</span>");
            }

            debug!(channel = self.channel_id; "Delta: <span class=\"log-number\">{shifted_delta:.3}</span> {shifted_msg}");

            if self.config.general.stop_threshold > 0.0
                && shifted_delta.abs() > self.config.general.stop_threshold
            {
                // Handle summer/winter time changes.
                // It only checks if the time change is one hour backwards or forwards.
                // Production usage must be shown if this is sufficient, or if a real change needs to be verified.
                if is_close(
                    shifted_delta.abs(),
                    3600.0,
                    self.config.general.stop_threshold,
                ) {
                    warn!(
                        channel = self.channel_id;
                        "A time change seemed to have occurred, apply time shift: <span class=\"log-number\">{shifted_delta:.3}</span> seconds."
                    );

                    self.set_status(&None, time_shift + shifted_delta).await;
                } else if self.manager.is_alive.load(Ordering::SeqCst) {
                    error!(channel = self.channel_id; "Clip begin out of sync for <span class=\"log-number\">{delta:.3}</span> seconds.");

                    self.set_status(&Some(current_date), 0.0).await;
                    self.manager.list_init.store(true, Ordering::SeqCst);
                    self.manager.current_index.store(0, Ordering::SeqCst);
                    node.skip = true;
                    self.current_node = node;
                    return;
                }
            }
        }

        if (total_delta > node.out - node.seek && !last)
            || node.index.unwrap() < 2
            || !self.config.playlist.length.contains(':')
            || self.config.playlist.infinit
        {
            // when we are in the 24 hour range, get the clip
            self.gen_source(node, last_index).await;

            return;
        } else if total_delta <= 0.0 {
            info!(channel = self.channel_id; "Begin is over play time, skip: {}", node.source);
        } else if total_delta < node.duration - node.seek || last {
            self.handle_list_end(node, total_delta, last_index).await;

            return;
        }

        node.skip = true;
        self.current_node = node;
    }

    /// Generate the source CMD, or when clip not exist, get a dummy.
    pub async fn gen_source(&mut self, mut node: Media, last_index: usize) {
        let node_index = node.index.unwrap_or_default();
        let duration = node.out - node.seek;

        if node.duration > 0.0 && duration < 1.0 {
            warn!(
                channel = self.channel_id;
                "Skip clip that is less then one second long (<span class=\"log-number\">{duration:.3}</span>)."
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

        trace!("Clip length: {duration}, duration: {}", node.duration);

        if node.probe.is_none()
            && !node.source.is_empty()
            && let Err(e) = node.add_probe(true).await
        {
            trace!("{e:?}");
        };

        // separate if condition, because of node.add_probe() in last condition
        if node.probe.is_none() {
            trace!("clip index: {node_index} | last index: {last_index}");

            if node_index < last_index {
                error!(
                    channel = self.channel_id;
                    "Source not found: <span class=\"log-addr\">{}</span>", node.source
                );
            }

            self.manager.list_init.store(true, Ordering::SeqCst);

            let filler = {
                let fillers = self.manager.filler_list.lock().await;

                if self.config.storage.filler_path.is_dir() && !fillers.is_empty() {
                    let index = self
                        .manager
                        .filler_index
                        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |i| {
                            Some(if i + 1 >= fillers.len() { 0 } else { i + 1 })
                        })
                        .unwrap_or(0);
                    trace!("take filler: {}", fillers[index].source);
                    Some(fillers[index].clone())
                } else {
                    None
                }
            };

            if let Some(mut filler_media) = filler {
                if filler_media.probe.is_none()
                    && let Err(e) = filler_media.add_probe(false).await
                {
                    error!(channel = self.channel_id; "{e:?}");
                };

                node.source = filler_media.source;
                node.seek = 0.0;
                node.out = placeholder_duration(duration, filler_media.duration);
                node.duration = filler_media.duration;
                node.probe = filler_media.probe;
                node.is_placeholder = true;
            } else {
                match probe_media(&self.config.storage.filler_path).await {
                    Ok(probe) => {
                        if let Some(filler_duration) = probe.clone().format.duration {
                            // Create placeholder from config filler.
                            node.source = self
                                .config
                                .storage
                                .filler_path
                                .clone()
                                .to_string_lossy()
                                .to_string();
                            node.seek = 0.0;
                            node.out = placeholder_duration(duration, filler_duration);
                            node.duration = filler_duration;
                            node.probe = Some(probe);
                            node.is_placeholder = true;
                        } else {
                            node.source = self
                                .config
                                .storage
                                .filler_path
                                .clone()
                                .to_string_lossy()
                                .to_string();
                            node.seek = 0.0;
                            node.out = duration;
                            node.duration = duration;
                            node.probe = Some(probe);
                            node.is_placeholder = true;
                        }
                    }
                    Err(e) => {
                        error!(channel = self.channel_id; "Filler error: {e}");

                        let mut dummy_duration = 60.0;

                        if node.duration > 0.0 && dummy_duration > duration {
                            dummy_duration = duration;
                        }

                        node.seek = 0.0;
                        node.out = dummy_duration;
                        node.duration = dummy_duration;
                    }
                }
            }

            warn!(
                channel = self.channel_id;
                "Generate filler with <span class=\"log-number\">{:.2}</span> seconds length!",
                node.out
            );
        }

        trace!(
            "return gen_source: {}, seek: {}, out: {}",
            node.source, node.seek, node.out,
        );

        self.current_node = node;
    }
}

/// Build the playlist iterator
impl CurrentProgram {
    pub async fn handle_playlist_init(&mut self) {
        trace!("Init playlist, from next iterator");

        let init_clip_is_filler = match self.json_playlist.path {
            Some(_) => self.init_clip().await,
            None => false,
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

            let length = self.manager.current_list.lock().await.len();
            let last_index = length.saturating_sub(1);

            let mut media = Media::new(length, "", false).await;
            media.begin = Some(current_time);
            media.duration = total_delta;
            media.out = total_delta;

            self.last_next_ad(&mut media).await;
            self.gen_source(media, last_index).await;
        }
    }

    pub async fn next(&mut self) -> Option<Media> {
        self.last_json_path.clone_from(&self.json_playlist.path);
        self.last_node_ad = self.current_node.last_ad;

        let list_init = self.manager.list_init.load(Ordering::SeqCst);
        self.check_for_playlist(list_init).await;

        if list_init {
            self.handle_playlist_init().await;
            return Some(self.current_node.clone());
        }

        let index = self.manager.current_index.load(Ordering::SeqCst);

        let current_list = self.manager.current_list.lock().await;
        let length = current_list.len();

        if index < length {
            let mut node = current_list[index].clone();
            let is_last = index == length - 1;
            drop(current_list);

            self.last_next_ad(&mut node).await;
            self.timed_source(node, is_last, length - 1).await;

            self.manager.current_index.fetch_add(1, Ordering::SeqCst);
            return Some(self.current_node.clone());
        }

        drop(current_list);

        let (_, total_delta) = get_delta(&self.config, &self.start_sec);

        if !self.config.playlist.infinit
            && self.last_json_path == self.json_playlist.path
            && total_delta.abs() > 1.0
        {
            // Playlist finished too early — fill with placeholder
            trace!("Total delta on list end: {total_delta}");
            self.fill_end(total_delta).await;
            return Some(self.current_node.clone());
        }

        // Get first clip from next playlist
        let mut first_node = {
            let list = self.manager.current_list.lock().await;
            list.first()?.clone()
        };

        if self.config.playlist.infinit {
            self.recalculate_begin(false).await;
        }

        self.manager.current_index.store(0, Ordering::SeqCst);
        self.last_next_ad(&mut first_node).await;
        first_node.last_ad = self.last_node_ad;

        self.gen_source(first_node, 0).await;
        self.manager.current_index.store(1, Ordering::SeqCst);

        Some(self.current_node.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::placeholder_duration;

    #[test]
    fn placeholder_never_exceeds_its_natural_duration() {
        assert_eq!(placeholder_duration(30.0, 12.0), 12.0);
    }

    #[test]
    fn placeholder_can_be_trimmed_to_a_shorter_missing_slot() {
        assert_eq!(placeholder_duration(5.0, 12.0), 5.0);
    }

    #[test]
    fn missing_playlist_uses_one_natural_placeholder_duration() {
        assert_eq!(placeholder_duration(86_400.0, 12.0), 12.0);
    }
}
