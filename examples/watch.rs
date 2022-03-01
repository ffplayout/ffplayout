use notify::DebouncedEvent::{Create, Remove, Rename};
use notify::{watcher, RecursiveMode, Watcher};
use std::{
    ffi::OsStr,
    path::Path,
    sync::{
        mpsc::{channel, Receiver},
        {Arc, Mutex},
    },
    thread::sleep,
    time::Duration,
};

use walkdir::WalkDir;

use tokio::runtime::Builder;

#[derive(Debug, Clone)]
pub struct Source {
    nodes: Arc<Mutex<Vec<String>>>,
    index: usize,
}

impl Source {
    pub fn new(path: String) -> Self {
        let mut file_list = vec![];

        for entry in WalkDir::new(path.clone())
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.path().is_file() {
                let ext = file_extension(entry.path());

                if ext.is_some()
                    && ["mp4".to_string(), "mkv".to_string()]
                        .clone()
                        .contains(&ext.unwrap().to_lowercase())
                {
                    file_list.push(entry.path().display().to_string());
                }
            }
        }

        Self {
            nodes: Arc::new(Mutex::new(file_list)),
            index: 0,
        }
    }
}

impl Iterator for Source {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.nodes.lock().unwrap().len() {
            let current_file = self.nodes.lock().unwrap()[self.index].clone();
            self.index += 1;

            Some(current_file)
        } else {
            let current_file = self.nodes.lock().unwrap()[0].clone();

            self.index = 1;

            Some(current_file)
        }
    }
}

async fn watch(receiver: Receiver<notify::DebouncedEvent>, sources: Arc<Mutex<Vec<String>>>) {
    while let Ok(res) = receiver.recv() {
        match res {
            Create(new_path) => {
                sources.lock().unwrap().push(new_path.display().to_string());
                println!("Create new file: {:?}", new_path);
            }
            Remove(old_path) => {
                sources
                    .lock()
                    .unwrap()
                    .retain(|x| x != &old_path.display().to_string());
                println!("Remove file: {:?}", old_path);
            }
            Rename(old_path, new_path) => {
                let i = sources
                    .lock()
                    .unwrap()
                    .iter()
                    .position(|x| *x == old_path.display().to_string())
                    .unwrap();
                sources.lock().unwrap()[i] = new_path.display().to_string();
                println!("Rename file: {:?} to {:?}", old_path, new_path);
            }
            _ => (),
        }
    }
}

fn file_extension(filename: &Path) -> Option<&str> {
    filename.extension().and_then(OsStr::to_str)
}

fn main() {
    let path = "/home/jb/Videos/tv-media/ADtv/01 - Intro".to_string();
    let sources = Source::new(path.clone());

    let (sender, receiver) = channel();
    let runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("file_watcher")
        .enable_all()
        .build()
        .expect("Creating Tokio runtime");

    let mut watcher = watcher(sender, Duration::from_secs(2)).unwrap();

    watcher
        .watch(path.clone(), RecursiveMode::Recursive)
        .unwrap();

    runtime.spawn(watch(
        receiver,
        Arc::clone(&sources.nodes),
    ));

    let mut count = 0;

    for node in sources {
        println!("task: {:?}", node);
        sleep(Duration::from_secs(1));

        count += 1;

        if count == 5 {
            break;
        }
    }

    println!("after loop");
}
