use std::{
    process::exit,
    sync::{Arc, Mutex},
};

use chrono::{Duration, NaiveDate};
use simplelog::*;

use crate::input::Source;
use crate::utils::{json_serializer::Playlist, GlobalConfig, Media};

fn get_date_range(date_range: &Vec<String>) -> Vec<String> {
    let mut range = vec![];
    let start;
    let end;

    match NaiveDate::parse_from_str(&date_range[0], "%Y-%m-%d") {
        Ok(s) => {
            start = s;
        }
        Err(_) => {
            error!("date format error in: <yellow>{:?}</>", date_range[0]);
            exit(1);
        }
    }

    match NaiveDate::parse_from_str(&date_range[2], "%Y-%m-%d") {
        Ok(e) => {
            end = e;
        }
        Err(_) => {
            error!("date format error in: <yellow>{:?}</>", date_range[2]);
            exit(1);
        }
    }

    let duration = end.signed_duration_since(start);
    let days = duration.num_days() + 1;

    for day in 0..days {
        range.push((start + Duration::days(day)).format("%Y-%m-%d").to_string());
    }

    range
}

pub fn generate_playlist() {
    let config = GlobalConfig::global();
    let mut date_range = config.general.generate.clone().unwrap();
    let total_length = config.playlist.length_sec.unwrap().clone();
    let current_list = Arc::new(Mutex::new(vec![Media::new(0, "".to_string(), false)]));
    let index = Arc::new(Mutex::new(0));

    if date_range.contains(&"-".to_string()) && date_range.len() == 3 {
        date_range = get_date_range(&date_range)
    }

    let media_list = Source::new(current_list, index);

    for date in date_range {
        info!("Generate playlist for {date}");

        let mut filler = Media::new(0, config.storage.filler_clip.clone(), true);
        let filler_length = filler.duration.clone();
        let mut length = 0.0;

        let mut playlist = Playlist {
            date,
            current_file: None,
            start_sec: None,
            modified: None,
            program: vec![],
        };

        let mut round = 0;

        for item in media_list.clone() {
            let duration = item.duration.clone();

            if total_length > length + filler_length {
                playlist.program.push(item);

                length += duration;
            } else if filler_length > 0.0 && filler_length > total_length - length {
                println!("{filler_length}");
                filler.out = filler_length - (total_length - length);
                println!("{}", total_length - length);
                playlist.program.push(filler);

                break;
            } else if round == 3 {
                println!("break");
                println!("length {length}");
                println!("total_length {total_length}");
                break;
            } else {
                round += 1;
            }
        }

        println!("{length:?}");
        println!("{:?}", playlist.program[playlist.program.len() - 1]);
    }
}
