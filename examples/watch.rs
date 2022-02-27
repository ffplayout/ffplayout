use notify::EventKind::{Create, Modify, Remove};
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event};
use std::{
    path::Path,
    sync::mpsc::{channel, Receiver, Sender},
    thread::sleep,
    time::Duration,
};

use std::sync::{Arc, Mutex};

//std::sync::mpsc::Receiver<notify::DebouncedEvent>

use tokio::runtime::Builder;

// #[derive(Debug, Copy, Clone)]
// struct WatchMan {
//     stop: bool,
// }

// impl WatchMan {
//     fn new() -> Self {
//         Self {
//             stop: false,
//         }
//     }

//     async fn start(self, receiver: Receiver<notify::DebouncedEvent>) -> Result<(), String> {
//         loop {
//             if self.stop {
//                 println!("break out");
//                 break
//             }

//             match receiver.recv() {
//                 Ok(event) => match event {
//                     Create(new_path) => {
//                         println!("Create new file: {:?}", new_path);
//                     }
//                     Remove(old_path) => {
//                         println!("Remove file: {:?}", old_path);
//                     }
//                     Rename(old_path, new_path) => {
//                         println!("Rename file: {:?} to {:?}", old_path, new_path);
//                     }
//                     _ => (),
//                 },
//                 Err(e) => {
//                     println!("watch error: {:?}", e);
//                     println!("watch error: {:?}", self.stop);
//                     sleep(Duration::from_secs(1));
//                     // return Err(e.to_string())
//                 },
//             }
//         }

//         Ok(())
//     }

//     fn stop(&mut self) {
//         println!("stop watching");
//         self.stop = true;

//     }
// }

async fn watch(receiver: Receiver<Result<Event, notify::Error>>) {
    for res in receiver {
        match res {
            Ok(event) => println!("changed: {:?}", event),
            Err(e) => println!("watch error: {:?}", e),
        }
    }
}

fn main() -> notify::Result<()> {
    let runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("file_watcher")
        .enable_all()
        .build()
        .expect("Creating Tokio runtime");

    // let mut watch = WatchMan::new();

    let (sender, receiver) = channel();


    // let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(sender)?;
    watcher.watch(Path::new("/home/jb/Videos/"), RecursiveMode::Recursive).unwrap();

    runtime.spawn(watch(receiver));

    let mut count = 0;
    loop {
        println!("task: {}", count);
        sleep(Duration::from_secs(1));

        count += 1;

        if count == 15 {
            break;
        }
    }

    println!("after loop");
    watcher.unwatch(Path::new("/home/jb/Videos/")).unwrap();
    // watch.stop();

    // watch.run = false;

    //runtime.block_on(watch.stop());
    Ok(())
}
