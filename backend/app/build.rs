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
const RUN_P_BIN: &str = "../../node_modules/.bin/run-p.cmd";

#[cfg(all(not(debug_assertions), not(windows)))]
const RUN_P_BIN: &str = "../../node_modules/.bin/run-p";

#[cfg(not(debug_assertions))]
fn run_npm(args: &[&str]) {
    let output = Command::new(NPM_COMMAND)
        .args(args)
        .current_dir("../..")
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

#[cfg(windows)]
fn embed_windows_resources() {
    const ICON_PATH: &str = "../../frontend/public/favicon.ico";

    println!("cargo:rerun-if-changed={ICON_PATH}");
    winresource::WindowsResource::new()
        .set_icon(ICON_PATH)
        .compile()
        .expect("failed to embed Windows executable resources");
}

#[cfg(not(windows))]
fn embed_windows_resources() {}

fn main() {
    embed_windows_resources();

    #[cfg(not(debug_assertions))]
    {
        // These files live outside the Cargo package. Watch the frontend inputs
        // individually so changes rebuild the embedded assets without watching
        // frontend/dist, which this build script generates itself.
        for path in [
            "../../frontend/src",
            "../../frontend/public",
            "../../frontend/index.html",
            "../../frontend/env.d.ts",
            "../../frontend/tsconfig.app.json",
            "../../frontend/tsconfig.node.json",
            "../../tsconfig.json",
            "../../vite.config.ts",
            "../../package.json",
            "../../package-lock.json",
        ] {
            println!("cargo:rerun-if-changed={path}");
        }

        if !Path::new(RUN_P_BIN).exists() {
            info!("run-p not found, installing frontend dependencies with npm ci");
            run_npm(&["ci"]);
        }

        run_npm(&["run", "build"]);
    }

    println!("cargo:rerun-if-env-changed=FFPLAYOUT_VAAPI_SHARED");
    if matches!(std::env::var("FFPLAYOUT_VAAPI_SHARED").as_deref(), Ok("1")) {
        println!("cargo:rustc-link-lib=dylib=va-drm");
        println!("cargo:rustc-link-lib=dylib=va");
        println!("cargo:rustc-link-lib=dylib=drm");
    }
}
