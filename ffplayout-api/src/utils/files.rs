use relative_path::RelativePath;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use simplelog::*;

use crate::utils::{errors::ServiceError, playout_config};
use ffplayout_lib::utils::file_extension;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PathObject {
    root: String,
    #[serde(skip_deserializing)]
    folders: Vec<String>,
    #[serde(skip_deserializing)]
    files: Vec<String>,
}

impl PathObject {
    fn new(root: String) -> Self {
        Self {
            root,
            folders: vec![],
            files: vec![],
        }
    }
}

pub async fn browser(id: i64, path_obj: &PathObject) -> Result<PathObject, ServiceError> {
    let (config, _) = playout_config(&id).await?;
    let path = PathBuf::from(config.storage.path);
    let extensions = config.storage.extensions;
    let path_component = RelativePath::new(&path_obj.root)
        .normalize()
        .to_string()
        .replace("../", "");
    let path = path.join(path_component.clone());
    let mut obj = PathObject::new(path_component.clone());

    let mut paths: Vec<_> = match fs::read_dir(path) {
        Ok(p) => p.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            error!("{e} in {path_component}");
            return Err(ServiceError::InternalServerError);
        }
    };

    paths.sort_by_key(|dir| dir.path());

    for path in paths {
        let file_path = path.path().to_owned();
        let path_str = file_path.display().to_string();

        // ignore hidden files/folders on unix
        if path_str.contains("/.") {
            continue;
        }

        if file_path.is_dir() {
            obj.folders.push(path_str);
        } else if file_path.is_file() {
            if let Some(ext) = file_extension(&file_path) {
                if extensions.contains(&ext.to_string().to_lowercase()) {
                    obj.files.push(path_str);
                }
            }
        }
    }

    Ok(obj)
}
