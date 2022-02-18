use crate::utils::{
    get_config,
    json_reader::{read_json, Program},
    modified_time, MediaProbe,
};

pub struct CurrentProgram {
    json_mod: String,
    json_path: String,
    nodes: Vec<Program>,
    idx: usize,
}

impl CurrentProgram {
    fn new() -> Self {
        let config = get_config();
        let json = read_json(&config, true);

        Self {
            json_mod: json.modified.unwrap(),
            json_path: json.current_file.unwrap(),
            nodes: json.program.into(),
            idx: json.start_index.unwrap(),
        }
    }

    fn check_update(&mut self) {
        let config = get_config();
        let mod_time = modified_time(self.json_path.clone());

        if !mod_time.unwrap().to_string().eq(&self.json_mod) {
            // when playlist has changed, reload it
            let json = read_json(&config, true);

            self.json_mod = json.modified.unwrap();
            self.nodes = json.program.into();
        }
    }

    fn append_probe(&mut self, node: &mut Program) {
        node.probe = Some(MediaProbe::new(node.source.clone()))
    }
}

impl Iterator for CurrentProgram {
    type Item = Program;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.nodes.len() {
            self.check_update();
            let mut current = self.nodes[self.idx].clone();
            self.idx += 1;

            self.append_probe(&mut current);

            Some(current)
        } else {
            let config = get_config();
            let json = read_json(&config, false);
            self.json_mod = json.modified.unwrap();
            self.json_path = json.current_file.unwrap();
            self.nodes = json.program.into();
            self.idx = 1;

            let mut current = self.nodes[0].clone();

            self.append_probe(&mut current);

            Some(current)
        }
    }
}

pub fn program() -> CurrentProgram {
    CurrentProgram::new()
}
