use std::path::Path;

use simplelog::*;

use crate::utils::{
    check_sync, gen_dummy, get_delta, get_sec, json_reader::read_json, modified_time, time_to_sec,
    Config, Media,
};

#[derive(Debug)]
pub struct CurrentProgram {
    config: Config,
    json_mod: String,
    json_path: String,
    nodes: Vec<Media>,
    current_node: Media,
    init: bool,
    idx: usize,
}

impl CurrentProgram {
    pub fn new(config: Config) -> Self {
        let json = read_json(&config, true);

        Self {
            config: config,
            json_mod: json.modified.unwrap(),
            json_path: json.current_file.unwrap(),
            nodes: json.program.into(),
            current_node: Media::new(json.start_index.unwrap(), "".to_string()),
            init: true,
            idx: json.start_index.unwrap(),
        }
    }

    fn check_update(&mut self) {
        let mod_time = modified_time(self.json_path.clone());

        if !mod_time.unwrap().to_string().eq(&self.json_mod) {
            // when playlist has changed, reload it
            let json = read_json(&self.config, false);

            self.json_mod = json.modified.unwrap();
            self.nodes = json.program.into();
        }
    }

    fn check_for_next_playlist(&mut self, last: bool) -> f64 {
        let mut out = self.current_node.out.clone();
        let start_sec = time_to_sec(&self.config.playlist.day_start);
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

        // println!("{}", next_start);

        next_start
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
}

impl Iterator for CurrentProgram {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.nodes.len() {
            self.check_update();

            if self.init {
                self.current_node = handle_list_init(self.nodes[self.idx].clone(), &self.config);
                self.init = false;
            } else {
                let new_source = timed_source(self.nodes[self.idx].clone(), &self.config, false);
                self.current_node = match new_source {
                    Some(src) => src,
                    None => {
                        let mut media = Media::new(self.idx, "".to_string());
                        media.process = Some(false);

                        media
                    }
                };

                let (delta, _) = get_delta(&self.current_node.begin.unwrap(), &self.config);
                let sync = check_sync(delta, &self.config);

                if !sync {
                    self.current_node.cmd = None;

                    return Some(self.current_node.clone());
                }
            }

            self.current_node.last_ad = self.is_ad(self.idx, false);
            self.current_node.next_ad = self.is_ad(self.idx, false);

            self.idx += 1;

            self.check_for_next_playlist(false);
            Some(self.current_node.clone())
        } else {
            let (_, time_diff) = get_delta(&get_sec(), &self.config);
            let mut last_ad = self.is_ad(self.idx, false);

            if time_diff.abs() > self.config.general.stop_threshold {
                self.current_node = Media::new(self.idx + 1, "".to_string());
                self.current_node.begin = Some(get_sec());
                let mut duration = time_diff.abs();

                if duration > 60.0 {
                    duration = 60.0;
                }
                self.current_node.duration = duration;
                self.current_node.out = duration;
                self.current_node = gen_source(self.current_node.clone(), &self.config);
                self.nodes.push(self.current_node.clone());

                last_ad = self.is_ad(self.idx, false);
                self.current_node.last_ad = last_ad;
                self.current_node.add_filter(&self.config);

                return Some(self.current_node.clone());
            }

            let json = read_json(&self.config, false);
            self.json_mod = json.modified.unwrap();
            self.json_path = json.current_file.unwrap();
            self.nodes = json.program.into();

            if self.init {
                self.current_node = handle_list_init(self.nodes[0].clone(), &self.config);
                self.init = false;
            } else {
                let new_source = timed_source(self.nodes[0].clone(), &self.config, false);
                self.current_node = match new_source {
                    Some(src) => src,
                    None => {
                        let mut media = Media::new(self.idx, "".to_string());
                        media.process = Some(false);

                        media
                    }
                };

                let (delta, _) = get_delta(&self.current_node.begin.unwrap(), &self.config);
                let sync = check_sync(delta, &self.config);

                if !sync {
                    self.current_node.cmd = None;

                    return Some(self.current_node.clone());
                }
            }

            self.current_node.last_ad = last_ad;
            self.current_node.next_ad = self.is_ad(0, false);

            self.idx = 1;

            Some(self.current_node.clone())
        }
    }
}

fn timed_source(node: Media, config: &Config, last: bool) -> Option<Media> {
    // prepare input clip
    // check begin and length from clip
    // return clip only if we are in 24 hours time range

    let (delta, total_delta) = get_delta(&node.begin.unwrap(), &config);
    let mut new_node = None;

    if config.playlist.day_start.contains(":") && config.playlist.length.contains(":") {
        debug!("Delta: <yellow>{delta}</>");
        check_sync(delta, &config);
    }

    if (total_delta > node.out - node.seek && !last) || !config.playlist.length.contains(":") {
        // when we are in the 24 hour range, get the clip
        new_node = Some(gen_source(node, &config));
    } else if total_delta <= 0.0 {
        info!("Begin is over play time, skip: {}", node.source);
    } else if total_delta < node.duration - node.seek || last {
        println!("handle list end");
        new_node = handle_list_end(node, total_delta);
    }

    return new_node;
}

fn gen_source(mut node: Media, config: &Config) -> Media {
    if Path::new(&node.source).is_file() {
        node.add_probe();
        node.add_filter(&config);
    } else {
        if node.source.chars().count() == 0 {
            warn!(
                "Generate filler with <yellow>{}</> seconds length!",
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

    debug!("Playlist init");

    let (_, total_delta) = get_delta(&node.begin.unwrap(), config);
    let mut out = node.out;

    if node.out - node.seek > total_delta {
        out = total_delta + node.seek;
    }

    node.out = out;
    let new_node = gen_source(node, &config);

    new_node
}

fn handle_list_end(mut node: Media, total_delta: f64) -> Option<Media> {
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
        return None;
    } else {
        error!(
            "Playlist is not long enough: <yellow>{:.2}</> seconds needed",
            total_delta
        );
    }

    Some(node)
}
