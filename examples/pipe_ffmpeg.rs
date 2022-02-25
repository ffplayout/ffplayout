use std::{
    io::{prelude::*, Read},
    process::{Command, Stdio},
    thread::sleep,
    time::Duration,
};

fn main() {
    let mut enc_proc = match Command::new("ffplay")
        .args(["-v", "error", "-hide_banner", "-nostats", "-i", "pipe:0"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => panic!("couldn't spawn ffplay: {}", e),
        Ok(proc) => proc,
    };

    let mut buffer: [u8; 65424] = [0; 65424];

    let mut dec_proc = match Command::new("ffmpeg")
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=6:size=1280x720:rate=25",
            "-f",
            "lavfi",
            "-i",
            "anoisesrc=d=6:c=pink:r=48000:a=0.5",
            "-pix_fmt",
            "yuv420p",
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
    {
        Err(e) => panic!("couldn't spawn ffmpeg: {}", e),
        Ok(proc) => proc,
    };

    let mut enc_writer = enc_proc.stdin.as_ref().unwrap();
    let dec_reader = dec_proc.stdout.as_mut().unwrap();

    loop {
        let bytes_len = match dec_reader.read(&mut buffer[..]) {
            Ok(length) => length,
            Err(e) => panic!("Reading error from decoder: {:?}", e)
        };

        match enc_writer.write(&buffer[..bytes_len]) {
            Ok(_) => (),
            Err(e) => panic!("Err: {:?}", e),
        };

        if bytes_len == 0 {
            break;
        }
    }

    match dec_proc.wait() {
        Ok(_) => println!("decoding done..."),
        Err(e) => panic!("Enc error: {:?}", e),
    }

    sleep(Duration::from_secs(1));

    match enc_proc.kill() {
        Ok(_) => println!("Playout done..."),
        Err(e) => panic!("Enc error: {:?}", e),
    }
}
