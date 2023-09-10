use ffplayout_lib::utils::{generator::ordered_list, sum_durations, Media};

#[test]
fn test_ordered_list() {
    let clip_list = vec![
        Media::new(0, "./assets/with_audio.mp4", true), // 30 seconds
        Media::new(0, "./assets/dual_audio.mp4", true), // 30 seconds
        Media::new(0, "./assets/av_sync.mp4", true),    // 30 seconds
        Media::new(0, "./assets/ad.mp4", true),         // 25 seconds
    ];

    let result = ordered_list(clip_list.clone(), 85.0);

    assert_eq!(result.len(), 3);
    assert_eq!(result[2].duration, 25.0);
    assert_eq!(sum_durations(&result), 85.0);

    let result = ordered_list(clip_list, 120.0);

    assert_eq!(result.len(), 4);
    assert_eq!(result[2].duration, 30.0);
    assert_eq!(sum_durations(&result), 115.0);
}
