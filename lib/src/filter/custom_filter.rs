use simplelog::*;

fn strip_str(mut input: &str) -> String {
    input = input.strip_prefix(';').unwrap_or(input);
    input = input.strip_prefix("[0:v]").unwrap_or(input);
    input = input.strip_prefix("[0:a]").unwrap_or(input);
    input = input.strip_suffix(';').unwrap_or(input);
    input = input.strip_suffix("[c_v_out]").unwrap_or(input);
    input = input.strip_suffix("[c_a_out]").unwrap_or(input);

    input.to_string()
}

/// Apply custom filters
pub fn custom_filter(filter: &str) -> (String, String) {
    let mut video_filter = String::new();
    let mut audio_filter = String::new();

    if filter.contains("[c_v_out]") && filter.contains("[c_a_out]") {
        let v_pos = filter.find("[c_v_out]").unwrap();
        let a_pos = filter.find("[c_a_out]").unwrap();
        let mut delimiter = "[c_v_out]";

        if v_pos > a_pos {
            delimiter = "[c_a_out]";
        }

        if let Some((f_1, f_2)) = filter.split_once(delimiter) {
            if f_2.contains("[c_a_out]") {
                video_filter = strip_str(f_1);
                audio_filter = strip_str(f_2);
            } else {
                video_filter = strip_str(f_2);
                audio_filter = strip_str(f_1);
            }
        }
    } else if filter.contains("[c_v_out]") {
        video_filter = strip_str(filter);
    } else if filter.contains("[c_a_out]") {
        audio_filter = strip_str(filter);
    } else if !filter.is_empty() && filter != "~" {
        error!("Custom filter is not well formatted, use correct out link names (\"[c_v_out]\" and/or \"[c_a_out]\"). Filter skipped!")
    }

    (video_filter, audio_filter)
}
