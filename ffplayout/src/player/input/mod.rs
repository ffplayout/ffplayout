use std::thread;

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

/// Create a source iterator from playlist, or from folder.
pub fn source_generator(manager: ChannelManager) -> Box<dyn Iterator<Item = Media>> {
    let config = manager.config.lock().unwrap().clone();
    let id = config.general.channel_id;
    let is_terminated = manager.is_terminated.clone();
    let current_list = manager.current_list.clone();

    match config.processing.mode {
        Folder => {
            info!(target: Target::file_mail(), channel = id; "Playout in folder mode");
            debug!(target: Target::file_mail(), channel = id;
                "Monitor folder: <b><magenta>{:?}</></b>",
                config.channel.storage_path
            );

            let config_clone = config.clone();
            let folder_source = FolderSource::new(&config, manager);
            let list_clone = current_list.clone();

            // Spawn a thread to monitor folder for file changes.
            thread::spawn(move || watchman(config_clone, is_terminated.clone(), list_clone));

            Box::new(folder_source) as Box<dyn Iterator<Item = Media>>
        }
        Playlist => {
            info!(target: Target::file_mail(), channel = id; "Playout in playlist mode");
            let program = CurrentProgram::new(manager);

            Box::new(program) as Box<dyn Iterator<Item = Media>>
        }
    }
}
