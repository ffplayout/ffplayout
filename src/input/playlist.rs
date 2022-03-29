use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use simplelog::*;
use tokio::runtime::Handle;

use crate::utils::{
    check_sync, gen_dummy, get_date, get_delta, get_sec, is_close, json_reader::read_json,
    modified_time, seek_and_length, GlobalConfig, Media, DUMMY_LEN,
};

#[derive(Debug)]
pub struct CurrentProgram {
    config: GlobalConfig,
    start_sec: f64,
    json_mod: Option<String>,
    json_path: Option<String>,
    json_date: String,
    nodes: Vec<Media>,
    current_node: Media,
    pub init: Arc<Mutex<bool>>,
    index: usize,
    rt_handle: Handle,
    is_terminated: Arc<Mutex<bool>>,
}

impl CurrentProgram {
    pub fn new(rt_handle: Handle, is_terminated: Arc<Mutex<bool>>) -> Self {
        let config = GlobalConfig::global();
        let json = read_json(None, rt_handle.clone(), is_terminated.clone(), true, 0.0);

        Self {
            config: config.clone(),
            start_sec: json.start_sec.unwrap(),
            json_mod: json.modified,
            json_path: json.current_file,
            json_date: json.date,
            nodes: json.program,
            current_node: Media::new(0, "".to_string()),
            init: Arc::new(Mutex::new(true)),
            index: 0,
            rt_handle,
            is_terminated,
        }
    }

    fn check_update(&mut self, seek: bool) {
        if self.json_path.is_none() {
            let json = read_json(
                None,
                self.rt_handle.clone(),
                self.is_terminated.clone(),
                seek,
                0.0,
            );

            self.json_path = json.current_file;
            self.json_mod = json.modified;
            self.nodes = json.program;
        } else if Path::new(&self.json_path.clone().unwrap()).is_file() {
            let mod_time = modified_time(&self.json_path.clone().unwrap());

            if !mod_time
                .unwrap()
                .to_string()
                .eq(&self.json_mod.clone().unwrap())
            {
                // when playlist has changed, reload it
                info!(
                    "Reload playlist <b><magenta>{}</></b>",
                    self.json_path.clone().unwrap()
                );

                let json = read_json(
                    self.json_path.clone(),
                    self.rt_handle.clone(),
                    self.is_terminated.clone(),
                    false,
                    0.0,
                );

                self.json_mod = json.modified;
                self.nodes = json.program;
            }
        } else {
            error!(
                "Playlist <b><magenta>{}</></b> not exists!",
                self.json_path.clone().unwrap()
            );
            let mut media = Media::new(0, "".to_string());
            media.begin = Some(get_sec());
            media.duration = DUMMY_LEN;
            media.out = DUMMY_LEN;

            self.json_path = None;
            self.nodes = vec![media.clone()];
            self.current_node = media;
            *self.init.lock().unwrap() = true;
            self.index = 0;
        }
    }

    fn check_for_next_playlist(&mut self) {
        let current_time = get_sec();
        let start_sec = self.config.playlist.start_sec.unwrap();
        let target_length = self.config.playlist.length_sec.unwrap();
        let (delta, total_delta) = get_delta(&current_time);
        let mut duration = self.current_node.out.clone();

        if self.current_node.duration > self.current_node.out {
            duration = self.current_node.duration.clone()
        }

        let next_start = self.current_node.begin.unwrap() - start_sec + duration + delta;
        let date = get_date(false, start_sec, next_start);

        if (next_start >= target_length
            || is_close(total_delta, 0.0, 2.0)
            || is_close(total_delta, target_length, 2.0))
            && date != self.json_date
        {
            let json = read_json(
                None,
                self.rt_handle.clone(),
                self.is_terminated.clone(),
                false,
                next_start,
            );

            self.json_path = json.current_file.clone();
            self.json_mod = json.modified;
            self.json_date = json.date;
            self.nodes = json.program;
            self.index = 0;

            if json.current_file.is_none() {
                *self.init.lock().unwrap() = true;
            }
        }
    }

