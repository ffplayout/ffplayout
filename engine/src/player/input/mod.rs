use log::*;

pub mod folder;
pub mod ingest;
pub mod playlist;

pub use ingest::ingest_server;
pub use playlist::CurrentProgram;

use crate::player::{controller::ChannelManager, input::folder::FolderSource, utils::Media};
use crate::utils::{config::ProcessMode::*, logging::Target};

pub enum SourceIterator {
    Folder(FolderSource),
    Playlist(CurrentProgram),
}

impl SourceIterator {
    pub async fn next(&mut self) -> Option<Media> {
        match self {
            SourceIterator::Folder(folder_source) => folder_source.next().await,
            SourceIterator::Playlist(program) => program.next().await,
        }
    }
}

/// Create a source iterator from playlist, or from folder.
pub async fn source_generator(manager: ChannelManager) -> SourceIterator {
    let config = manager.config.lock().await.clone();
    let id = config.general.channel_id;
    let is_alive = manager.is_alive.clone();
    let current_list = manager.current_list.clone();

    match config.processing.mode {
        Folder => {
            info!(target: Target::file_mail(), channel = id; "Playout in folder mode");
            let config_clone = config.clone();

            // Spawn a task to monitor folder for file changes.
            {
                let mut storage = manager.storage.lock().await;
                storage.watchman(config_clone, is_alive, current_list).await;
            }

            let folder_source = FolderSource::new(&config, manager);

            SourceIterator::Folder(folder_source.await)
        }
        Playlist => {
            info!(target: Target::file_mail(), channel = id; "Playout in playlist mode");
            let program = CurrentProgram::new(manager);

            SourceIterator::Playlist(program.await)
        }
    }
}
