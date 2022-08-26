use std::{
    path::Path,
    process::exit,
    sync::{
        atomic::{AtomicUsize, Ordering},
        {Arc, Mutex},
    },
};

use rand::{seq::SliceRandom, thread_rng};
use simplelog::*;
use walkdir::WalkDir;

use crate::utils::{get_sec, include_file, Media, PlayoutConfig};

/// Folder Sources
///
/// Like playlist source, we create here a folder list for iterate over it.
#[derive(Debug, Clone)]
pub struct FolderSource {
    config: PlayoutConfig,
    filter_chain: Arc<Mutex<Vec<String>>>,
    pub nodes: Arc<Mutex<Vec<Media>>>,
    current_node: Media,
    index: Arc<AtomicUsize>,
}

impl FolderSource {
    pub fn new(
        config: &PlayoutConfig,
        filter_chain: Arc<Mutex<Vec<String>>>,
        current_list: Arc<Mutex<Vec<Media>>>,
        global_index: Arc<AtomicUsize>,
    ) -> Self {
        let mut media_list = vec![];
        let mut index: usize = 0;

        if !Path::new(&config.storage.path).is_dir() {
            error!(
                "Path not exists: <b><magenta>{}</></b>",
                config.storage.path
            );
            exit(1);
        }

        for entry in WalkDir::new(config.storage.path.clone())
            .into_iter()
            .flat_map(|e| e.ok())
            .filter(|f| f.path().is_file())
        {
            if include_file(config.clone(), entry.path()) {
                let media = Media::new(0, entry.path().display().to_string(), false);
                media_list.push(media);
            }
        }

        if media_list.is_empty() {
            error!(
                "no playable files found under: <b><magenta>{}</></b>",
                config.storage.path
            );

            exit(1);
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
            filter_chain,
            nodes: current_list,
            current_node: Media::new(0, String::new(), false),
            index: global_index,
        }
    }

    fn shuffle(&mut self) {
        let mut rng = thread_rng();
        let mut nodes = self.nodes.lock().unwrap();

        nodes.shuffle(&mut rng);

        for (index, item) in nodes.iter_mut().enumerate() {
            item.index = Some(index);
        }
    }

    fn sort(&mut self) {
        let mut nodes = self.nodes.lock().unwrap();

        nodes.sort_by(|d1, d2| d1.source.cmp(&d2.source));

        for (index, item) in nodes.iter_mut().enumerate() {
            item.index = Some(index);
        }
    }
}

/// Create iterator for folder source
impl Iterator for FolderSource {
    type Item = Media;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index.load(Ordering::SeqCst) < self.nodes.lock().unwrap().len() {
            let i = self.index.load(Ordering::SeqCst);
            self.current_node = self.nodes.lock().unwrap()[i].clone();
            self.current_node.add_probe();
            self.current_node
                .add_filter(&self.config, &self.filter_chain);
            self.current_node.begin = Some(get_sec());

            self.index.fetch_add(1, Ordering::SeqCst);

            Some(self.current_node.clone())
        } else {
            if self.config.storage.shuffle {
                if self.config.general.generate.is_none() {
                    info!("Shuffle files");
                }

                self.shuffle();
            } else {
                if self.config.general.generate.is_none() {
                    info!("Sort files");
                }

                self.sort();
            }

            self.current_node = self.nodes.lock().unwrap()[0].clone();
            self.current_node.add_probe();
            self.current_node
                .add_filter(&self.config, &self.filter_chain);
            self.current_node.begin = Some(get_sec());

            self.index.store(1, Ordering::SeqCst);

            Some(self.current_node.clone())
        }
    }
}
