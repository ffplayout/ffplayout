use std::sync::{
    atomic::{AtomicUsize, Ordering},
    {Arc, Mutex},
};

use lexical_sort::natural_lexical_cmp;
use rand::{seq::SliceRandom, thread_rng};
use simplelog::*;
use walkdir::WalkDir;

use crate::player::utils::{include_file_extension, time_in_seconds, Media, PlayoutConfig};

/// Folder Sources
///
/// Like playlist source, we create here a folder list for iterate over it.
#[derive(Debug, Clone)]
pub struct FolderSource {
    config: PlayoutConfig,
    filter_chain: Option<Arc<Mutex<Vec<String>>>>,
    pub current_list: Arc<Mutex<Vec<Media>>>,
    pub current_index: Arc<AtomicUsize>,
    current_node: Media,
}

impl FolderSource {
    pub fn new(
        config: &PlayoutConfig,
        filter_chain: Option<Arc<Mutex<Vec<String>>>>,
        current_list: Arc<Mutex<Vec<Media>>>,
        current_index: Arc<AtomicUsize>,
    ) -> Self {
        let mut path_list = vec![];
        let mut media_list = vec![];
        let mut index: usize = 0;

        if config.general.generate.is_some() && !config.storage.paths.is_empty() {
            for path in &config.storage.paths {
                path_list.push(path)
            }
        } else {
            path_list.push(&config.storage.path)
        }

        for path in &path_list {
            if !path.is_dir() {
                error!("Path not exists: <b><magenta>{path:?}</></b>");
            }

            for entry in WalkDir::new(path)
                .into_iter()
                .flat_map(|e| e.ok())
                .filter(|f| f.path().is_file())
                .filter(|f| include_file_extension(config, f.path()))
            {
                let media = Media::new(0, &entry.path().to_string_lossy(), false);
                media_list.push(media);
            }
        }

        if media_list.is_empty() {
            error!(
                "no playable files found under: <b><magenta>{:?}</></b>",
                path_list
            );
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
            current_list,
            current_index,
            current_node: Media::new(0, "", false),
        }
    }

    pub fn from_list(
        config: &PlayoutConfig,
        filter_chain: Option<Arc<Mutex<Vec<String>>>>,
        current_list: Arc<Mutex<Vec<Media>>>,
        current_index: Arc<AtomicUsize>,
        list: Vec<Media>,
    ) -> Self {
        *current_list.lock().unwrap() = list;

        Self {
            config: config.clone(),
            filter_chain,
            current_list,
            current_index,
            current_node: Media::new(0, "", false),
        }
    }

    fn shuffle(&mut self) {
        let mut rng = thread_rng();
        let mut nodes = self.current_list.lock().unwrap();

        nodes.shuffle(&mut rng);

        for (index, item) in nodes.iter_mut().enumerate() {
            item.index = Some(index);
        }
    }

    fn sort(&mut self) {
        let mut nodes = self.current_list.lock().unwrap();

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
        if self.current_index.load(Ordering::SeqCst) < self.current_list.lock().unwrap().len() {
            let i = self.current_index.load(Ordering::SeqCst);
            self.current_node = self.current_list.lock().unwrap()[i].clone();
            let _ = self.current_node.add_probe(false).ok();
            self.current_node
                .add_filter(&self.config, &self.filter_chain);
            self.current_node.begin = Some(time_in_seconds());

            self.current_index.fetch_add(1, Ordering::SeqCst);

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

            self.current_node = self.current_list.lock().unwrap()[0].clone();
            let _ = self.current_node.add_probe(false).ok();
            self.current_node
                .add_filter(&self.config, &self.filter_chain);
            self.current_node.begin = Some(time_in_seconds());

            self.current_index.store(1, Ordering::SeqCst);

            Some(self.current_node.clone())
        }
    }
}

pub fn fill_filler_list(
    config: &PlayoutConfig,
    fillers: Option<Arc<Mutex<Vec<Media>>>>,
) -> Vec<Media> {
    let mut filler_list = vec![];
    let filler_path = &config.storage.filler;

    if filler_path.is_dir() {
        for (index, entry) in WalkDir::new(&config.storage.filler)
            .into_iter()
            .flat_map(|e| e.ok())
            .filter(|f| f.path().is_file())
            .filter(|f| include_file_extension(config, f.path()))
            .enumerate()
        {
            let mut media = Media::new(index, &entry.path().to_string_lossy(), false);

            if fillers.is_none() {
                if let Err(e) = media.add_probe(false) {
                    error!("{e:?}");
                };
            }

            filler_list.push(media);
        }

        if config.storage.shuffle {
            let mut rng = thread_rng();

            filler_list.shuffle(&mut rng);
        } else {
            filler_list.sort_by(|d1, d2| natural_lexical_cmp(&d1.source, &d2.source));
        }

        for (index, item) in filler_list.iter_mut().enumerate() {
            item.index = Some(index);
        }

        if let Some(f) = fillers.as_ref() {
            f.lock().unwrap().clone_from(&filler_list);
        }
    } else if filler_path.is_file() {
        let mut media = Media::new(0, &config.storage.filler.to_string_lossy(), false);

        if fillers.is_none() {
            if let Err(e) = media.add_probe(false) {
                error!("{e:?}");
            };
        }

        filler_list.push(media);

        if let Some(f) = fillers.as_ref() {
            f.lock().unwrap().clone_from(&filler_list);
        }
    }

    filler_list
}
