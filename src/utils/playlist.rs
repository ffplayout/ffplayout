use std::path::Path;

use simplelog::*;

use crate::utils::{
    check_sync, gen_dummy, get_delta, get_sec, json_reader::read_json, modified_time,
    seek_and_length, Config, Media, DUMMY_LEN,
};

#[derive(Debug)]
pub struct CurrentProgram {
    config: Config,
    start_sec: f64,
    json_mod: Option<String>,
    json_path: Option<String>,
    nodes: Vec<Media>,
    current_node: Media,
    init: bool,
    index: usize,
}

impl CurrentProgram {
    pub fn new(config: Config) -> Self {
        let json = read_json(&config, true, 0.0);

        Self {
            config,
            start_sec: json.start_sec.unwrap(),
            json_mod: json.modified,
            json_path: json.current_file,
            nodes: json.program,
            current_node: Media::new(0, "".to_string()),
            init: true,
            index: 0,
        }
    }

    fn check_update(&mut self) {
        match self.json_path.clone() {
            Some(path) => {
                if Path::new(&path).is_file() {
                    let mod_time = modified_time(path);

                    if !mod_time
                        .unwrap()
                        .to_string()
                        .eq(&self.json_mod.clone().unwrap())
                    {
                        // when playlist has changed, reload it
                        let json = read_json(&self.config, false, 0.0);

                        self.json_mod = json.modified;
                        self.nodes = json.program;
                    }
                } else {
                    error!("Playlist <b><magenta>{}</></b> not exists!", path);
                    let mut media = Media::new(0, "".to_string());
                    media.begin = Some(get_sec());
                    media.duration = DUMMY_LEN;
                    media.out = DUMMY_LEN;

                    self.json_path = None;
                    self.nodes = vec![media.clone()];
                    self.current_node = media;
                    self.init = true;
                    self.index = 0;
                }
            }

            _ => (),
        }
    }

    fn check_for_next_playlist(&mut self, last: bool) {
        let mut out = self.current_node.out.clone();
        let start_sec = &self.config.playlist.start_sec.unwrap();
        let mut delta = 0.0;

        if self.current_node.duration > self.current_node.out {
            out = self.current_node.duration.clone()
        }

        if last {
            let seek = if self.current_node.seek > 0.0 {
                self.current_node.seek
            } else {
                0.0
            };
            (delta, _) = get_delta(&self.current_node.begin.unwrap().clone(), &self.config);
            delta += seek + self.config.general.stop_threshold;
        }

        let next_start = self.current_node.begin.unwrap() - start_sec + out + delta;

        if next_start >= self.config.playlist.length_sec.unwrap() {
            println!("----read");
            let json = read_json(&self.config, false, next_start);

            self.json_mod = json.modified;
            self.nodes = json.program;
            self.index = 0;
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

        let mut start_sec = self.start_sec.clone();

        for (i, item) in self.nodes.iter_mut().enumerate() {
            if start_sec + item.out - item.seek > time_sec {
                self.init = false;
                self.index = i + 1;
                item.seek = time_sec - start_sec;

                self.current_node = handle_list_init(item.clone(), &self.config);
                break;
            }
            start_sec += item.out - item.seek;
        }
    }

    fn get_new_playlist(&mut self) {
        // Here we should end up, when we need a new playlist
        let (delta, _) = get_delta(&self.config.playlist.start_sec.unwrap(), &self.config);
        let next_begin =
                self.current_node.begin.unwrap() + self.current_node.out - self.current_node.seek + delta;

        let json = read_json(&self.config, false, next_begin);

        if json.current_file.is_none() {
            self.init = true;
            self.json_path = None;
            self.nodes = json.program;
        } else {
            self.json_mod = json.modified;
            self.json_path = json.current_file;
            self.nodes = json.program;
        }
    }
}

impl Iterator for CurrentProgram {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        if self.init {
            debug!("Playlist init");

            if self.json_path.is_some() {
                self.get_init_clip();
            }

            if self.init {
                let json = read_json(&self.config, false, get_sec() + DUMMY_LEN);

                if json.current_file.is_some() {
                    self.json_mod = json.modified;
                    self.json_path = json.current_file;
                    self.nodes = json.program;
                    self.get_init_clip();
                } else {
                    let mut media = Media::new(0, "".to_string());
                    media.begin = Some(get_sec());
                    media.duration = DUMMY_LEN;
                    media.out = DUMMY_LEN;

                    self.current_node = gen_source(media, &self.config);
                }
            }

            self.current_node.last_ad = self.is_ad(self.index, false);
            self.current_node.next_ad = self.is_ad(self.index, true);

