use std::path::Path;

use crate::utils::{
    check_sync, gen_dummy, get_delta,
    json_reader::{read_json, Program},
    modified_time, Config, MediaProbe, Messenger,
};

use crate::filter::filter_chains;

pub struct CurrentProgram {
    msg: Messenger,
    config: Config,
    json_mod: String,
    json_path: String,
    nodes: Vec<Program>,
    init: bool,
    idx: usize,
}

impl CurrentProgram {
    pub fn new(msg: &Messenger, config: Config) -> Self {
        let json = read_json(&msg, &config, true);

        Self {
            msg: msg.clone(),
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
            let json = read_json(&self.msg, &self.config, false);

            self.json_mod = json.modified.unwrap();
            self.nodes = json.program.into();
        }
    }

    fn append_probe(&mut self, node: &mut Program) {
        node.probe = Some(MediaProbe::new(node.source.clone()))
    }

    fn add_filter(&mut self, node: &mut Program, last: bool, next: bool) {
        node.filter = Some(filter_chains(node, &self.config, last, next));
    }
}

impl Iterator for CurrentProgram {
    type Item = Program;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.nodes.len() {
            self.check_update();
            let mut current = self.nodes[self.idx].clone();
            let mut last = false;
            let mut next = false;

            if self.idx > 0 && self.nodes[self.idx - 1].category == "advertisement" {
                last = true
            }

            self.msg.debug(format!("Last: {}", self.nodes[self.idx - 1].source));
            self.msg.debug(format!("Next: {}", self.nodes[self.idx + 1].source));

            self.idx += 1;

            if self.idx <= self.nodes.len() - 1 && self.nodes[self.idx].category == "advertisement"
            {
                next = true
            }

            if !self.init {
                let delta = get_delta(&current.begin.unwrap(), &self.config);
                self.msg.debug(format!("Delta: {delta}"));
                check_sync(delta, &self.config);
            }

            if Path::new(&current.source).is_file() {
                self.append_probe(&mut current);
                self.add_filter(&mut current, last, next);
            } else {
                self.msg.error(format!("File not found: {}", current.source));
                let dummy = gen_dummy(current.out - current.seek, &self.config);
                current.source = dummy.0;
                current.cmd = Some(dummy.1);
                current.filter = Some(vec![]);
            }

            self.init = false;
            Some(current)
        } else {
            let mut last = false;
            let mut next = false;

            if self.nodes[self.idx - 1].category == "advertisement" {
                last = true
            }

            let json = read_json(&self.msg, &self.config, false);
            self.json_mod = json.modified.unwrap();
            self.json_path = json.current_file.unwrap();
            self.nodes = json.program.into();
            self.idx = 1;

            let mut current = self.nodes[0].clone();

            if self.nodes[self.idx].category == "advertisement" {
                next = true
            }

            if !self.init {
                let delta = get_delta(&current.begin.unwrap(), &self.config);
                self.msg.debug(format!("Delta: {delta}"));
                check_sync(delta, &self.config);
            }

            if Path::new(&current.source).is_file() {
                self.append_probe(&mut current);
                self.add_filter(&mut current, last, next);
            } else {
                self.msg.error(format!("File not found: {}", current.source));
                let dummy = gen_dummy(current.out - current.seek, &self.config);
                current.source = dummy.0;
                current.cmd = Some(dummy.1);
                current.filter = Some(vec![]);
            }

            Some(current)
        }
    }
}
