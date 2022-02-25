use std::{
    io::{prelude::*, Read},
    process::{Command, Stdio},
    thread::sleep,
    time::Duration,
};

use crate::utils::{Config, CurrentProgram, Messenger};

pub fn play(msg: Messenger, config: Config) {
    let get_source = CurrentProgram::new(&msg, config.clone());
    let dec_settings = config.processing.settings.unwrap();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level);
    let mut enc_cmd = vec![
        "-hide_banner",
        "-nostats",
        "-v",
        ff_log_format.as_str(),
        "-i",
        "pipe:0",
    ];

    let mut enc_filter: Vec<String> = vec![];
    let mut buffer: [u8; 65424] = [0; 65424];

    if config.text.add_text && !config.text.over_pre {
        let text_filter: String = format!(
            "null,zmq=b=tcp\\\\://'{}',drawtext=text='':fontfile='{}'",
            config.text.bind_address.replace(":", "\\:"),
            config.text.fontfile
        );

        enc_filter = vec!["-vf".to_string(), text_filter];
    }

    enc_cmd.append(&mut enc_filter.iter().map(String::as_str).collect());

    msg.debug(format!("Encoder CMD: <bright-blue>{:?}</>", enc_cmd));

    let mut enc_proc = match Command::new("ffplay")
        .args(enc_cmd)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => {
            msg.error(format!("couldn't spawn encoder process: {}", e));
            panic!("couldn't spawn encoder process: {}", e)
        },
        Ok(proc) => proc,
    };

    for node in get_source {
        // println!("Node begin: {:?}", sec_to_time(node.begin.unwrap()));
        msg.info(format!("Play: <b><magenta>{}</></b>", node.source));

        let cmd = node.cmd.unwrap();
        let filter = node.filter.unwrap();

        let mut dec_cmd = vec![
            "-v",
            ff_log_format.as_str(),
            "-hide_banner",
            "-nostats",
        ];

        dec_cmd.append(&mut cmd.iter().map(String::as_str).collect());

        if filter.len() > 1 {
            dec_cmd.append(&mut filter.iter().map(String::as_str).collect());
        }

        dec_cmd.append(&mut dec_settings.iter().map(String::as_str).collect());
        msg.debug(format!("Decoder CMD: <bright-blue>{:?}</>", dec_cmd));

        let mut dec_proc = match Command::new("ffmpeg")
            .args(dec_cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                msg.error(format!("couldn't spawn decoder process: {}", e));
                panic!("couldn't spawn decoder process: {}", e)
            },
            Ok(proc) => proc,
        };

        let mut enc_writer = enc_proc.stdin.as_ref().unwrap();
        let dec_reader = dec_proc.stdout.as_mut().unwrap();

        loop {
            let dec_bytes_len = match dec_reader.read(&mut buffer[..]) {
                Ok(length) => length,
                Err(e) => panic!("Reading error from decoder: {:?}", e)
            };

            match enc_writer.write(&buffer[..dec_bytes_len]) {
                Ok(_) => (),
                Err(e) => panic!("Err: {:?}", e),
            };

            if dec_bytes_len == 0 {
                break;
            }
        }

        match dec_proc.wait() {
            Ok(_) => msg.debug("decoding done...".into()),
            Err(e) => panic!("Enc error: {:?}", e),
        }
    }

    sleep(Duration::from_secs(1));

    match enc_proc.kill() {
        Ok(_) => println!("Playout done..."),
        Err(e) => panic!("Enc error: {:?}", e),
    }
}
