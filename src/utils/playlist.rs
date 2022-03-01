use std::path::Path;

use simplelog::*;

use crate::utils::{
    check_sync, gen_dummy, get_delta, json_reader::read_json, modified_time, time_to_sec, Config,
    Media,
};

#[derive(Debug)]
pub struct CurrentProgram {
    config: Config,
    json_mod: String,
    json_path: String,
    nodes: Vec<Media>,
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

    fn check_for_next_playlist(&mut self, node: &Media, last: bool) {
        let mut out = node.out.clone();
        let start_sec = time_to_sec(&self.config.playlist.day_start);
        let mut delta = 0.0;

        if node.duration > node.out {
            out = node.duration.clone()
        }

        if last {
            let seek = if node.seek > 0.0 { node.seek } else { 0.0 };
            (delta, _) = get_delta(&node.begin.unwrap().clone(), &self.config);
            delta += seek + self.config.general.stop_threshold;
        }

        let next_start = node.begin.unwrap() - start_sec + out + delta;

        println!("{}", next_start);
    }
}

impl Iterator for CurrentProgram {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.nodes.len() {
            self.check_update();
            let mut current;

            if self.init {
                current = handle_list_init(self.nodes[self.idx].clone(), &self.config);
                self.init = false;
            } else {
                current = self.nodes[self.idx].clone();

                let (delta, _) = get_delta(&current.begin.unwrap(), &self.config);
                debug!("Delta: <yellow>{delta}</>");
                let sync = check_sync(delta, &self.config);

                if !sync {
                    current.cmd = None;

                    return Some(current);
                }
            }

            self.idx += 1;
            current = gen_source(current, &self.config);

            self.check_for_next_playlist(&current, true);
            Some(current)
        } else {
            let mut current;
            let json = read_json(&self.config, false);
            self.json_mod = json.modified.unwrap();
            self.json_path = json.current_file.unwrap();
            self.nodes = json.program.into();
            self.idx = 1;

            if self.init {
                current = handle_list_init(self.nodes[0].clone(), &self.config);
                self.init = false;
            } else {
                current = self.nodes[0].clone();

                let (delta, _) = get_delta(&current.begin.unwrap(), &self.config);
                debug!("Delta: <yellow>{delta}</>");
                let sync = check_sync(delta, &self.config);

                if !sync {
                    current.cmd = None;

                    return Some(current);
                }
            }

            current = gen_source(current, &self.config);

            Some(current)
        }
    }
}

fn gen_source(mut node: Media, config: &Config) -> Media {
    if Path::new(&node.source).is_file() {
        node.add_probe();
        node.add_filter(&config);
    } else {
        error!("File not found: {}", node.source);
        let dummy = gen_dummy(node.out - node.seek, &config);
        node.source = dummy.0;
        node.cmd = Some(dummy.1);
        node.filter = Some(vec![]);
    }

    node
}

fn handle_list_init(mut node: Media, config: &Config) -> Media {
    // handle init clip, but this clip can be the last one in playlist,
    // this we have to figure out and calculate the right length

    debug!("Playlist init");
    println!("{:?}", node);

    let (_, total_delta) = get_delta(&node.begin.unwrap(), config);

    let mut out = node.out;

    if node.out - node.seek > total_delta {
        out = total_delta + node.seek
    }

    node.out = out;

    node
}

fn handle_list_end(mut node: Media, total_delta: f64) -> Option<Media> {
    // when we come to last clip in playlist,
    // or when we reached total playtime,
    // we end up here

    debug!("Playlist end");

    let mut out = if node.seek > 0.0 {node.seek + total_delta} else {total_delta};

    // prevent looping
    if out > node.duration {
        out = node.duration
    } else {
        warn!("Clip length is not in time, new duration is: <yellow>{:.2}</>", total_delta)
    }

    if node.duration > total_delta && total_delta > 1.0 && node.duration - node.seek >= total_delta {
        node.out = out;
    } else if node.duration > total_delta && total_delta < 1.0 {
        warn!("Last clip less then 1 second long, skip: {}", node.source);
        return None
    } else {
        error!("Playlist is not long enough: <yellow>{:.2}</> seconds needed", total_delta);
    }

    Some(node)
}
