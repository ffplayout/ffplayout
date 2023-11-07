use std::cmp;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use local_ip_address::list_afinet_netifas;
use sysinfo::{CpuExt, DiskExt, NetworkExt, System, SystemExt};

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

#[derive(Debug)]
struct Cpu {
    cores: f32,
    usage: f32,
}

#[derive(Debug, Default)]
struct Disk {
    name: String,
    total: String,
    used: String,
}

#[derive(Debug)]
struct Load {
    one: f64,
    five: f64,
    fifteen: f64,
}

#[derive(Debug)]
struct Memory {
    total: String,
    used: String,
    free: String,
}

#[derive(Debug, Default)]
struct Network {
    name: String,
    current_in: String,
    total_in: String,
    current_out: String,
    total_out: String,
}

#[derive(Debug)]
struct MySystem {
    name: Option<String>,
    kernel: Option<String>,
    version: Option<String>,
}

#[derive(Debug)]
struct Swap {
    total: String,
    used: String,
    free: String,
}

#[derive(Debug)]
struct SystemStat {
    cpu: Cpu,
    disk: Disk,
    load: Load,
    memory: Memory,
    network: Network,
    swap: Swap,
    system: MySystem,
}

fn main() {
    let network_interfaces = list_afinet_netifas().unwrap();
    let mut sys = System::new_all();

    let mut interfaces = vec![];

    for (name, ip) in network_interfaces.iter() {
        if !ip.is_loopback() {
            interfaces.push((name, ip))
        }
    }

    if interfaces.len() > 1 {
        interfaces = interfaces
            .into_iter()
            .filter(|i| i.1.is_ipv4())
            .collect::<_>();
    }

    loop {
        sys.refresh_all();

        let mut usage = 0.0;
        let cores = sys.cpus().len() as f32;

        for cpu in sys.cpus() {
            usage += cpu.cpu_usage();
        }

        let my_cpu = Cpu {
            cores,
            usage: usage * cores / 100.0,
        };

        let mut my_disk = Disk::default();

        for disk in sys.disks() {
            if disk.mount_point().to_string_lossy().len() > 1
                && Path::new("/home/jb/Videos").starts_with(disk.mount_point())
            {
                my_disk.name = disk.name().to_string_lossy().to_string();
                my_disk.total = byte_convert(disk.total_space() as f64);
                my_disk.used = byte_convert(disk.available_space() as f64);
            }
        }

        let load_avg = sys.load_average();
        let my_load = Load {
            one: load_avg.one,
            five: load_avg.five,
            fifteen: load_avg.fifteen,
        };

        let my_memory = Memory {
            total: byte_convert(sys.total_memory() as f64),
            used: byte_convert(sys.used_memory() as f64),
            free: byte_convert((sys.total_memory() - sys.used_memory()) as f64),
        };

        let mut my_network = Network::default();

        for (interface_name, data) in sys.networks() {
            if !interfaces.is_empty() && interface_name == interfaces[0].0 {
                my_network.name = interface_name.clone();
                my_network.current_in = byte_convert(data.received() as f64);
                my_network.total_in = byte_convert(data.total_received() as f64);
                my_network.current_out = byte_convert(data.transmitted() as f64);
                my_network.total_out = byte_convert(data.total_transmitted() as f64);
            }
        }

        let my_swap = Swap {
            total: byte_convert(sys.total_swap() as f64),
            used: byte_convert(sys.used_swap() as f64),
            free: byte_convert((sys.total_swap() - sys.used_swap()) as f64),
        };

        let my_system = MySystem {
            name: sys.name(),
            kernel: sys.kernel_version(),
            version: sys.os_version(),
        };

        let system_stat = SystemStat {
            system: my_system,
            memory: my_memory,
            swap: my_swap,
            disk: my_disk,
            cpu: my_cpu,
            load: my_load,
            network: my_network,
        };

        print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
        println!("{system_stat:#?}");

        sleep(Duration::from_secs(1))
    }
}
