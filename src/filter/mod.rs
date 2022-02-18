use crate::utils::MediaProbe;

fn deinterlace(probe: MediaProbe) -> String {
    if probe.video_streams.unwrap()[0].field_order.is_some()
        && probe.video_streams.unwrap()[0].field_order.unwrap() != "progressive".to_string() {
        "yadif=0:-1:0"
    }

    ""
}
