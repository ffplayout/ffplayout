// use std::io::prelude::*;
use std::{
    io,
    process::{Command, Stdio},
};

use crate::utils::program;

pub fn play(_settings: Option<Vec<String>>) -> io::Result<()> {
    let get_source = program();

    let mut enc_proc = Command::new("ffplay")
        .args([
            "-hide_banner",
            "-nostats",
            "-v",
            "level+error",
            "-i",
            "pipe:0",
        ])
        .stdin(Stdio::piped())
        // .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // let mut stdin = enc_proc.stdin.unwrap();
    // let mut buffer = vec![0; 65376];

    if let Some(mut enc_input) = enc_proc.stdin.take() {
         for node in get_source {
            println!("Play: {}", node.source);

            let mut dec_proc = Command::new("ffmpeg")
                .args([
                    "-v",
                    "level+error",
                    "-hide_banner",
                    "-nostats",
                    "-i",
                    &node.source,
                    "-pix_fmt",
                    "yuv420p",
                    "-r",
                    "25",
                    "-c:v",
                    "mpeg2video",
                    "-g",
                    "1",
                    "-b:v",
                    "50000k",
                    "-minrate",
                    "50000k",
                    "-maxrate",
                    "50000k",
                    "-bufsize",
                    "25000k",
                    "-c:a",
                    "s302m",
                    "-strict",
                    "-2",
                    "-ar",
                    "48000",
                    "-ac",
                    "2",
                    "-f",
                    "mpegts",
                    "-",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();

            if let Some(mut dec_output) = dec_proc.stdout.take() {
                io::copy(&mut dec_output, &mut enc_input).expect("Write to streaming pipe failed!");

                dec_proc.wait()?;
                let dec_output = dec_proc.wait_with_output()?;

                if dec_output.stderr.len() > 0 {
                    println!("[Encoder] {}", String::from_utf8(dec_output.stderr).unwrap());
                }
            }
        }

        enc_proc.wait()?;
        let enc_output = enc_proc.wait_with_output()?;
        println!("[Encoder] {}", String::from_utf8(enc_output.stderr).unwrap());
    }

    Ok(())
}
