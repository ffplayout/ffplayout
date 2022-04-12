use std::{
    fmt,
    process::Child,
    sync::{Arc, Mutex, RwLock},

};

use jsonrpc_http_server::CloseHandle;
use simplelog::*;

use crate::utils::Media;

pub enum ProcessUnit {
    Decoder,
    Encoder,
    Ingest,
}

impl fmt::Display for ProcessUnit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ProcessUnit::Decoder => write!(f, "Decoder"),
            ProcessUnit::Encoder => write!(f, "Encoder"),
            ProcessUnit::Ingest => write!(f, "Ingest"),
        }
    }
}

use ProcessUnit::*;

#[derive(Clone)]
pub struct ProcessControl {
    pub decoder_term: Arc<Mutex<Option<Child>>>,
    pub encoder_term: Arc<Mutex<Option<Child>>>,
    pub server_term: Arc<Mutex<Option<Child>>>,
    pub server_is_running: Arc<Mutex<bool>>,
    pub rpc_handle: Arc<Mutex<Option<CloseHandle>>>,
    pub is_terminated: Arc<Mutex<bool>>,
    pub is_alive: Arc<RwLock<bool>>,
}

impl ProcessControl {
    pub fn new() -> Self {
        Self {
            decoder_term: Arc::new(Mutex::new(None)),
            encoder_term: Arc::new(Mutex::new(None)),
            server_term: Arc::new(Mutex::new(None)),
            server_is_running: Arc::new(Mutex::new(false)),
            rpc_handle: Arc::new(Mutex::new(None)),
            is_terminated: Arc::new(Mutex::new(false)),
            is_alive: Arc::new(RwLock::new(true)),
        }
    }
}

impl ProcessControl {
    pub fn kill(&mut self, proc: ProcessUnit) -> Result<(), String> {
        match proc {
            Decoder => {
                if let Some(proc) = self.decoder_term.lock().unwrap().as_mut() {
                    if let Err(e) = proc.kill() {
                        return Err(format!("Decoder {e:?}"));
                    };
                }
            }
            Encoder => {
                if let Some(proc) = self.encoder_term.lock().unwrap().as_mut() {
                    if let Err(e) = proc.kill() {
                        return Err(format!("Encoder {e:?}"));
                    };
                }
            }
            Ingest => {
                if let Some(proc) = self.server_term.lock().unwrap().as_mut() {
                    if let Err(e) = proc.kill() {
                        return Err(format!("Ingest server {e:?}"));
                    };
                }
            }
        }

        if let Err(e) = self.wait(proc) {
            return Err(e);
        };

        Ok(())
    }

    pub fn wait(&mut self, proc: ProcessUnit) -> Result<(), String> {
        match proc {
            Decoder => {
                if let Some(proc) = self.decoder_term.lock().unwrap().as_mut() {
                    if let Err(e) = proc.wait() {
                        return Err(format!("Decoder {e:?}"));
                    };
                }
            }
            Encoder => {
                if let Some(proc) = self.encoder_term.lock().unwrap().as_mut() {
                    if let Err(e) = proc.wait() {
                        return Err(format!("Encoder {e:?}"));
                    };
                }
            }
            Ingest => {
                if let Some(proc) = self.server_term.lock().unwrap().as_mut() {
                    if let Err(e) = proc.wait() {
                        return Err(format!("Ingest server {e:?}"));
                    };
                }
            }
        }

        Ok(())
    }

    pub fn kill_all(&mut self) {
        *self.is_terminated.lock().unwrap() = true;

        if *self.is_alive.read().unwrap() {
            *self.is_alive.write().unwrap() = false;

            if let Some(rpc) = &*self.rpc_handle.lock().unwrap() {
                rpc.clone().close()
            };

            for unit in [
                Decoder,
                Encoder,
                Ingest,
            ] {
                if let Err(e) = self.kill(unit) {
                    error!("{e}")
                }
            }
        }
    }
}

impl Drop for ProcessControl {
    fn drop(&mut self) {
        self.kill_all()
    }
}

#[derive(Clone)]
pub struct PlayerControl {
    pub current_media: Arc<Mutex<Option<Media>>>,
    pub current_list: Arc<Mutex<Vec<Media>>>,
    pub index: Arc<Mutex<usize>>,
}

impl PlayerControl {
    pub fn new() -> Self {
        Self {
            current_media: Arc::new(Mutex::new(None)),
            current_list: Arc::new(Mutex::new(vec![Media::new(0, String::new(), false)])),
            index: Arc::new(Mutex::new(0)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PlayoutStatus {
    pub time_shift: Arc<Mutex<f64>>,
    pub date: Arc<Mutex<String>>,
    pub current_date: Arc<Mutex<String>>,
    pub list_init: Arc<Mutex<bool>>,
}

impl PlayoutStatus {
    pub fn new() -> Self {
        Self {
            time_shift: Arc::new(Mutex::new(0.0)),
            date: Arc::new(Mutex::new(String::new())),
            current_date: Arc::new(Mutex::new(String::new())),
            list_init: Arc::new(Mutex::new(true)),
        }
    }
}

