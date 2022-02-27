use notify::DebouncedEvent::{Create, Remove, Rename};
use notify::{watcher, RecursiveMode, Watcher};
use rand::{seq::SliceRandom, thread_rng};
use std::{
    ffi::OsStr,
    path::Path,
    sync::{mpsc::channel, Arc},
    time::Duration,
};

use tokio::sync::Mutex;

use walkdir::WalkDir;

use simplelog::*;

use crate::utils::{Config, Media};

#[derive(Debug, Clone)]
pub struct Source {
    config: Config,
    nodes: Vec<String>,
    index: usize,
}

impl Source {
    pub fn new(config: Config) -> Self {
        let mut file_list = vec![];

        for entry in WalkDir::new(config.storage.path.clone())
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.path().is_file() {
                let ext = file_extension(entry.path());

                if ext.is_some()
                    && config
                        .storage
                        .extensions
                        .clone()
                        .contains(&ext.unwrap().to_lowercase())
                {
                    file_list.push(entry.path().display().to_string());
                }
            }
        }

        if config.storage.shuffle {
            info!("Shuffle files");
            let mut rng = thread_rng();
            file_list.shuffle(&mut rng);
        } else {
            file_list.sort();
        }

        Self {
            config: config,
            nodes: file_list,
            index: 0,
        }
    }

    fn push(&mut self, file: String) {
        self.nodes.push(file)
    }

    fn rm(&mut self, file: String) {
        self.nodes.retain(|x| x != &file);
    }

    fn mv(&mut self, old_file: String, new_file: String) {
        let i = self.nodes.iter().position(|x| *x == old_file).unwrap();
        self.nodes[i] = new_file;
    }

    fn shuffle(&mut self) {
        let mut rng = thread_rng();
        self.nodes.shuffle(&mut rng);
    }

    fn sort(&mut self) {
        self.nodes.sort();
    }
}

impl Iterator for Source {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.nodes.len() {
            let current_file = self.nodes[self.index].clone();
            let mut media = Media::new(self.index, current_file);
            media.add_probe();
            media.add_filter(&self.config, false, false);
            self.index += 1;

            Some(media)
        } else {
            if self.config.storage.shuffle {
                info!("Shuffle files");
                self.shuffle();
            } else {
                self.sort();
            }

            let current_file = self.nodes[0].clone();
            let mut media = Media::new(self.index, current_file);
            media.add_probe();
            media.add_filter(&self.config, false, false);
            self.index = 1;

            Some(media)
        }
    }
}

fn file_extension(filename: &Path) -> Option<&str> {
    filename.extension().and_then(OsStr::to_str)
}

pub async fn watch_folder(path: &String, source: &mut Arc<Mutex<Source>>) {
    // let mut source = Source::new();
    let (sender, receiver) = channel();

    let mut watcher = watcher(sender, Duration::from_secs(2)).unwrap();
    watcher.watch(path, RecursiveMode::Recursive).unwrap();

    println!("watch path: '{}'", path);

    loop {
        match receiver.recv() {
            Ok(event) => match event {
                Create(new_path) => {
                    println!("Create new file: {:?}", new_path);
                    let mut lock = source.lock().await;
                    lock.push(new_path.display().to_string());
                }
                Remove(old_path) => {
                    println!("Remove file: {:?}", old_path);
                    let mut lock = source.lock().await;
                    lock.rm(old_path.display().to_string());
                }
                Rename(old_path, new_path) => {
                    println!("Rename file: {:?} to {:?}", old_path, new_path);
                    let mut lock = source.lock().await;
                    lock.mv(
                        old_path.display().to_string(),
                        new_path.display().to_string(),
                    );
                }
                _ => (),
            },
            Err(e) => println!("watch error: {:?}", e),
        }
    }
}
