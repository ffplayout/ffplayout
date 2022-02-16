use crate::utils::get_config;
use crate::utils::json_reader::{Program, read_json};

pub struct CurrentProgram {
    nodes: Vec<Program>,
    idx: usize,
}

impl Iterator for CurrentProgram {
    type Item = Program;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.nodes.len() - 1 {
            let current = self.nodes[self.idx].clone();
            self.idx = 0;

            Some(current)
        } else {
            let current = self.nodes[self.idx].clone();
            self.idx += 1;

            Some(current)
        }
    }
}

pub fn program() -> CurrentProgram {
    let config = get_config();

    let program: Vec<Program> = read_json(&config).program;

    CurrentProgram {
        nodes: program,
        idx: 0,
    }
}