    fn is_ad(&mut self, i: usize, next: bool) -> Option<bool> {
        if next {
            if i + 1 < self.nodes.len() && self.nodes[i + 1].category == "advertisement".to_string()
            {
                return Some(true);
            } else {
                return Some(false);
            }
        } else {
            if i > 0
                && i < self.nodes.len()
                && self.nodes[i - 1].category == "advertisement".to_string()
            {
                return Some(true);
            } else {
                return Some(false);
            }
        }
    }

    fn get_init_clip(&mut self) {
        let mut time_sec = get_sec();

        if time_sec < self.start_sec {
            time_sec += self.config.playlist.length_sec.unwrap()
        }

        for (i, item) in self.nodes.iter_mut().enumerate() {
            if item.begin.unwrap() + item.out - item.seek > time_sec {
                *self.init.lock().unwrap() = false;
                self.index = i + 1;

                // de-instance node to preserve original values in list
                let mut node_clone = item.clone();
                node_clone.seek = time_sec - node_clone.begin.unwrap();

                self.current_node = handle_list_init(node_clone);

                break;
            }
        }
    }
}

impl Iterator for CurrentProgram {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        if *self.init.lock().unwrap() {
            debug!("Playlist init");
            self.check_update(true);

            if self.json_path.is_some() {
                self.get_init_clip();
            }

            if *self.init.lock().unwrap() {
                // on init load playlist, could be not long enough,
                // so we check if we can take the next playlist already,
                // or we fill the gap with a dummy.
                self.current_node = self.nodes[self.nodes.len() - 1].clone();
                self.check_for_next_playlist();

                let new_node = self.nodes[self.nodes.len() - 1].clone();
                let new_length = new_node.begin.unwrap() + new_node.duration;

                if new_length
                    >= self.config.playlist.length_sec.unwrap()
                        + self.config.playlist.start_sec.unwrap()
                {
                    self.get_init_clip();
                } else {
                    let mut current_time = get_sec();
                    let (_, total_delta) = get_delta(&current_time);
                    let mut duration = DUMMY_LEN;

                    if DUMMY_LEN > total_delta {
                        duration = total_delta;
                        *self.init.lock().unwrap() = false;
                    }

                    if self.config.playlist.start_sec.unwrap() > current_time {
                        current_time += self.config.playlist.length_sec.unwrap() + 1.0;
                    }
                    let mut media = Media::new(0, "".to_string());
                    media.begin = Some(current_time);
                    media.duration = duration;
                    media.out = duration;

                    self.current_node = gen_source(media);
                    self.nodes.push(self.current_node.clone());
                    self.index = self.nodes.len();
                }
            }

            self.current_node.last_ad = self.is_ad(self.index, false);
            self.current_node.next_ad = self.is_ad(self.index, true);

            return Some(self.current_node.clone());
        }

        if self.index < self.nodes.len() {
            let mut is_last = false;

            if self.index == self.nodes.len() - 1 {
                is_last = true
            }

            self.current_node = timed_source(self.nodes[self.index].clone(), &self.config, is_last);
            self.current_node.last_ad = self.is_ad(self.index, false);
            self.current_node.next_ad = self.is_ad(self.index, true);
            self.index += 1;

            // update playlist should happen after current clip,
            // to prevent unknown behaviors.
            self.check_update(false);
            self.check_for_next_playlist();
            Some(self.current_node.clone())
        } else {
            let last_playlist = self.json_path.clone();
            self.check_for_next_playlist();
            let (_, total_delta) = get_delta(&self.config.playlist.start_sec.unwrap());
            let mut last_ad = self.is_ad(self.index, false);

            if last_playlist == self.json_path
                && total_delta.abs() > self.config.general.stop_threshold
            {
                // Test if playlist is to early finish,
                // and if we have to fill it with a placeholder.
                self.index += 1;
                self.current_node = Media::new(self.index, "".to_string());
                self.current_node.begin = Some(get_sec());
                let mut duration = total_delta.abs();

                if duration > DUMMY_LEN {
                    duration = DUMMY_LEN;
                }
                self.current_node.duration = duration;
                self.current_node.out = duration;
                self.current_node = gen_source(self.current_node.clone());
                self.nodes.push(self.current_node.clone());

                last_ad = self.is_ad(self.index, false);
                self.current_node.last_ad = last_ad;
                self.current_node.add_filter();

                return Some(self.current_node.clone());
            }

            self.current_node = gen_source(self.nodes[0].clone());
            self.current_node.last_ad = last_ad;
            self.current_node.next_ad = self.is_ad(0, true);

            self.index = 1;

            Some(self.current_node.clone())
        }
    }
}

