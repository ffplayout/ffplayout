use std::thread;

use simplelog::*;
use sqlx::{Pool, Sqlite};

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
use crate::utils::config::ProcessMode::*;

/// Create a source iterator from playlist, or from folder.
pub fn source_generator(
    manager: ChannelManager,
    db_pool: Pool<Sqlite>,
) -> Box<dyn Iterator<Item = Media>> {
    let config = manager.config.lock().unwrap().clone();
    let is_terminated = manager.is_terminated.clone();
    let chain = manager.chain.clone();
    let current_list = manager.current_list.clone();
    let current_index = manager.current_index.clone();

    match config.processing.mode {
        Folder => {
            info!("Playout in folder mode");
            debug!(
                "Monitor folder: <b><magenta>{:?}</></b>",
                config.storage.path
            );

            let config_clone = config.clone();
            let folder_source =
                FolderSource::new(&config, chain, current_list.clone(), current_index);
            let list_clone = current_list.clone();

            // Spawn a thread to monitor folder for file changes.
            thread::spawn(move || watchman(config_clone, is_terminated.clone(), list_clone));

            Box::new(folder_source) as Box<dyn Iterator<Item = Media>>
        }
        Playlist => {
            info!("Playout in playlist mode");
            let program = CurrentProgram::new(manager, db_pool);

            Box::new(program) as Box<dyn Iterator<Item = Media>>
        }
    }
}
