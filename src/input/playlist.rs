use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use simplelog::*;
use tokio::runtime::Handle;

use crate::utils::{
    check_sync, gen_dummy, get_delta, get_sec, is_close, json_reader::read_json, modified_time,
    seek_and_length, GlobalConfig, Media, PlayoutStatus, DUMMY_LEN,
};

#[derive(Debug)]
pub struct CurrentProgram {
    config: GlobalConfig,
    start_sec: f64,
    json_mod: Option<String>,
    json_path: Option<String>,
    json_date: String,
    pub nodes: Arc<Mutex<Vec<Media>>>,
    current_node: Media,
    pub init: Arc<Mutex<bool>>,
    index: Arc<Mutex<usize>>,
    rt_handle: Handle,
    is_terminated: Arc<Mutex<bool>>,
}

impl CurrentProgram {
    pub fn new(
        rt_handle: Handle,
        is_terminated: Arc<Mutex<bool>>,
        current_list: Arc<Mutex<Vec<Media>>>,
        global_index: Arc<Mutex<usize>>,
    ) -> Self {
        let config = GlobalConfig::global();
        let status = PlayoutStatus::global();
        let json = read_json(None, rt_handle.clone(), is_terminated.clone(), true, 0.0);

        *current_list.lock().unwrap() = json.program;

        if status.date != json.date {
            status.clone().write(json.date.clone(), 0.0)
        }

        Self {
            config: config.clone(),
            start_sec: json.start_sec.unwrap(),
            json_mod: json.modified,
            json_path: json.current_file,
            json_date: json.date,
            nodes: current_list,
            current_node: Media::new(0, "".to_string(), false),
            init: Arc::new(Mutex::new(true)),
            index: global_index,
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
            *self.nodes.lock().unwrap() = json.program;
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
                *self.nodes.lock().unwrap() = json.program;

                self.get_current_clip();
                *self.index.lock().unwrap() += 1;
            }
        } else {
            error!(
                "Playlist <b><magenta>{}</></b> not exists!",
                self.json_path.clone().unwrap()
            );
            let mut media = Media::new(0, "".to_string(), false);
            media.begin = Some(get_sec());
            media.duration = DUMMY_LEN;
            media.out = DUMMY_LEN;

            self.json_path = None;
            *self.nodes.lock().unwrap() = vec![media.clone()];
            self.current_node = media;
            *self.init.lock().unwrap() = true;
            *self.index.lock().unwrap() = 0;
        }
    }

    fn check_for_next_playlist(&mut self) {
        let current_time = get_sec();
        let start_sec = self.config.playlist.start_sec.unwrap();
        let target_length = self.config.playlist.length_sec.unwrap();
        let (delta, total_delta) = get_delta(&current_time, &self.json_date, true);
        let mut duration = self.current_node.out.clone();

        if self.current_node.duration > self.current_node.out {
            duration = self.current_node.duration.clone()
        }

        let next_start = self.current_node.begin.unwrap() - start_sec + duration + delta;

        if next_start >= target_length
            || is_close(total_delta, 0.0, 2.0)
            || is_close(total_delta, target_length, 2.0)
        {
            let json = read_json(
                None,
                self.rt_handle.clone(),
                self.is_terminated.clone(),
                false,
                next_start,
            );

            let status = PlayoutStatus::global();
            status.clone().write(json.date.clone(), 0.0);

            self.json_path = json.current_file.clone();
            self.json_mod = json.modified;
            self.json_date = json.date;
            *self.nodes.lock().unwrap() = json.program;
            *self.index.lock().unwrap() = 0;

            if json.current_file.is_none() {
                *self.init.lock().unwrap() = true;
            }
        }
    }

    fn last_next_ad(&mut self) {
        let index = *self.index.lock().unwrap();
        let current_list = self.nodes.lock().unwrap();

        if index + 1 < current_list.len()
            && current_list[index + 1].category == "advertisement".to_string()
        {
            self.current_node.next_ad = Some(true);
        }

        if index > 0
            && index < current_list.len()
            && current_list[index - 1].category == "advertisement".to_string()
        {
            self.current_node.last_ad = Some(true);
        }
    }

    fn get_current_time(&mut self) -> f64 {
        let mut time_sec = get_sec();

        if time_sec < self.start_sec {
            time_sec += self.config.playlist.length_sec.unwrap()
        }

        time_sec
    }

    fn get_current_clip(&mut self) {
        let time_sec = self.get_current_time();

        for (i, item) in self.nodes.lock().unwrap().iter_mut().enumerate() {
            if item.begin.unwrap() + item.out - item.seek > time_sec {
                *self.init.lock().unwrap() = false;
                *self.index.lock().unwrap() = i;

                break;
            }
        }
    }

    fn init_clip(&mut self) {
        self.get_current_clip();

        if !*self.init.lock().unwrap() {
            let time_sec = self.get_current_time();
            let index = *self.index.lock().unwrap();

            // de-instance node to preserve original values in list
            let mut node_clone = self.nodes.lock().unwrap()[index].clone();
            *self.index.lock().unwrap() += 1;

            node_clone.seek = time_sec - node_clone.begin.unwrap();
            self.current_node = handle_list_init(node_clone, &self.json_date);
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
                self.init_clip();
            }

            if *self.init.lock().unwrap() {
                // on init load playlist, could be not long enough,
                // so we check if we can take the next playlist already,
                // or we fill the gap with a dummy.
                let list_length = self.nodes.lock().unwrap().len();
                self.current_node = self.nodes.lock().unwrap()[list_length - 1].clone();
                self.check_for_next_playlist();

                let new_node = self.nodes.lock().unwrap()[list_length - 1].clone();
                let new_length = new_node.begin.unwrap() + new_node.duration;

                if new_length
                    >= self.config.playlist.length_sec.unwrap()
                        + self.config.playlist.start_sec.unwrap()
                {
                    self.init_clip();
                } else {
                    let mut current_time = get_sec();
                    let (_, total_delta) = get_delta(&current_time, &self.json_date, true);
                    let mut duration = DUMMY_LEN;

                    if DUMMY_LEN > total_delta {
                        duration = total_delta;
                        *self.init.lock().unwrap() = false;
                    }

                    if self.config.playlist.start_sec.unwrap() > current_time {
                        current_time += self.config.playlist.length_sec.unwrap() + 1.0;
                    }
                    let mut media = Media::new(0, "".to_string(), false);
                    media.begin = Some(current_time);
                    media.duration = duration;
                    media.out = duration;

                    self.current_node = gen_source(media, &self.json_date);
                    self.nodes.lock().unwrap().push(self.current_node.clone());
                    *self.index.lock().unwrap() = self.nodes.lock().unwrap().len();
                }
            }

            self.last_next_ad();

            return Some(self.current_node.clone());
        }

        if *self.index.lock().unwrap() < self.nodes.lock().unwrap().len() {
            self.check_for_next_playlist();
            let mut is_last = false;
            let index = *self.index.lock().unwrap();

            if index == self.nodes.lock().unwrap().len() - 1 {
                is_last = true
            }

            self.current_node = timed_source(
                self.nodes.lock().unwrap()[index].clone(),
                &self.config,
                is_last,
                &self.json_date,
            );
            self.last_next_ad();
            *self.index.lock().unwrap() += 1;

            // update playlist should happen after current clip,
            // to prevent unknown behaviors.
            self.check_update(false);
            Some(self.current_node.clone())
        } else {
            let last_playlist = self.json_path.clone();
            let last_ad = self.current_node.last_ad.clone();
            self.check_for_next_playlist();
            let (_, total_delta) = get_delta(
                &self.config.playlist.start_sec.unwrap(),
                &self.json_date,
                true,
            );

            if last_playlist == self.json_path
                && total_delta.abs() > self.config.general.stop_threshold
            {
                // Test if playlist is to early finish,
                // and if we have to fill it with a placeholder.
                let index = *self.index.lock().unwrap();
                self.current_node = Media::new(index, "".to_string(), false);
                self.current_node.begin = Some(get_sec());
                let mut duration = total_delta.abs();

                if duration > DUMMY_LEN {
                    duration = DUMMY_LEN;
                }
                self.current_node.duration = duration;
                self.current_node.out = duration;
                self.current_node = gen_source(self.current_node.clone(), &self.json_date);
                self.nodes.lock().unwrap().push(self.current_node.clone());
                self.last_next_ad();

                self.current_node.last_ad = last_ad;
                self.current_node.add_filter(&self.json_date);

                *self.index.lock().unwrap() += 1;

                return Some(self.current_node.clone());
            }

            *self.index.lock().unwrap() = 0;
            self.current_node = gen_source(self.nodes.lock().unwrap()[0].clone(), &self.json_date);
            self.last_next_ad();
            self.current_node.last_ad = last_ad;

            *self.index.lock().unwrap() = 1;

            Some(self.current_node.clone())
        }
    }
}

