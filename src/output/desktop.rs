// use std::io::prelude::*;
use std::{
    io,
    process::{Command, Stdio},
};

use crate::utils::{program, sec_to_time, Config};

pub fn play(config: Config) -> io::Result<()> {
    let get_source = program(config.clone());
    let dec_settings = config.processing.settings.unwrap();

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
            // println!("Play: {:#?}", node);
            println!("Node begin: {:?}", sec_to_time(node.begin.unwrap()));
            let cmd = node.cmd.unwrap();
            let filter = node.filter.unwrap();

            let mut dec_cmd = vec![
                "-v",
                "level+error",
                "-hide_banner",
                "-nostats"
            ];

            dec_cmd.append(&mut cmd.iter().map(String::as_str).collect());
            dec_cmd.append(&mut filter.iter().map(String::as_str).collect());
            dec_cmd.append(&mut dec_settings.iter().map(String::as_str).collect());

            let mut dec_proc = Command::new("ffmpeg")
                .args(dec_cmd)
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
