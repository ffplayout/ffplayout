use std::{
    process,
    sync::{
        atomic::{AtomicBool, AtomicUsize},
        Arc, Mutex,
    },
    thread,
};

use simplelog::*;

use ffplayout_lib::utils::{Media, PlayoutConfig, PlayoutStatus};

pub mod folder;
pub mod ingest;
pub mod playlist;

pub use folder::watchman;
pub use ingest::ingest_server;
pub use playlist::CurrentProgram;

use ffplayout_lib::utils::folder::FolderSource;

/// Create a source iterator from playlist, or from folder.
pub fn source_generator(
    config: PlayoutConfig,
    current_list: Arc<Mutex<Vec<Media>>>,
    index: Arc<AtomicUsize>,
    playout_stat: PlayoutStatus,
    is_terminated: Arc<AtomicBool>,
) -> Box<dyn Iterator<Item = Media>> {
    let get_source = match config.processing.mode.as_str() {
        "folder" => {
            info!("Playout in folder mode");
            debug!(
                "Monitor folder: <b><magenta>{}</></b>",
                &config.storage.path
            );

            let config_clone = config.clone();
            let folder_source = FolderSource::new(&config, playout_stat.chain, current_list, index);
            let node_clone = folder_source.nodes.clone();

            // Spawn a thread to monitor folder for file changes.
            thread::spawn(move || watchman(config_clone, is_terminated.clone(), node_clone));

            Box::new(folder_source) as Box<dyn Iterator<Item = Media>>
        }
        "playlist" => {
            info!("Playout in playlist mode");
            let program =
                CurrentProgram::new(&config, playout_stat, is_terminated, current_list, index);

            Box::new(program) as Box<dyn Iterator<Item = Media>>
        }
        _ => {
            error!("Process Mode not exists!");
            process::exit(1);
        }
    };

    get_source
}
