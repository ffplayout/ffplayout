use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::channel,
        {Arc, Mutex},
    },
    thread::sleep,
    time::Duration,
};

use notify::{
    DebouncedEvent::{Create, Remove, Rename},
    {watcher, RecursiveMode, Watcher},
};
use simplelog::*;

use ffplayout_lib::utils::{Media, PlayoutConfig};

/// Create a watcher, which monitor file changes.
/// When a change is register, update the current file list.
/// This makes it possible, to play infinitely and and always new files to it.
pub fn watchman(
    config: PlayoutConfig,
    is_terminated: Arc<AtomicBool>,
    sources: Arc<Mutex<Vec<Media>>>,
) {
    let (tx, rx) = channel();

    let path = config.storage.path;

    if !Path::new(&path).exists() {
        error!("Folder path not exists: '{path}'");
        panic!("Folder path not exists: '{path}'");
    }

    let mut watcher = watcher(tx, Duration::from_secs(1)).unwrap();
    watcher.watch(path, RecursiveMode::Recursive).unwrap();

    while !is_terminated.load(Ordering::SeqCst) {
        if let Ok(res) = rx.try_recv() {
            match res {
                Create(new_path) => {
                    let index = sources.lock().unwrap().len();
                    let media = Media::new(index, new_path.display().to_string(), false);

                    sources.lock().unwrap().push(media);
                    info!("Create new file: <b><magenta>{new_path:?}</></b>");
                }
                Remove(old_path) => {
                    sources
                        .lock()
                        .unwrap()
                        .retain(|x| x.source != old_path.display().to_string());
                    info!("Remove file: <b><magenta>{old_path:?}</></b>");
                }
                Rename(old_path, new_path) => {
                    let index = sources
                        .lock()
                        .unwrap()
                        .iter()
                        .position(|x| *x.source == old_path.display().to_string())
                        .unwrap();

                    let media = Media::new(index, new_path.display().to_string(), false);
                    sources.lock().unwrap()[index] = media;

                    info!("Rename file: <b><magenta>{old_path:?}</></b> to <b><magenta>{new_path:?}</></b>");
                }
                _ => (),
            }
        }

        sleep(Duration::from_secs(5));
    }
}
