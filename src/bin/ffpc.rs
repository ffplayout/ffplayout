use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use itertools::Itertools;

use horust::horust::ExitStatus;
use horust::horust::HorustConfig;
use horust::Horust;

#[derive(Parser, Debug)]
#[clap(version,
    name = "ffpc",
    version = "0.1.0",
    about = "ffplayout process control",
    long_about = None)]
pub struct Args {
    #[clap(
        short,
        long,
        help = "File path to ffpc.toml",
        default_value = "/etc/ffplayout/ffpc.toml"
    )]
    config_path: PathBuf,

    #[clap(
        short,
        long = "services-path",
        help = "Play folder content",
        default_value = "/etc/ffplayout/services"
    )]
    services_paths: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let opts = Args::parse();

    let horust_cfg = HorustConfig {
        unsuccessful_exit_finished_failed: true,
    };

    let config =
        HorustConfig::load_and_merge(&horust_cfg, &opts.config_path).with_context(|| {
            format!(
                "Failed loading configuration: {}",
                &opts.config_path.display()
            )
        })?;

    println!(
        "Loading services from {}",
        display_directories(&opts.services_paths)
    );

    let mut horust = {
        Horust::from_services_dirs(&opts.services_paths).with_context(|| {
            format!(
                "Failed loading services from {}",
                display_directories(&opts.services_paths)
            )
        })?
    };

    if let ExitStatus::SomeServiceFailed = horust.run() {
        if config.unsuccessful_exit_finished_failed {
            println!("Some processes have failed.");
            std::process::exit(101);
        }
    }
    Ok(())
}

fn display_directories(dirs: &[PathBuf]) -> String {
    match dirs.len() {
        1 => format!("directory: {}", dirs.first().unwrap().display()),
        _ => format!(
            "directories:\n{}",
            dirs.iter().map(|d| format!("* {}", d.display())).join("\n")
        ),
    }
}
