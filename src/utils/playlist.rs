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
        let program: Vec<Program> = read_json(&config).program;
        Self {
            nodes: program,
            idx: 0,
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
            let program: Vec<Program> = read_json(&config).program;
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
