use chrono::prelude::*;
use std::{
    thread::sleep};

fn get_timestamp() -> i64 {
    let local: DateTime<Local> = Local::now();

    local.timestamp_millis() as i64
}

struct Timer {
    init: bool,
    timestamp: i64,
    limit: i64,
    messages: Vec<String>,
}

impl Timer {
    fn new() -> Self {
        Self {
            init: true,
            timestamp: get_timestamp(),
            limit: 10 * 1000,
            messages: vec![],
        }
    }

    fn reset(&mut self) {
        self.messages.clear();
        self.timestamp = get_timestamp();
    }

    fn send(&mut self, msg: String) {
        let now = get_timestamp();
        self.messages.push(msg);

        if self.init {
            self.reset();
            self.init = false;
        }

        if now >= self.timestamp + self.limit {
            println!("Send messages: {:?}", self.messages);

            self.reset();
        }
    }
}

fn main() {
    let mut timer = Timer::new();

    for i in 0..40 {
        println!("{:?}", i);
        timer.send(format!("{:?}", i));

        sleep(std::time::Duration::from_secs(1));
    }
}
