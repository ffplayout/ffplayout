use std::sync::{
    atomic::Ordering,
    {Arc, Mutex},
};

use lexical_sort::natural_lexical_cmp;
use log::*;
use rand::{seq::SliceRandom, thread_rng};
use walkdir::WalkDir;

use crate::player::{
    controller::ChannelManager,
    utils::{include_file_extension, time_in_seconds, Media, PlayoutConfig},
};
use crate::utils::logging::Target;

/// Folder Sources
///
/// Like playlist source, we create here a folder list for iterate over it.
#[derive(Debug, Clone)]
pub struct FolderSource {
    manager: ChannelManager,
    current_node: Media,
}

impl FolderSource {
    pub fn new(config: &PlayoutConfig, manager: ChannelManager) -> Self {
        let id = config.general.channel_id;
        let mut path_list = vec![];
        let mut media_list = vec![];
        let mut index: usize = 0;

        debug!(target: Target::file_mail(), channel = id;
            "generate: {:?}, paths: {:?}",
            config.general.generate, config.storage.paths
        );

        if config.general.generate.is_some() && !config.storage.paths.is_empty() {
            for path in &config.storage.paths {
                path_list.push(path);
            }
        } else {
            path_list.push(&config.channel.storage);
        }

        for path in &path_list {
            if !path.is_dir() {
                error!(target: Target::file_mail(), channel = id; "Path not exists: <b><magenta>{path:?}</></b>");
            }

            for entry in WalkDir::new(path)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|f| f.path().is_file())
                .filter(|f| include_file_extension(config, f.path()))
            {
                let media = Media::new(0, &entry.path().to_string_lossy(), false);
                media_list.push(media);
            }
        }

        if media_list.is_empty() {
            error!(target: Target::file_mail(), channel = id;
                "no playable files found under: <b><magenta>{:?}</></b>",
                path_list
            );
        }

        if config.storage.shuffle {
            info!(target: Target::file_mail(), channel = id; "Shuffle files");
            let mut rng = thread_rng();
            media_list.shuffle(&mut rng);
        } else {
            media_list.sort_by(|d1, d2| d1.source.cmp(&d2.source));
        }

        for item in &mut media_list {
            item.index = Some(index);

            index += 1;
        }

        *manager.current_list.lock().unwrap() = media_list;

        Self {
            manager,
            current_node: Media::new(0, "", false),
        }
    }

    pub fn from_list(manager: &ChannelManager, list: Vec<Media>) -> Self {
        *manager.current_list.lock().unwrap() = list;

        Self {
            manager: manager.clone(),
            current_node: Media::new(0, "", false),
        }
    }

    fn shuffle(&mut self) {
        let mut rng = thread_rng();
        let mut nodes = self.manager.current_list.lock().unwrap();

        nodes.shuffle(&mut rng);

        for (index, item) in nodes.iter_mut().enumerate() {
            item.index = Some(index);
        }
    }

    fn sort(&mut self) {
        let mut nodes = self.manager.current_list.lock().unwrap();

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
        let config = self.manager.config.lock().unwrap().clone();
        let id = config.general.id;

        if self.manager.current_index.load(Ordering::SeqCst)
            < self.manager.current_list.lock().unwrap().len()
        {
            let i = self.manager.current_index.load(Ordering::SeqCst);
            self.current_node = self.manager.current_list.lock().unwrap()[i].clone();
            let _ = self.current_node.add_probe(false).ok();
            self.current_node
                .add_filter(&config, &self.manager.filter_chain);
            self.current_node.begin = Some(time_in_seconds());
            self.manager.current_index.fetch_add(1, Ordering::SeqCst);
        } else {
            if config.storage.shuffle {
                if config.general.generate.is_none() {
                    info!(target: Target::file_mail(), channel = id; "Shuffle files");
                }

                self.shuffle();
            } else {
                if config.general.generate.is_none() {
                    info!(target: Target::file_mail(), channel = id; "Sort files");
                }

                self.sort();
            }

            self.current_node = match self.manager.current_list.lock().unwrap().first() {
                Some(m) => m.clone(),
                None => return None,
            };
            let _ = self.current_node.add_probe(false).ok();
            self.current_node
                .add_filter(&config, &self.manager.filter_chain);
            self.current_node.begin = Some(time_in_seconds());
            self.manager.current_index.store(1, Ordering::SeqCst);
        }

        Some(self.current_node.clone())
    }
}

pub fn fill_filler_list(
    config: &PlayoutConfig,
    fillers: Option<Arc<Mutex<Vec<Media>>>>,
) -> Vec<Media> {
    let id = config.general.channel_id;
    let mut filler_list = vec![];
    let filler_path = &config.storage.filler_path;

    if filler_path.is_dir() {
        for (index, entry) in WalkDir::new(&config.storage.filler_path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|f| f.path().is_file())
            .filter(|f| include_file_extension(config, f.path()))
            .enumerate()
        {
            let mut media = Media::new(index, &entry.path().to_string_lossy(), false);

            if fillers.is_none() {
                if let Err(e) = media.add_probe(false) {
                    error!(target: Target::file_mail(), channel = id; "{e:?}");
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
        let mut media = Media::new(0, &config.storage.filler_path.to_string_lossy(), false);

        if fillers.is_none() {
            if let Err(e) = media.add_probe(false) {
                error!(target: Target::file_mail(), channel = id; "{e:?}");
            };
        }

        filler_list.push(media);

        if let Some(f) = fillers.as_ref() {
            f.lock().unwrap().clone_from(&filler_list);
        }
    }

    filler_list
}