fn timed_source(node: Media, config: &GlobalConfig, last: bool) -> Media {
    // prepare input clip
    // check begin and length from clip
    // return clip only if we are in 24 hours time range

    let (delta, total_delta) = get_delta(&node.begin.unwrap());
    let mut new_node = node.clone();
    new_node.process = Some(false);

    if config.playlist.length.contains(":") {
        debug!("Delta: <yellow>{delta:.3}</>");
        debug!("Total delta: <yellow>{total_delta:.3}</>");
        let sync = check_sync(delta);

        if !sync {
            new_node.cmd = None;

            return new_node;
        }
    }

    if (total_delta > node.out - node.seek && !last)
        || node.index.unwrap() < 2
        || !config.playlist.length.contains(":")
    {
        // when we are in the 24 hour range, get the clip
        new_node = gen_source(node);
        new_node.process = Some(true);
    } else if total_delta <= 0.0 {
        info!("Begin is over play time, skip: {}", node.source);
    } else if total_delta < node.duration - node.seek || last {
        new_node = handle_list_end(node, total_delta);
    }

    new_node
}

fn gen_source(mut node: Media) -> Media {
    if Path::new(&node.source).is_file() {
        node.add_probe();
        node.cmd = Some(seek_and_length(
            node.source.clone(),
            node.seek,
            node.out,
            node.duration,
        ));
        node.add_filter();
    } else {
        if node.source.chars().count() == 0 {
            warn!(
                "Generate filler with <yellow>{:.2}</> seconds length!",
                node.out - node.seek
            );
        } else {
            error!("File not found: {}", node.source);
        }
        let (source, cmd) = gen_dummy(node.out - node.seek);
        node.source = source;
        node.cmd = Some(cmd);
        node.add_filter();
    }

    node
}

fn handle_list_init(mut node: Media) -> Media {
    // handle init clip, but this clip can be the last one in playlist,
    // this we have to figure out and calculate the right length

    let (_, total_delta) = get_delta(&node.begin.unwrap());
    let mut out = node.out;

    if node.out - node.seek > total_delta {
        out = total_delta + node.seek;
    }

    node.out = out;

    let new_node = gen_source(node);
    new_node
}

fn handle_list_end(mut node: Media, total_delta: f64) -> Media {
    // when we come to last clip in playlist,
    // or when we reached total playtime,
    // we end up here

    debug!("Playlist end");

    let mut out = if node.seek > 0.0 {
        node.seek + total_delta
    } else {
        total_delta
    };

    // prevent looping
    if out > node.duration {
        out = node.duration
    } else {
        warn!(
            "Clip length is not in time, new duration is: <yellow>{:.2}</>",
            total_delta
        )
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
        node.cmd = Some(seek_and_length(
            node.source.clone(),
            node.seek,
            node.out,
            node.duration,
        ));

        node.process = Some(false);

        return node;
    } else {
        error!(
            "Playlist is not long enough: <yellow>{:.2}</> seconds needed",
            total_delta
        );
    }

    node.process = Some(true);
    node.cmd = Some(seek_and_length(
        node.source.clone(),
        node.seek,
        node.out,
        node.duration,
    ));

    node
}
