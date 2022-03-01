use notify::DebouncedEvent::{Create, Remove, Rename};
use notify::{watcher, RecursiveMode, Watcher};
use std::{
    sync::{
        mpsc::{channel, Receiver},
        {Arc, Mutex},
    },
    thread::sleep,
    time::Duration,
};

use tokio::runtime::Builder;

async fn watch(receiver: Receiver<notify::DebouncedEvent>, stop: Arc<Mutex<bool>>) {
    loop {
        if *stop.lock().unwrap() {
            break;
        }

        match receiver.recv() {
            Ok(event) => match event {
                Create(new_path) => {
                    println!("Create new file: {:?}", new_path);
                }
                Remove(old_path) => {
                    println!("Remove file: {:?}", old_path);
                }
                Rename(old_path, new_path) => {
                    println!("Rename file: {:?} to {:?}", old_path, new_path);
                }
                _ => (),
            },
            Err(e) => {
                println!("{:?}", e);
            }
        }

        sleep(Duration::from_secs(1));
    }
}

fn main() {
    let path = "/home/jb/Videos/tv-media/ADtv/01 - Intro".to_string();
    let stop = Arc::new(Mutex::new(false));

    let (sender, receiver) = channel();
    let mut watcher = watcher(sender, Duration::from_secs(2)).unwrap();
    watcher.watch(path.clone(), RecursiveMode::Recursive).unwrap();

    let runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("file_watcher")
        .enable_all()
        .build()
        .expect("Creating Tokio runtime");

    if true {
        runtime.spawn(watch(receiver, Arc::clone(&stop)));
    }

    let mut count = 0;

    loop {
        println!("task: {count}");
        sleep(Duration::from_secs(1));

        count += 1;

        if count == 5 {
            break;
        }
    }

    *stop.lock().unwrap() = true;

    watcher.unwatch(path).unwrap();

    println!("after loop");
}