fn timed_source(node: Media, config: &GlobalConfig, last: bool, json_date: &String) -> Media {
    // prepare input clip
    // check begin and length from clip
    // return clip only if we are in 24 hours time range

    let (delta, total_delta) = get_delta(&node.begin.unwrap(), &json_date, true);
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
        new_node = gen_source(node, &json_date);
        new_node.process = Some(true);
    } else if total_delta <= 0.0 {
        info!("Begin is over play time, skip: {}", node.source);
    } else if total_delta < node.duration - node.seek || last {
        new_node = handle_list_end(node, total_delta);
    }

    new_node
}

fn gen_source(mut node: Media, json_date: &String) -> Media {
    if Path::new(&node.source).is_file() {
        node.add_probe();
        node.cmd = Some(seek_and_length(
            node.source.clone(),
            node.seek,
            node.out,
            node.duration,
        ));
        node.add_filter(&json_date);
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
        node.add_filter(&json_date);
    }

    node
}

fn handle_list_init(mut node: Media, json_date: &String) -> Media {
    // handle init clip, but this clip can be the last one in playlist,
    // this we have to figure out and calculate the right length

    let (_, total_delta) = get_delta(&node.begin.unwrap(), &json_date, true);
    let mut out = node.out;

    if node.out - node.seek > total_delta {
        out = total_delta + node.seek;
    }

    node.out = out;

    let new_node = gen_source(node, &json_date);
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
