use notify::DebouncedEvent::{Create, Remove, Rename};
use rand::{seq::SliceRandom, thread_rng};
use simplelog::*;
use std::{
    ffi::OsStr,
    path::Path,
    sync::{
        mpsc::Receiver,
        {Arc, Mutex},
    },
    thread::sleep,
    time::Duration,
};

use walkdir::WalkDir;

use crate::utils::{Config, Media};

#[derive(Debug, Clone)]
pub struct Source {
    config: Config,
    pub nodes: Arc<Mutex<Vec<String>>>,
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
            nodes: Arc::new(Mutex::new(file_list)),
            index: 0,
        }
    }

    fn shuffle(&mut self) {
        let mut rng = thread_rng();
        self.nodes.lock().unwrap().shuffle(&mut rng);
    }

    fn sort(&mut self) {
        self.nodes.lock().unwrap().sort();
    }
}

impl Iterator for Source {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.nodes.lock().unwrap().len() {
            let current_file = self.nodes.lock().unwrap()[self.index].clone();
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

            let current_file = self.nodes.lock().unwrap()[0].clone();
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

pub async fn watch_folder(
    receiver: Receiver<notify::DebouncedEvent>,
    stop: Arc<Mutex<bool>>,
    sources: Arc<Mutex<Vec<String>>>,
) {
    loop {
        if *stop.lock().unwrap() {
            break;
        }

        match receiver.recv() {
            Ok(event) => match event {
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
            },
            Err(e) => {
                println!("{:?}", e);
            }
        }

        sleep(Duration::from_secs(2));
    }
}
