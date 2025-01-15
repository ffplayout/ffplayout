use std::sync::{atomic::Ordering, Arc};

use async_walkdir::{Filtering, WalkDir};
use lexical_sort::natural_lexical_cmp;
use log::*;
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;

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
    pub async fn new(config: &PlayoutConfig, manager: ChannelManager) -> Self {
        let id = config.general.channel_id;
        let mut path_list = vec![];
        let mut media_list = vec![];
        let mut index: usize = 0;

        if !config.storage.paths.is_empty() && config.general.generate.is_some() {
            path_list.extend(&config.storage.paths);
        } else {
            path_list.push(&config.channel.storage);
        }

        if let Some(dates) = &config.general.generate {
            debug!(target: Target::file_mail(), channel = id;
                "generate: {dates:?}, paths: {path_list:?}"
            );
        }

        for path in &path_list {
            if !path.is_dir() {
                error!(target: Target::file_mail(), channel = id; "Path not exists: <b><magenta>{path:?}</></b>");
            }

            let config = config.clone();
            let mut entries = WalkDir::new(path).filter(move |entry| {
                let config = config.clone();
                async move {
                    if entry.path().is_file() && include_file_extension(&config, &entry.path()) {
                        return Filtering::Continue;
                    }

                    Filtering::Ignore
                }
            });

            loop {
                match entries.next().await {
                    Some(Ok(entry)) => {
                        let media = Media::new(0, &entry.path().to_string_lossy(), false).await;
                        media_list.push(media);
                    }
                    Some(Err(e)) => {
                        error!(target: Target::file_mail(), channel = id; "error: {e}");
                        break;
                    }
                    None => break,
                }
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
            let mut rng = StdRng::from_entropy();
            media_list.shuffle(&mut rng);
        } else {
            media_list.sort_by(|d1, d2| d1.source.cmp(&d2.source));
        }

        for item in &mut media_list {
            item.index = Some(index);

            index += 1;
        }

        *manager.current_list.lock().await = media_list;

        Self {
            manager,
            current_node: Media::default(),
        }
    }

    pub async fn from_list(manager: &ChannelManager, list: Vec<Media>) -> Self {
        *manager.current_list.lock().await = list;

        Self {
            manager: manager.clone(),
            current_node: Media::default(),
        }
    }

    async fn shuffle(&mut self) {
        let mut rng = StdRng::from_entropy();
        let mut nodes = self.manager.current_list.lock().await;

        nodes.shuffle(&mut rng);

        for (index, item) in nodes.iter_mut().enumerate() {
            item.index = Some(index);
        }
    }

    async fn sort(&mut self) {
        let mut nodes = self.manager.current_list.lock().await;

        nodes.sort_by(|d1, d2| d1.source.cmp(&d2.source));

        for (index, item) in nodes.iter_mut().enumerate() {
            item.index = Some(index);
        }
    }
}

/// Create iterator for folder source
impl async_iterator::Iterator for FolderSource {
    type Item = Media;

    async fn next(&mut self) -> Option<Self::Item> {
        let config = self.manager.config.lock().await.clone();
        let id = config.general.id;

        if self.manager.current_index.load(Ordering::SeqCst)
            < self.manager.current_list.lock().await.len()
        {
            let i = self.manager.current_index.load(Ordering::SeqCst);
            self.current_node = self.manager.current_list.lock().await[i].clone();
            let _ = self.current_node.add_probe(false).await.ok();
            self.current_node
                .add_filter(&config, &self.manager.filter_chain)
                .await;
            self.current_node.begin = Some(time_in_seconds(&config.channel.timezone));
            self.manager.current_index.fetch_add(1, Ordering::SeqCst);
        } else {
            if config.storage.shuffle {
                if config.general.generate.is_none() {
                    info!(target: Target::file_mail(), channel = id; "Shuffle files");
                }

                self.shuffle().await;
            } else {
                if config.general.generate.is_none() {
                    info!(target: Target::file_mail(), channel = id; "Sort files");
                }

                self.sort().await;
            }

            self.current_node = match self.manager.current_list.lock().await.first() {
                Some(m) => m.clone(),
                None => return None,
            };
            let _ = self.current_node.add_probe(false).await.ok();
            self.current_node
                .add_filter(&config, &self.manager.filter_chain)
                .await;
            self.current_node.begin = Some(time_in_seconds(&config.channel.timezone));
            self.manager.current_index.store(1, Ordering::SeqCst);
        }

        Some(self.current_node.clone())
    }
}

pub async fn fill_filler_list(
    config: &PlayoutConfig,
    fillers: Option<Arc<Mutex<Vec<Media>>>>,
) -> Vec<Media> {
    let id = config.general.channel_id;
    let mut filler_list = vec![];
    let filler_path = &config.storage.filler_path;

    if filler_path.is_dir() {
        let config_clone = config.clone();
        let mut index = 0;

        let mut entries = WalkDir::new(&config_clone.storage.filler_path).filter(move |entry| {
            let config = config_clone.clone();
            async move {
                if entry.path().is_file() && include_file_extension(&config, &entry.path()) {
                    return Filtering::Continue;
                }

                Filtering::Ignore
            }
        });

        loop {
            match entries.next().await {
                Some(Ok(entry)) => {
                    let mut media = Media::new(index, &entry.path().to_string_lossy(), false).await;

                    if fillers.is_none() {
                        if let Err(e) = media.add_probe(false).await {
                            error!(target: Target::file_mail(), channel = id; "{e:?}");
                        };
                    }

                    filler_list.push(media);

                    index += 1;
                }
                Some(Err(e)) => {
                    error!(target: Target::file_mail(), "error: {e}");
                    break;
                }
                None => break,
            }
        }

        if config.storage.shuffle {
            let mut rng = StdRng::from_entropy();

            filler_list.shuffle(&mut rng);
        } else {
            filler_list.sort_by(|d1, d2| natural_lexical_cmp(&d1.source, &d2.source));
        }

        for (index, item) in filler_list.iter_mut().enumerate() {
            item.index = Some(index);
        }

        if let Some(f) = fillers.as_ref() {
            f.lock().await.clone_from(&filler_list);
        }
    } else if filler_path.is_file() {
        let mut media = Media::new(0, &config.storage.filler_path.to_string_lossy(), false).await;

        if fillers.is_none() {
            if let Err(e) = media.add_probe(false).await {
                error!(target: Target::file_mail(), channel = id; "{e:?}");
            };
        }

        filler_list.push(media);

        if let Some(f) = fillers.as_ref() {
            f.lock().await.clone_from(&filler_list);
        }
    }

    filler_list
}
