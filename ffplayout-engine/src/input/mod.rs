use std::{
    sync::{atomic::AtomicBool, Arc},
    thread,
};

use simplelog::*;

use ffplayout_lib::utils::{Media, PlayoutConfig, PlayoutStatus, ProcessMode::*};

pub mod folder;
pub mod ingest;
pub mod playlist;

pub use folder::watchman;
pub use ingest::ingest_server;
pub use playlist::CurrentProgram;

use ffplayout_lib::utils::{controller::PlayerControl, folder::FolderSource};

/// Create a source iterator from playlist, or from folder.
pub fn source_generator(
    config: PlayoutConfig,
    player_control: &PlayerControl,
    playout_stat: PlayoutStatus,
    is_terminated: Arc<AtomicBool>,
) -> Box<dyn Iterator<Item = Media>> {
    match config.processing.mode {
        Folder => {
            info!("Playout in folder mode");
            debug!(
                "Monitor folder: <b><magenta>{:?}</></b>",
                config.storage.path
            );

            let config_clone = config.clone();
            let folder_source = FolderSource::new(&config, playout_stat.chain, player_control);
            let node_clone = folder_source.player_control.current_list.clone();

            // Spawn a thread to monitor folder for file changes.
            thread::spawn(move || watchman(config_clone, is_terminated.clone(), node_clone));

            Box::new(folder_source) as Box<dyn Iterator<Item = Media>>
        }
        Playlist => {
            info!("Playout in playlist mode");
            let program = CurrentProgram::new(&config, playout_stat, is_terminated, player_control);

            Box::new(program) as Box<dyn Iterator<Item = Media>>
        }
    }
}