            return Some(self.current_node.clone());
        }
        if self.index < self.nodes.len() {
            self.check_update();
            self.check_for_next_playlist(false);
            let mut is_last = false;

            if self.index == self.nodes.len() - 1 {
                is_last = true
            }

            self.current_node = timed_source(self.nodes[self.index].clone(), &self.config, is_last);
            self.current_node.last_ad = self.is_ad(self.index, false);
            self.current_node.next_ad = self.is_ad(self.index, true);
            self.index += 1;

            // if self.current_node.process.unwrap() {
            //     self.index += 1;
            // } else {
            //     self.index = 0;
            //     println!("go to next");
            //     self.current_node.begin = self.config.playlist.start_sec;
            //     // self.get_new_playlist();
            // }

            Some(self.current_node.clone())
        } else {
            debug!("Playlist reached end");
            let (delta, time_diff) = get_delta(&self.config.playlist.start_sec.unwrap(), &self.config);
            let mut last_ad = self.is_ad(self.index, false);

            println!("delta: {:?} | time_diff: {:?}", delta, time_diff);

            if time_diff.abs() > self.config.general.stop_threshold {
                // Test if playlist is to early finish,
                // and if we have to fill it with a placeholder.
                self.current_node = Media::new(self.index + 1, "".to_string());
                self.current_node.begin = Some(get_sec());
                let mut duration = time_diff.abs();

                if duration > DUMMY_LEN {
                    duration = DUMMY_LEN;
                }
                self.current_node.duration = duration;
                self.current_node.out = duration;
                self.current_node = gen_source(self.current_node.clone(), &self.config);
                self.nodes.push(self.current_node.clone());

                last_ad = self.is_ad(self.index, false);
                self.current_node.last_ad = last_ad;
                self.current_node.add_filter(&self.config);

                return Some(self.current_node.clone());
            }

            self.get_new_playlist();

            self.current_node = gen_source(self.nodes[0].clone(), &self.config);
            self.current_node.last_ad = last_ad;
            self.current_node.next_ad = self.is_ad(0, true);

            self.index = 1;

            Some(self.current_node.clone())
        }
    }
}

fn timed_source(node: Media, config: &Config, last: bool) -> Media {
    // prepare input clip
    // check begin and length from clip
    // return clip only if we are in 24 hours time range

    let (delta, total_delta) = get_delta(&node.begin.unwrap(), &config);
    let mut new_node = node.clone();
    new_node.process = Some(false);

    if config.playlist.length.contains(":") {
        debug!("Delta: <yellow>{delta}</>");
        debug!("Total delta: <yellow>{total_delta}</>");
        let sync = check_sync(delta, &config);

        if !sync {
            new_node.cmd = None;

            return new_node;
        }
    }

    if (total_delta > node.out - node.seek && !last) || !config.playlist.length.contains(":") {
        // when we are in the 24 hour range, get the clip
        new_node = gen_source(node, &config);
        new_node.process = Some(true);
    } else if total_delta <= 0.0 {
        info!("Begin is over play time, skip: {}", node.source);
    } else if total_delta < node.duration - node.seek || last {
        println!("handle list end");
        new_node = handle_list_end(node, total_delta);
    }

    new_node
}

fn gen_source(mut node: Media, config: &Config) -> Media {
    if Path::new(&node.source).is_file() {
        node.add_probe();
        node.cmd = Some(seek_and_length(
            node.source.clone(),
            node.seek,
            node.out,
            node.duration,
        ));
        node.add_filter(&config);
    } else {
        println!("{:?}", node);
        if node.source.chars().count() == 0 {
            warn!(
                "Generate filler with <yellow>{:.2}</> seconds length!",
                node.out - node.seek
            );
        } else {
            error!("File not found: {}", node.source);
        }
        let (source, cmd) = gen_dummy(node.out - node.seek, &config);
        node.source = source;
        node.cmd = Some(cmd);
        node.add_filter(&config);
    }

    node
}

fn handle_list_init(mut node: Media, config: &Config) -> Media {
    // handle init clip, but this clip can be the last one in playlist,
    // this we have to figure out and calculate the right length

    let (_, total_delta) = get_delta(&node.begin.unwrap(), config);
    let mut out = node.out;

    if node.out - node.seek > total_delta {
        out = total_delta + node.seek;
    }

    node.out = out;

    let new_node = gen_source(node, &config);
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
        warn!("Last clip less then 1 second long, skip: {}", node.source);
        node.out = out;
        node.cmd = Some(seek_and_length(
            node.source.clone(),
            node.seek,
            node.out,
            node.duration,
        ));

        // node.process = Some(false);

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
