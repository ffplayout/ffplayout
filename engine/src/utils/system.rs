use std::fmt;

use local_ip_address::list_afinet_netifas;
use serde::Serialize;
use sysinfo::System;

use crate::utils::config::PlayoutConfig;
use crate::{DISKS, NETWORKS, SYS};

const IGNORE_INTERFACES: [&str; 7] = ["docker", "lxdbr", "tab", "tun", "virbr", "veth", "vnet"];

#[derive(Debug, Serialize)]
pub struct Cpu {
    pub cores: f32,
    pub usage: f32,
}

#[derive(Debug, Default, Serialize)]
pub struct Storage {
    pub path: String,
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Serialize)]
pub struct Load {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Serialize)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct Network {
    pub name: String,
    pub current_in: u64,
    pub total_in: u64,
    pub current_out: u64,
    pub total_out: u64,
}

#[derive(Debug, Serialize)]
pub struct MySystem {
    pub name: Option<String>,
    pub kernel: Option<String>,
    pub version: Option<String>,
    pub ffp_version: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Swap {
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

#[derive(Debug, Serialize)]
pub struct SystemStat {
    pub cpu: Cpu,
    pub load: Load,
    pub memory: Memory,
    pub network: Network,
    pub storage: Storage,
    pub swap: Swap,
    pub system: MySystem,
}

impl fmt::Display for SystemStat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}

pub fn stat(config: &PlayoutConfig) -> SystemStat {
    let mut disks = DISKS.lock().unwrap();
    let mut networks = NETWORKS.lock().unwrap();
    let mut sys = SYS.lock().unwrap();

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

    disks.refresh(true);
    networks.refresh(true);
    sys.refresh_cpu_usage();
    sys.refresh_memory();

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
            && config.channel.storage.starts_with(disk.mount_point())
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

    let system = MySystem {
        name: System::name(),
        kernel: System::kernel_version(),
        version: System::os_version(),
        ffp_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    SystemStat {
        cpu,
        load,
        memory,
        network,
        storage,
        swap,
        system,
    }
}
