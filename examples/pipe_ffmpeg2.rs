use std::{
    io::{prelude::*, BufReader, Error, Read},
    process::{Command, Stdio},
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Mutex,
    },
    thread::sleep,
    time::Duration,
};

use process_control::{ChildExt, Terminator};
use tokio::runtime::Runtime;

async fn ingest_server(
    dec_setting: Vec<&str>,
    ingest_sender: SyncSender<(usize, [u8; 65088])>,
    proc_terminator: Arc<Mutex<Option<Terminator>>>,
    is_terminated: Arc<Mutex<bool>>,
    server_is_running: Arc<Mutex<bool>>,
) -> Result<(), Error> {
    let mut buffer: [u8; 65088] = [0; 65088];
    let filter = "[0:v]fps=25,scale=1024:576,setdar=dar=1.778[vout1]";
    let mut filter_list = vec!["-filter_complex", &filter, "-map", "[vout1]", "-map", "0:a"];
    let mut server_cmd = vec!["-hide_banner", "-nostats", "-v", "error"];

    let mut stream_input = vec![
        "-f",
        "live_flv",
        "-listen",
        "1",
        "-i",
        "rtmp://localhost:1936/live/stream",
    ];

    server_cmd.append(&mut stream_input);
    server_cmd.append(&mut filter_list);
    server_cmd.append(&mut dec_setting.clone());

    let mut is_running;

    loop {
        if *is_terminated.lock().unwrap() {
            break;
        }

        let mut server_proc = match Command::new("ffmpeg")
            .args(server_cmd.clone())
            .stdout(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                panic!("couldn't spawn ingest server: {}", e)
            }
            Ok(proc) => proc,
        };

        let serv_terminator = server_proc.terminator()?;
        *proc_terminator.lock().unwrap() = Some(serv_terminator);
        let ingest_reader = server_proc.stdout.as_mut().unwrap();
        is_running = false;

        loop {
            let bytes_len = match ingest_reader.read(&mut buffer[..]) {
                Ok(length) => length,
                Err(e) => {
                    println!("Reading error from ingest server: {:?}", e);

                    break;
                }
            };

            if !is_running {
                *server_is_running.lock().unwrap() = true;
                is_running = true;
            }

            if bytes_len > 0 {
                if let Err(e) = ingest_sender.send((bytes_len, buffer)) {
                    println!("Ingest server write error: {:?}", e);

                    *is_terminated.lock().unwrap() = true;
                    break;
                }
            } else {
                break;
            }
        }

        *server_is_running.lock().unwrap() = false;

        sleep(Duration::from_secs(1));

        if let Err(e) = server_proc.kill() {
            print!("Ingest server {:?}", e)
        };

        if let Err(e) = server_proc.wait() {
            panic!("Decoder error: {:?}", e)
        };
    }

    Ok(())
}
fn main() {
    let server_term: Arc<Mutex<Option<Terminator>>> = Arc::new(Mutex::new(None));
    let is_terminated: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let server_is_running: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    let dec_setting: Vec<&str> = vec![
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
    ];

    let mut player_proc = match Command::new("ffplay")
        .args(["-v", "error", "-hide_banner", "-nostats", "-i", "pipe:0"])
        .stdin(Stdio::piped())
        .spawn()
    {
        Err(e) => panic!("couldn't spawn ffplay: {}", e),
        Ok(proc) => proc,
    };

    let (ingest_sender, ingest_receiver): (
        SyncSender<(usize, [u8; 65088])>,
        Receiver<(usize, [u8; 65088])>,
    ) = sync_channel(1);
    let runtime = Runtime::new().unwrap();

    runtime.spawn(ingest_server(
        dec_setting.clone(),
        ingest_sender,
        server_term.clone(),
        is_terminated.clone(),
        server_is_running.clone(),
    ));

    let mut buffer: [u8; 65088] = [0; 65088];

    let mut dec_cmd = vec![
        "-v",
        "error",
        "-hide_banner",
        "-nostats",
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=120:size=1024x576:rate=25",
        "-f",
        "lavfi",
        "-i",
        "anoisesrc=d=120:c=pink:r=48000:a=0.5",
    ];

    dec_cmd.append(&mut dec_setting.clone());

    let mut dec_proc = match Command::new("ffmpeg")
        .args(dec_cmd)
        .stdout(Stdio::piped())
        .spawn()
    {
        Err(e) => panic!("couldn't spawn ffmpeg: {}", e),
        Ok(proc) => proc,
    };

    let mut player_writer = player_proc.stdin.as_ref().unwrap();
    let mut dec_reader = BufReader::new(dec_proc.stdout.take().unwrap());

    let mut live_on = false;

    let mut count = 0;

    loop {
        count += 1;

        if *server_is_running.lock().unwrap() {
            if let Ok(receive) = ingest_receiver.try_recv() {
                if let Err(e) = player_writer.write(&receive.1[..receive.0]) {
                    println!("Ingest receiver error: {:?}", e);

                    break;
                };
            }

            if !live_on {
                println!("Switch from offline source to live");

                live_on = true;
            }
        } else {
            println!("{count}");
            let dec_bytes_len = match dec_reader.read(&mut buffer[..]) {
                Ok(length) => length,
                Err(e) => {
                    println!("Reading error from decoder: {:?}", e);

                    break;
                }
            };

            if dec_bytes_len > 0 {
                if let Err(e) = player_writer.write(&buffer[..dec_bytes_len]) {
                    println!("Encoder write error: {:?}", e);

                    break;
                };
            } else {
                if live_on {
                    println!("Switch from live ingest to offline source");

                    live_on = false;
                }

                player_writer.flush().unwrap();
            }
        }
    }

    *is_terminated.lock().unwrap() = true;

    if let Some(server) = &*server_term.lock().unwrap() {
        unsafe {
            if let Ok(_) = server.terminate() {
                println!("Terminate ingest server done");
            }
        }
    };

    sleep(Duration::from_secs(1));

    match player_proc.kill() {
        Ok(_) => println!("Playout done..."),
        Err(e) => panic!("Encoder error: {:?}", e),
    }

    if let Err(e) = player_proc.wait() {
        println!("Encoder: {e}")
    };
}
