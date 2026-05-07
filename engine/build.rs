#[cfg(not(debug_assertions))]
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};

#[cfg(not(debug_assertions))]
use build_print::info;

#[cfg(all(not(debug_assertions), windows))]
const NPM_COMMAND: &str = "npm.cmd";

#[cfg(all(not(debug_assertions), not(windows)))]
const NPM_COMMAND: &str = "npm";

#[cfg(all(not(debug_assertions), windows))]
const RUN_P_BIN: &str = "../frontend/node_modules/.bin/run-p.cmd";

#[cfg(all(not(debug_assertions), not(windows)))]
const RUN_P_BIN: &str = "../frontend/node_modules/.bin/run-p";

#[cfg(not(debug_assertions))]
fn run_npm(args: &[&str]) {
    let output = Command::new(NPM_COMMAND)
        .args(args)
        .current_dir("../frontend")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .and_then(|mut child| {
            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = line?;
                info!("{}", line.trim());
            }
            child.wait_with_output()
        })
        .expect("Failed to execute command");

    if !output.status.success() {
        panic!("Command executed with failing error code");
    }
}

fn main() {
    #[cfg(not(debug_assertions))]
    {
        if !Path::new(RUN_P_BIN).exists() {
            info!("run-p not found, installing frontend dependencies with npm ci");
            run_npm(&["ci"]);
        }

        run_npm(&["run", "build"]);
    }
}
