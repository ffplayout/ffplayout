use crate::utils::{
    json_reader::{read_json, Program},
    modified_time, Config, MediaProbe,
};

use crate::filter::filter_chains;

pub struct CurrentProgram {
    config: Config,
    json_mod: String,
    json_path: String,
    nodes: Vec<Program>,
    idx: usize,
}

impl CurrentProgram {
    fn new(config: Config) -> Self {
        let json = read_json(&config, true);

        Self {
            config: config,
            json_mod: json.modified.unwrap(),
            json_path: json.current_file.unwrap(),
            nodes: json.program.into(),
            idx: json.start_index.unwrap(),
        }
    }

    fn check_update(&mut self) {
        let mod_time = modified_time(self.json_path.clone());

        if !mod_time.unwrap().to_string().eq(&self.json_mod) {
            // when playlist has changed, reload it
            let json = read_json(&self.config, true);

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

            self.idx += 1;

            if self.idx <= self.nodes.len() - 1 && self.nodes[self.idx].category == "advertisement" {
                next = true
            }

            self.append_probe(&mut current);
            self.add_filter(&mut current, last, next);

            Some(current)
        } else {
            let mut last = false;
            let mut next = false;

            if self.nodes[self.idx - 1].category == "advertisement" {
                last = true
            }

            let json = read_json(&self.config, false);
            self.json_mod = json.modified.unwrap();
            self.json_path = json.current_file.unwrap();
            self.nodes = json.program.into();
            self.idx = 1;

            let mut current = self.nodes[0].clone();

            if self.nodes[self.idx].category == "advertisement" {
                next = true
            }

            self.append_probe(&mut current);
            self.add_filter(&mut current, last, next);

            Some(current)
        }
    }
}

pub fn program(config: Config) -> CurrentProgram {
    CurrentProgram::new(config)
}
