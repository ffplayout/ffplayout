use crate::utils::{
    get_config,
    json_reader::{read_json, Program},
};

pub struct CurrentProgram {
    nodes: Vec<Program>,
    idx: usize,
}

impl CurrentProgram {
    fn new() -> Self {
        let config = get_config();
        let json = read_json(&config, true);
        let program: Vec<Program> = json.program;
        Self {
            nodes: program,
            idx: json.start_index.unwrap(),
        }
    }
}

impl Iterator for CurrentProgram {
    type Item = Program;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.nodes.len() {
            let current = self.nodes[self.idx].clone();
            self.idx += 1;

            Some(current)
        } else {
            // play first item from next playlist
            let config = get_config();
            let program: Vec<Program> = read_json(&config, false).program;
            self.nodes = program;
            self.idx = 1;

            let current = self.nodes[0].clone();

            Some(current)
        }
    }
}

pub fn program() -> CurrentProgram {
    CurrentProgram::new()
}
