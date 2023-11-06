use std::cmp;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use sysinfo::{CpuExt, DiskExt, NetworkExt, System, SystemExt};

pub fn byte_convert(num: f64) -> String {
    let negative = if num.is_sign_positive() { "" } else { "-" };
    let num = num.abs();
    let units = ["B", "kiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    if num < 1_f64 {
        return format!("{}{} {}", negative, num, "B");
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
    format!("{}{} {}", negative, pretty_bytes, unit)
}

fn main() {
    let mut sys = System::new_all();

    loop {
        sys.refresh_all();

        let mut usage = 0.0;
        let count = sys.cpus().len() as f32;

        for cpu in sys.cpus() {
            usage += cpu.cpu_usage();
        }

        print!("{esc}[2J{esc}[1;1H", esc = 27 as char);

        println!("System name:             {:?}", sys.name());
        println!("System kernel version:   {:?}", sys.kernel_version());
        println!("System OS version:       {:?}\n", sys.os_version());

        println!("total memory: {}", byte_convert(sys.total_memory() as f64));
        println!("used memory : {}", byte_convert(sys.used_memory() as f64));
        println!(
            "free memory : {}\n",
            byte_convert((sys.total_memory() - sys.used_memory()) as f64)
        );

        println!("total swap  : {}", byte_convert(sys.total_swap() as f64));
        println!("used swap   : {}", byte_convert(sys.used_swap() as f64));
        println!(
            "free swap   : {}\n",
            byte_convert((sys.total_swap() - sys.used_swap()) as f64)
        );

        for disk in sys.disks() {
            if disk.mount_point() == Path::new("/") {
                println!(
                    "disk: {:?} | available space {}",
                    disk.name(),
                    byte_convert(disk.available_space() as f64),
                );
            }
        }
        println!();

        let load_avg = sys.load_average();

        for (interface_name, data) in sys.networks() {
            if interface_name == "wlp5s0" {
                println!(
                    "{}: (in / total) {} / {} | (out / total) {} / {}",
                    interface_name,
                    byte_convert(data.received() as f64),
                    byte_convert(data.total_received() as f64),
                    byte_convert(data.transmitted() as f64),
                    byte_convert(data.total_transmitted() as f64),
                );
            }
        }

        println!();

        println!("CPU Usage: {:.2}%", usage * count / 100.0);
        println!(
            "Load:      {}% {}% {}%\n",
            load_avg.one, load_avg.five, load_avg.fifteen
        );

        sleep(Duration::from_secs(1))
    }
}
