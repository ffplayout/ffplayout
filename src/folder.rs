use notify::DebouncedEvent::{Create, Remove, Rename};
use notify::{watcher, RecursiveMode, Watcher};
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::ffi::OsStr;
use std::path::Path;
use std::process;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use std::{thread, time};
use walkdir::WalkDir;

#[derive(Clone)]
struct Source {
    files: Vec<String>,
}

impl Source {
    fn new(path: &String, extensions: &Vec<String>) -> Self {
        let mut file_list = vec![];

        for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
            if entry.path().is_file() {
                let ext = file_extension(entry.path()).unwrap().to_lowercase();

                if extensions.contains(&ext) {
                    file_list.push(entry.path().display().to_string());
                }
            }
        }

        Self { files: file_list }
    }

    fn push(&mut self, file: String) {
        self.files.push(file)
    }

    fn rm(&mut self, file: String) {
        self.files.retain(|x| x != &file);
    }

    fn mv(&mut self, old_file: String, new_file: String) {
        let i = self.files.iter().position(|x| *x == old_file).unwrap();
        self.files[i] = new_file;
    }

    fn shuffle(&mut self) {
        let mut rng = thread_rng();
        self.files.shuffle(&mut rng);
    }
}

fn file_extension(filename: &Path) -> Option<&str> {
    filename.extension().and_then(OsStr::to_str)
}

fn watch_folder(source: &mut Source, receiver: &Receiver<notify::DebouncedEvent>) {
    match receiver.try_recv() {
        Ok(event) => match event {
            Create(new_path) => {
                println!("Create new file: {:?}", new_path);
                source.push(new_path.display().to_string());
            }
            Remove(old_path) => {
                println!("Remove file: {:?}", old_path);
                source.rm(old_path.display().to_string());
            }
            Rename(old_path, new_path) => {
                println!("Rename file: {:?} to {:?}", old_path, new_path);
                source.mv(
                    old_path.display().to_string(),
                    new_path.display().to_string(),
                );
            }
            _ => (),
        },
        Err(_) => (),
    }
}

pub fn walk(path: &String, shuffle: bool, extensions: &Vec<String>) {
    if !Path::new(path).exists() {
        println!("Folder path not exists: '{}'", path);
        process::exit(0x0100);
    }
    let mut source = Source::new(path, extensions);
    let mut index: usize = 0;

    let (sender, receiver) = channel();
    let mut watcher = watcher(sender, Duration::from_secs(2)).unwrap();
    watcher.watch(path, RecursiveMode::Recursive).unwrap();

    loop {
        if shuffle {
            println!("Shuffle files in folder");
            source.shuffle();
        }

        while index < source.files.len() {
            watch_folder(&mut source, &receiver);
            println!("Play file {}: {:?}", index, source.files[index]);
            index += 1;

            thread::sleep(time::Duration::from_secs(1));
        }
        index = 0
    }
}
