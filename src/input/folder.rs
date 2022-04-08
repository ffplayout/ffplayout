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
};

use walkdir::WalkDir;

use crate::utils::{get_sec, GlobalConfig, Media};

#[derive(Debug, Clone)]
pub struct Source {
    config: GlobalConfig,
    pub nodes: Arc<Mutex<Vec<Media>>>,
    current_node: Media,
    index: Arc<Mutex<usize>>,
}

impl Source {
    pub fn new(
        current_list: Arc<Mutex<Vec<Media>>>,
        global_index: Arc<Mutex<usize>>,
    ) -> Self {
        let config = GlobalConfig::global();
        let mut media_list = vec![];
        let mut index: usize = 0;

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
                    let media = Media::new(0, entry.path().display().to_string(), false);
                    media_list.push(media);
                }
            }
        }

        if config.storage.shuffle {
            info!("Shuffle files");
            let mut rng = thread_rng();
            media_list.shuffle(&mut rng);
        } else {
            media_list.sort_by(|d1, d2| d1.source.cmp(&d2.source));
        }

        for item in media_list.iter_mut() {
            item.index = Some(index);

            index += 1;
        }

        *current_list.lock().unwrap() = media_list;

        Self {
            config: config.clone(),
            nodes: current_list,
            current_node: Media::new(0, String::new(), false),
            index: global_index,
        }
    }

    fn shuffle(&mut self) {
        let mut rng = thread_rng();
        self.nodes.lock().unwrap().shuffle(&mut rng);
        let mut index: usize = 0;

        for item in self.nodes.lock().unwrap().iter_mut() {
            item.index = Some(index);

            index += 1;
        }
    }

    fn sort(&mut self) {
        self.nodes
            .lock()
            .unwrap()
            .sort_by(|d1, d2| d1.source.cmp(&d2.source));
        let mut index: usize = 0;

        for item in self.nodes.lock().unwrap().iter_mut() {
            item.index = Some(index);

            index += 1;
        }
    }
}

impl Iterator for Source {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        if *self.index.lock().unwrap() < self.nodes.lock().unwrap().len() {
            let i = *self.index.lock().unwrap();
            self.current_node = self.nodes.lock().unwrap()[i].clone();
            self.current_node.add_probe();
            self.current_node.add_filter();
            self.current_node.begin = Some(get_sec());

            *self.index.lock().unwrap() += 1;

            Some(self.current_node.clone())
        } else {
            if self.config.storage.shuffle {
                info!("Shuffle files");
                self.shuffle();
            } else {
                info!("Sort files");
                self.sort();
            }

            self.current_node = self.nodes.lock().unwrap()[0].clone();
            self.current_node.add_probe();
            self.current_node.add_filter();
            self.current_node.begin = Some(get_sec());

            *self.index.lock().unwrap() = 1;

            Some(self.current_node.clone())
        }
    }
}

fn file_extension(filename: &Path) -> Option<&str> {
    filename.extension().and_then(OsStr::to_str)
}

pub async fn file_worker(
    receiver: Receiver<notify::DebouncedEvent>,
    sources: Arc<Mutex<Vec<Media>>>,
) {
    while let Ok(res) = receiver.recv() {
        match res {
            Create(new_path) => {
                let index = sources.lock().unwrap().len();
                let media = Media::new(index, new_path.display().to_string(), false);

                sources.lock().unwrap().push(media);
                info!("Create new file: {new_path:?}");
            }
            Remove(old_path) => {
                sources
                    .lock()
                    .unwrap()
                    .retain(|x| x.source != old_path.display().to_string());
                info!("Remove file: {old_path:?}");
            }
            Rename(old_path, new_path) => {
                let index = sources
                    .lock()
                    .unwrap()
                    .iter()
                    .position(|x| *x.source == old_path.display().to_string())
                    .unwrap();

                let media = Media::new(index, new_path.display().to_string(), false);
                sources.lock().unwrap()[index] = media;

                info!("Rename file: {old_path:?} to {new_path:?}");
            }
            _ => (),
        }
    }
}
