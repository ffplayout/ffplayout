use std::{
    collections::HashSet,
    ffi::OsStr,
    fmt,
    path::Path,
    sync::{Arc, Mutex, PoisonError},
};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use local_ip_address::list_afinet_netifas;
use serde::Serialize;
use sysinfo::{
    Disk, Disks, Networks, Process, ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid,
};

use crate::utils::{config::PlayoutConfig, sizeof_fmt};

const IGNORE_INTERFACES: [&str; 7] = ["docker", "lxdbr", "tab", "tun", "virbr", "veth", "vnet"];

#[derive(Clone, Debug, Default, Serialize)]
pub struct SystemStat {
    pub cpu: Cpu,
    pub load: Load,
    pub memory: Memory,
    pub network: Network,
    pub storage: Storage,
    pub swap: Swap,
    pub system: MySystem,
    #[serde(skip_serializing)]
    info_disks: Arc<Mutex<Option<Disks>>>,
    #[serde(skip_serializing)]
    info_net: Arc<Mutex<Option<Networks>>>,
    #[serde(skip_serializing)]
    info_sys: Arc<Mutex<Option<System>>>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Cpu {
    pub cores: f32,
    pub usage: f32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Storage {
    pub path: String,
    pub total: u64,
    pub free: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Load {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    pub process: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Network {
    pub name: String,
    pub current_in: u64,
    pub total_in: u64,
    pub current_out: u64,
    pub total_out: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MySystem {
    pub name: Option<String>,
    pub kernel: Option<String>,
    pub version: Option<String>,
    pub ffp_version: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Swap {
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

impl SystemStat {
    pub fn new() -> Self {
        let system = MySystem {
            name: System::name().ok(),
            kernel: System::kernel_version().ok(),
            version: System::os_version().ok(),
            ffp_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        };

        Self {
            info_disks: Arc::new(Mutex::new(initialize_collector(
                "disks",
                Disks::new_with_refreshed_list(),
            ))),
            info_net: Arc::new(Mutex::new(initialize_collector(
                "networks",
                Networks::new_with_refreshed_list(),
            ))),
            info_sys: Arc::new(Mutex::new(initialize_collector(
                "system",
                System::new_all(),
            ))),
            system,
            ..Default::default()
        }
    }

    pub async fn process_snapshot(&self) -> (usize, String) {
        let Ok(pid) = get_current_pid() else {
            return (0, String::from("0.0"));
        };

        let info_sys = Arc::clone(&self.info_sys);

        tokio::task::spawn_blocking(move || {
            let mut sys = info_sys.lock().unwrap_or_else(PoisonError::into_inner);

            let Some(sys) = sys.as_mut() else {
                return (0, String::from("0.0"));
            };

            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                false,
                ProcessRefreshKind::nothing().with_tasks().with_memory(),
            );

            if let Some(process) = sys.process(pid) {
                let threads = process.tasks().map_or(0, HashSet::len);
                let rss = sizeof_fmt(process.memory() as f64);

                (threads, rss)
            } else {
                (0, String::from("0.0"))
            }
        })
        .await
        .unwrap_or_else(|_| (0, String::from("0.0")))
    }

    pub async fn stat(&self, config: &PlayoutConfig) -> Self {
        let info_disks = Arc::clone(&self.info_disks);
        let info_net = Arc::clone(&self.info_net);
        let info_sys = Arc::clone(&self.info_sys);
        let storage_path = config.channel.storage.clone();
        let system = self.system.clone();

        let Ok((cpu, load, memory, network, storage, swap)) =
            tokio::task::spawn_blocking(move || {
                let mut disks = info_disks.lock().unwrap_or_else(PoisonError::into_inner);
                let mut networks = info_net.lock().unwrap_or_else(PoisonError::into_inner);
                let mut sys = info_sys.lock().unwrap_or_else(PoisonError::into_inner);

                if let Some(disks) = disks.as_mut() {
                    disks.refresh(true);
                }
                if let Some(networks) = networks.as_mut() {
                    networks.refresh(true);
                }
                let Some(sys) = sys.as_mut() else {
                    return (
                        Cpu::default(),
                        Load::default(),
                        Memory::default(),
                        Network::default(),
                        Storage::default(),
                        Swap::default(),
                    );
                };

                sys.refresh_cpu_usage();
                sys.refresh_memory();

                let process_memory = get_current_pid()
                    .ok()
                    .and_then(|pid| {
                        sys.refresh_processes_specifics(
                            ProcessesToUpdate::Some(&[pid]),
                            false,
                            ProcessRefreshKind::nothing().with_memory(),
                        );

                        sys.process(pid).map(Process::memory)
                    })
                    .unwrap_or_default();

                let network_interfaces = list_afinet_netifas().unwrap_or_default();
                let mut usage = 0.0;
                let mut interfaces = vec![];

                for (name, ip) in &network_interfaces {
                    if !ip.is_loopback()
                        && !IGNORE_INTERFACES
                            .iter()
                            .any(|&prefix| name.starts_with(prefix))
                    {
                        interfaces.push((name, ip));
                    }
                }

                interfaces.dedup_by(|a, b| a.0 == b.0);

                let cores = sys.cpus().len() as f32;

                for cpu in sys.cpus() {
                    usage += cpu.usage();
                }

                let cpu = Cpu {
                    cores,
                    usage: usage * cores / 100.0,
                };

                let storage = storage_for_path(&storage_path, disks.as_deref().unwrap_or_default());

                let load = System::load_average()
                    .map(|load| Load {
                        one: load.one,
                        five: load.five,
                        fifteen: load.fifteen,
                    })
                    .unwrap_or_default();

                let memory = Memory {
                    total: sys.total_memory(),
                    used: sys.used_memory(),
                    process: process_memory,
                };

                let mut network = Network::default();

                for (interface_name, data) in networks.iter().flatten() {
                    if !interfaces.is_empty() && interface_name == OsStr::new(interfaces[0].0) {
                        network.name = interface_name.to_string_lossy().into_owned();
                        network.current_in = data.received();
                        network.total_in = data.total_received();
                        network.current_out = data.transmitted();
                        network.total_out = data.total_transmitted();
                    }
                }

                let swap = Swap {
                    total: sys.total_swap(),
                    used: sys.used_swap(),
                    free: sys.free_swap(),
                };

                (cpu, load, memory, network, storage, swap)
            })
            .await
        else {
            return Self {
                system,
                ..Default::default()
            };
        };

        Self {
            cpu,
            load,
            memory,
            network,
            storage,
            swap,
            system,
            ..Default::default()
        }
    }
}

impl fmt::Display for SystemStat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}

fn storage_for_path(path: &Path, disks: &[Disk]) -> Storage {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    // macOS firmlinks (e.g. /Users -> the Data volume) are not resolved by
    // canonicalize. The filesystem's device ID identifies the actual volume.
    #[cfg(unix)]
    let device = path.metadata().ok().map(|metadata| metadata.dev());
    #[cfg(not(unix))]
    let device: Option<u64> = None;

    let same_device = |disk: &Disk| device.is_some_and(|id| disk.id() == Some(id));
    let disk = disks
        .iter()
        .filter(|disk| same_device(disk) || path.starts_with(disk.mount_point()))
        .max_by_key(|disk| (same_device(disk), disk.mount_point().components().count()));

    disk.map_or_else(Storage::default, |disk| Storage {
        path: disk.name().to_string_lossy().into_owned(),
        total: disk.total_space(),
        free: disk.available_space(),
    })
}

fn initialize_collector<T>(name: &str, result: Result<T, sysinfo::Error>) -> Option<T> {
    match result {
        Ok(collector) => Some(collector),
        Err(error) => {
            log::warn!("System statistics: failed to initialize {name}: {error}");

            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn storage_uses_the_filesystem_device_for_relative_and_symlinked_paths() {
        use std::{fs, os::unix::fs::symlink};

        let disks = Disks::new_with_refreshed_list().unwrap();
        let directory = std::env::temp_dir().join(format!(
            "system-storage-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&directory).unwrap();
        let link = directory.join("media");
        symlink(std::env::current_dir().unwrap(), &link).unwrap();

        for path in [Path::new("."), link.as_path()] {
            let device = path.metadata().unwrap().dev();
            let disk = disks.iter().find(|disk| disk.id() == Some(device)).unwrap();
            let storage = storage_for_path(path, &disks);
            assert_eq!(storage.path, disk.name().to_string_lossy());
            assert_eq!(storage.total, disk.total_space());
            assert!(storage.total > 0);
            // available_space() can query live filesystem state; concurrent
            // writes may change it between these two observations.
            assert!(storage.free <= storage.total);
        }

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn storage_reports_the_root_filesystem() {
        let disks = Disks::new_with_refreshed_list().unwrap();
        let root = std::env::current_dir()
            .unwrap()
            .ancestors()
            .last()
            .unwrap()
            .to_path_buf();
        let storage = storage_for_path(&root, &disks);
        assert!(storage.total > 0);
        assert!(!storage.path.is_empty());
    }
}
