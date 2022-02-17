use std::{
    thread::sleep,
    time::Duration,
};

struct List {
    arr: Vec<u8>,
    msg: String,
    i: usize,
}

impl List {
    fn new() -> Self {
        Self {
            arr: (0..10).collect(),
            msg: "fist init".to_string(),
            i: 0,
        }
    }

    fn fill(&mut self, val: String) {
        println!("{val}");
        self.msg = "new fill".to_string();
    }
}

impl Iterator for List {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == 0 {
            println!("{}", self.msg);
        }
        if self.i < self.arr.len() {
            let current = self.arr[self.i];
            self.i += 1;

            Some(current)
        } else {
            self.i = 1;
            let current = self.arr[0];
            self.fill("pass to function".to_string());
            println!("{}", self.msg);

            Some(current)
        }
    }
}

fn main() {
    let list = List::new();

    for i in list {
        println!("{i}");
        sleep(Duration::from_millis(300));
    }
}
