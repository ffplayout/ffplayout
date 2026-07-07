use std::{
    collections::HashSet,
    fmt,
    sync::{Arc, Mutex, PoisonError},
};

use local_ip_address::list_afinet_netifas;
use serde::Serialize;
use sysinfo::{Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System, get_current_pid};

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
    info_disks: Arc<Mutex<Disks>>,
    #[serde(skip_serializing)]
    info_net: Arc<Mutex<Networks>>,
    #[serde(skip_serializing)]
    info_sys: Arc<Mutex<System>>,
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
    pub used: u64,
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
    pub free: u64,
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
            name: System::name(),
            kernel: System::kernel_version(),
            version: System::os_version(),
            ffp_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        };

        Self {
            info_disks: Arc::new(Mutex::new(Disks::new_with_refreshed_list())),
            info_net: Arc::new(Mutex::new(Networks::new_with_refreshed_list())),
            info_sys: Arc::new(Mutex::new(System::new_all())),
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

                disks.refresh(true);
                networks.refresh(true);
                sys.refresh_cpu_usage();
                sys.refresh_memory();

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
                    usage += cpu.cpu_usage();
                }

                let cpu = Cpu {
                    cores,
                    usage: usage * cores / 100.0,
                };

                let mut storage = Storage::default();

                for disk in &*disks {
                    if disk.mount_point().to_string_lossy().len() > 1
                        && storage_path.starts_with(disk.mount_point())
                    {
                        storage.path = disk.name().to_string_lossy().to_string();
                        storage.total = disk.total_space();
                        storage.used = disk.available_space();
                    }
                }

                let load_avg = System::load_average();
                let load = Load {
                    one: load_avg.one,
                    five: load_avg.five,
                    fifteen: load_avg.fifteen,
                };

                let memory = Memory {
                    total: sys.total_memory(),
                    used: sys.used_memory(),
                    free: sys.total_memory() - sys.used_memory(),
                };

                let mut network = Network::default();

                for (interface_name, data) in &*networks {
                    if !interfaces.is_empty() && interface_name == interfaces[0].0 {
                        network.name.clone_from(interface_name);
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
