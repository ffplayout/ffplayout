use std::cmp;

use local_ip_address::list_afinet_netifas;
use serde::Serialize;
use sysinfo::{CpuExt, DiskExt, NetworkExt, System, SystemExt};

use ffplayout_lib::utils::PlayoutConfig;

pub fn byte_convert(num: f64) -> String {
    let negative = if num.is_sign_positive() { "" } else { "-" };
    let num = num.abs();
    let units = ["B", "kiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    if num < 1_f64 {
        return format!("{negative}{num} B");
    }
    let delimiter = 1024_f64;
    let exponent = cmp::min(
        (num.ln() / delimiter.ln()).floor() as i32,
        (units.len() - 1) as i32,
    );
    let pretty_bytes = format!("{:.3}", num / delimiter.powi(exponent))
        .parse::<f64>()
        .unwrap()
        * 1_f64;
    let unit = units[exponent as usize];
    format!("{negative}{pretty_bytes} {unit}")
}

#[derive(Debug, Serialize)]
pub struct Cpu {
    pub cores: f32,
    pub usage: f32,
}

#[derive(Debug, Default, Serialize)]
pub struct Storage {
    pub path: String,
    pub total: String,
    pub used: String,
}

#[derive(Debug, Serialize)]
pub struct Load {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Serialize)]
pub struct Memory {
    pub total: String,
    pub used: String,
    pub free: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Network {
    pub name: String,
    pub current_in: String,
    pub total_in: String,
    pub current_out: String,
    pub total_out: String,
}

#[derive(Debug, Serialize)]
pub struct MySystem {
    pub name: Option<String>,
    pub kernel: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Swap {
    pub total: String,
    pub used: String,
    pub free: String,
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

pub fn stat(config: PlayoutConfig) -> SystemStat {
    let network_interfaces = list_afinet_netifas().unwrap();
    let mut sys = System::new_all();
    let mut usage = 0.0;
    let mut interfaces = vec![];

    for (name, ip) in network_interfaces.iter() {
        if !ip.is_loopback() {
            interfaces.push((name, ip))
        }
    }

    interfaces.dedup_by(|a, b| a.0 == b.0);

    sys.refresh_all();

    let cores = sys.cpus().len() as f32;

    for cpu in sys.cpus() {
        usage += cpu.cpu_usage();
    }

    let cpu = Cpu {
        cores,
        usage: usage * cores / 100.0,
    };

    let mut storage = Storage::default();

    for disk in sys.disks() {
        if disk.mount_point().to_string_lossy().len() > 1
            && config.storage.path.starts_with(disk.mount_point())
        {
            storage.path = disk.name().to_string_lossy().to_string();
            storage.total = byte_convert(disk.total_space() as f64);
            storage.used = byte_convert(disk.available_space() as f64);
        }
    }

    let load_avg = sys.load_average();
    let load = Load {
        one: load_avg.one,
        five: load_avg.five,
        fifteen: load_avg.fifteen,
    };

    let memory = Memory {
        total: byte_convert(sys.total_memory() as f64),
        used: byte_convert(sys.used_memory() as f64),
        free: byte_convert((sys.total_memory() - sys.used_memory()) as f64),
    };

    let mut network = Network::default();

    for (interface_name, data) in sys.networks() {
        if !interfaces.is_empty() && interface_name == interfaces[0].0 {
            network.name = interface_name.clone();
            network.current_in = byte_convert(data.received() as f64);
            network.total_in = byte_convert(data.total_received() as f64);
            network.current_out = byte_convert(data.transmitted() as f64);
            network.total_out = byte_convert(data.total_transmitted() as f64);
        }
    }

    let swap = Swap {
        total: byte_convert(sys.total_swap() as f64),
        used: byte_convert(sys.used_swap() as f64),
        free: byte_convert((sys.total_swap() - sys.used_swap()) as f64),
    };

    let system = MySystem {
        name: sys.name(),
        kernel: sys.kernel_version(),
        version: sys.os_version(),
    };

    SystemStat {
        cpu,
        storage,
        load,
        memory,
        network,
        system,
        swap,
    }
}
