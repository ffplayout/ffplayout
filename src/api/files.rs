use log::error;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, canonicalize},
    path::{self, PathBuf},
};

use simplelog::*;

use crate::api::{errors::ServiceError, utils::playout_config};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PathObject {
    root: String,
    folders: Vec<String>,
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
    let config = playout_config(&id).await?;
    let path = PathBuf::from(config.storage.path);
    let absolute = match canonicalize(path_obj.root.clone()) {
        Ok(p) => p,
        Err(e) => {
            error!("{e}");
            return Err(ServiceError::InternalServerError);
        }
    };
    let path = path.join(absolute.clone());
    let obj = PathObject::new(path_obj.root.clone());

    println!("absolute: {:?}", absolute);

    let paths = fs::read_dir(path).unwrap();

    for path in paths {
        println!("Name: {:?}", path);
        // if let Ok(p) = path {
        //     let file_path = p.path().to_owned();
        //     if file_path.is_dir() {
        //         folders.push(file_path.display())
        //     } else if file_path.is_file() {
        //         files.push(file_path.clone().display())
        //     }
        // }
    }

    Ok(obj)
}
