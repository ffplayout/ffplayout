use log::*;

pub mod folder;
pub mod ingest;
pub mod playlist;

pub use folder::watchman;
pub use ingest::ingest_server;
pub use playlist::CurrentProgram;

use crate::player::{
    controller::ChannelManager,
    utils::{folder::FolderSource, Media},
};
use crate::utils::{config::ProcessMode::*, logging::Target};

pub enum SourceIterator {
    Folder(FolderSource),
    Playlist(CurrentProgram),
}

impl async_iterator::Iterator for SourceIterator {
    type Item = Media;

    async fn next(&mut self) -> Option<Self::Item> {
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
    let is_terminated = manager.is_terminated.clone();
    let current_list = manager.current_list.clone();

    match config.processing.mode {
        Folder => {
            info!(target: Target::file_mail(), channel = id; "Playout in folder mode");
            debug!(target: Target::file_mail(), channel = id;
                "Monitor folder: <b><magenta>{:?}</></b>",
                config.channel.storage
            );

            let config_clone = config.clone();
            let folder_source = FolderSource::new(&config, manager);
            let list_clone = current_list.clone();

            // Spawn a thread to monitor folder for file changes.
            tokio::spawn(
                async move { watchman(config_clone, is_terminated.clone(), list_clone).await },
            );

            SourceIterator::Folder(folder_source.await)
        }
        Playlist => {
            info!(target: Target::file_mail(), channel = id; "Playout in playlist mode");
            let program = CurrentProgram::new(manager);

            SourceIterator::Playlist(program.await)
        }
    }
}
