use std::{
    io::{Read, Write},
    process::Command,
};

use tokio::time::Instant;

#[tokio::main]
async fn main() {
    let mut ffmpeg_in = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostats",
            "-v",
            "level+error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=1920x1080:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000",
            "-c:v",
            "rawvideo",
            "-c:a",
            "pcm_s16le",
            "-t",
            "10",
            "-f",
            "nut",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let mut stdout = ffmpeg_in.stdout.take().expect("failed to open stdout");

    let mut ffmpeg_out = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostats",
            "-v",
            "level+error",
            "-i",
            "pipe:0",
            "-c:v",
            "copy",
            "-t",
            "10",
            "-c:a",
            "copy",
            "-f",
            "null",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap();

    let mut writer = ffmpeg_out.stdin.take().expect("failed to open stdin");

    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0;

    println!("⏳ Streaming...");

    let start = Instant::now();

    loop {
        let n = stdout.read(&mut buffer[..]).unwrap();
        if n == 0 {
            break;
        }

        writer.write_all(&buffer[..n]).unwrap();
        total += n;
    }

    let duration = start.elapsed();

    writer.flush().unwrap();
    println!(
        "✅ Transferred {} bytes (~{:.2} MB) in {:.2} seconds",
        total,
        total as f64 / 1024.0 / 1024.0,
        duration.as_secs_f64()
    );

    ffmpeg_in.wait().unwrap();
    ffmpeg_out.kill().unwrap();
    ffmpeg_out.wait().unwrap();
}
