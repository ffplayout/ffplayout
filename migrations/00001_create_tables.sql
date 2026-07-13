PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS global (
    id INTEGER PRIMARY KEY,
    secret TEXT NOT NULL UNIQUE,
    logs TEXT NOT NULL DEFAULT '/var/log/ffplayout',
    playlists TEXT NOT NULL DEFAULT '/var/lib/ffplayout/playlists',
    public TEXT NOT NULL DEFAULT '/usr/share/ffplayout/public',
    storage TEXT NOT NULL DEFAULT '/var/lib/ffplayout/tv-media',
    shared INTEGER NOT NULL DEFAULT 0,
    smtp_server TEXT NOT NULL DEFAULT 'mail.example.org',
    smtp_user TEXT NOT NULL DEFAULT 'ffplayout@example.org',
    smtp_password TEXT NOT NULL DEFAULT '',
    smtp_starttls INTEGER NOT NULL DEFAULT 0,
    smtp_port INTEGER NOT NULL DEFAULT 465,
    setup_completed INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS roles (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);

CREATE TABLE IF NOT EXISTS channels (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    preview_url TEXT NOT NULL,
    extra_extensions TEXT NOT NULL DEFAULT 'jpg,jpeg,png',
    active INTEGER NOT NULL DEFAULT 0,
    public TEXT NOT NULL DEFAULT '/usr/share/ffplayout/public',
    playlists TEXT NOT NULL DEFAULT '/var/lib/ffplayout/playlists',
    storage TEXT NOT NULL DEFAULT '/var/lib/ffplayout/tv-media',
    last_date TEXT,
    time_shift REAL NOT NULL DEFAULT 0,
    timezone TEXT
);

CREATE TABLE IF NOT EXISTS text_presets (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL DEFAULT 1,
    name TEXT NOT NULL,
    text TEXT NOT NULL,
    use_filename INTEGER NOT NULL DEFAULT 0,
    font_family TEXT NOT NULL DEFAULT 'DejaVu Sans',
    font_weight TEXT NOT NULL DEFAULT 'normal',
    filename_regex TEXT NOT NULL DEFAULT '^.+[/\\](.*)(.mp4|.mkv|.webm)$',
    position_x TEXT NOT NULL DEFAULT 'center',
    position_y TEXT NOT NULL DEFAULT 'end:72',
    font_size REAL NOT NULL DEFAULT 24.0,
    line_spacing REAL NOT NULL DEFAULT 4.0,
    text_color TEXT NOT NULL DEFAULT '#ffffff',
    text_opacity REAL NOT NULL DEFAULT 1.0,
    background_enabled INTEGER NOT NULL DEFAULT 0,
    background_color TEXT NOT NULL DEFAULT '#000000',
    background_opacity REAL NOT NULL DEFAULT 0.8,
    background_padding INTEGER NOT NULL DEFAULT 4,
    opacity REAL NOT NULL DEFAULT 1.0,
    scroll_direction TEXT NOT NULL DEFAULT 'none',
    scroll_speed INTEGER NOT NULL DEFAULT 100,
    scroll_repeat INTEGER NOT NULL DEFAULT -1,
    fade_in_seconds REAL NOT NULL DEFAULT 0.0,
    fade_out_seconds REAL NOT NULL DEFAULT 0.0,
    FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS user(
    id INTEGER PRIMARY KEY,
    mail TEXT NOT NULL,
    username TEXT NOT NULL UNIQUE,
    password TEXT NOT NULL,
    role_id INTEGER NOT NULL DEFAULT 3,
    two_factor INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (role_id) REFERENCES roles (id) ON UPDATE SET NULL ON DELETE SET DEFAULT
);

CREATE TABLE IF NOT EXISTS user_channels (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES user(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_channels_unique ON user_channels (channel_id, user_id);

CREATE TABLE IF NOT EXISTS outputs (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL DEFAULT 1,
    name TEXT NOT NULL,
    hls_variants TEXT NOT NULL DEFAULT '',
    stream_url TEXT NOT NULL DEFAULT '',
    stream_type TEXT,
    hls_playlist_name TEXT,
    hls_segment_duration INTEGER,
    hls_list_size INTEGER,
    desktop_fullscreen INTEGER NOT NULL DEFAULT 0,
    width INTEGER NOT NULL DEFAULT 1280,
    height INTEGER NOT NULL DEFAULT 720,
    fps REAL NOT NULL DEFAULT 25.0,
    video_preset TEXT,
    video_codec TEXT,
    audio_codec TEXT,
    rate_control TEXT,
    video_quality INTEGER,
    video_maxrate INTEGER,
    audio_bitrate INTEGER,
    FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS configurations (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL DEFAULT 1,
    general_stop_threshold REAL NOT NULL DEFAULT 11.0,
    mail_subject TEXT NOT NULL DEFAULT 'Playout Error',
    mail_recipient TEXT NOT NULL DEFAULT '',
    mail_level TEXT NOT NULL DEFAULT 'ERROR',
    mail_interval INTEGER NOT NULL DEFAULT 120,
    logging_ffmpeg_level TEXT NOT NULL DEFAULT 'ERROR',
    logging_ingest_level TEXT NOT NULL DEFAULT 'ERROR',
    logging_detect_silence INTEGER NOT NULL DEFAULT 0,
    logging_ignore TEXT NOT NULL DEFAULT 'P sub_mb_type 4 out of range at;error while decoding MB;negative number of zero coeffs at;out of range intra chroma pred mode;non-existing SPS 0 referenced in buffering period',
    processing_mode TEXT NOT NULL DEFAULT 'playlist',
    processing_add_logo INTEGER NOT NULL DEFAULT 1,
    processing_logo TEXT NOT NULL DEFAULT '00-assets/logo.png',
    processing_logo_scale TEXT NOT NULL DEFAULT '',
    processing_logo_opacity REAL NOT NULL DEFAULT 0.7,
    processing_logo_position TEXT NOT NULL DEFAULT 'W-w-12:12',
    processing_volume REAL NOT NULL DEFAULT 1.0,
    processing_vtt_enable INTEGER NOT NULL DEFAULT 0,
    processing_vtt_dummy TEXT DEFAULT '00-assets/dummy.vtt',
    processing_vtt_name TEXT NOT NULL DEFAULT 'Subtitles',
    processing_vtt_language TEXT NOT NULL DEFAULT 'en-US',
    processing_vtt_default INTEGER NOT NULL DEFAULT 0,
    ingest_enable INTEGER NOT NULL DEFAULT 0,
    ingest_url TEXT NOT NULL DEFAULT 'rtmp://127.0.0.1:1936/live/stream',
    playlist_day_start TEXT NOT NULL DEFAULT '05:59:25',
    playlist_length TEXT NOT NULL DEFAULT '24:00:00',
    playlist_infinit INTEGER NOT NULL DEFAULT 0,
    storage_filler TEXT NOT NULL DEFAULT 'filler/filler.mp4',
    storage_extensions TEXT NOT NULL DEFAULT 'mp4;mkv;webm',
    storage_shuffle INTEGER NOT NULL DEFAULT 1,
    text_preset_id INTEGER,
    task_enable INTEGER NOT NULL DEFAULT 0,
    task_path TEXT NOT NULL DEFAULT '',
    output_id INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (output_id) REFERENCES outputs (id) ON UPDATE CASCADE,
    FOREIGN KEY (text_preset_id) REFERENCES text_presets (id) ON UPDATE CASCADE ON DELETE SET NULL
);

INSERT OR IGNORE INTO
    roles (id, name)
VALUES
    (1, 'global_admin'),
    (2, 'channel_admin'),
    (3, 'user'),
    (4, 'guest');

INSERT OR IGNORE INTO
    channels (id, name, preview_url, extra_extensions, active)
VALUES
    (
        1,
        'Channel 1',
        'http://127.0.0.1:8787/public/1/live/stream.m3u8',
        'jpg,jpeg,png',
        0
    );

INSERT OR IGNORE INTO
    text_presets (
        id,
        channel_id,
        name,
        text,
        use_filename,
        font_family,
        font_weight,
        filename_regex,
        position_x,
        position_y,
        font_size,
        line_spacing,
        text_color,
        text_opacity,
        background_enabled,
        background_color,
        background_opacity,
        background_padding,
        opacity,
        scroll_direction,
        scroll_speed,
        scroll_repeat,
        fade_in_seconds,
        fade_out_seconds
    )
VALUES
    (
        1,
        1,
        'Default',
        'Welcome to ffplayout messenger!',
        0,
        'DejaVu Sans',
        'normal',
        '^.+[/\\](.*)(.mp4|.mkv|.webm)$',
        'center',
        'center',
        24.0,
        4.0,
        '#ffffff',
        1.0,
        0,
        '#000000',
        0.8,
        4,
        1.0,
        'none',
        100,
        -1,
        0.0,
        0.0
    ),
    (
        2,
        1,
        'Bottom Text fade in',
        'The upcoming event will be delayed by a few minutes.',
        0,
        'DejaVu Sans',
        'normal',
        '^.+[/\\](.*)(.mp4|.mkv|.webm)$',
        'center',
        'end:72',
        24.0,
        4.0,
        '#ffffff',
        1.0,
        1,
        '#000000',
        0.8,
        4,
        1.0,
        'none',
        100,
        -1,
        1.0,
        1.0
    ),
    (
        3,
        1,
        'Scrolling Text',
        'We have a very important announcement to make.',
        0,
        'DejaVu Sans',
        'normal',
        '^.+[/\\](.*)(.mp4|.mkv|.webm)$',
        'center',
        'end:72',
        24.0,
        4.0,
        '#ffffff',
        1.0,
        1,
        '#000000',
        0.8,
        4,
        1.0,
        'right_to_left',
        100,
        -1,
        0.0,
        0.0
    ),
    (
        4,
        1,
        'Filename overlay',
        '',
        1,
        'DejaVu Sans',
        'normal',
        '^.+[/\\](.*)(.mp4|.mkv|.webm)$',
        'center',
        'end:72',
        24.0,
        4.0,
        '#ffffff',
        1.0,
        1,
        '#000000',
        0.8,
        4,
        1.0,
        'none',
        100,
        -1,
        0.0,
        0.0
    );

INSERT OR IGNORE INTO
    outputs (
        id,
        channel_id,
        name,
        hls_variants,
        stream_url,
        stream_type,
        hls_playlist_name,
        hls_segment_duration,
        hls_list_size,
        width,
        height,
        fps,
        video_preset,
        video_codec,
        audio_codec,
        rate_control,
        video_quality,
        video_maxrate,
        audio_bitrate
    )
VALUES
    (1, 1, 'hls', '', '', NULL, 'stream', 6, 600, 1280, 720, 25.0, 'faster', 'libx264', 'aac', 'crf', 23, 2400, 128),
    (
        2,
        1,
        'stream',
        '',
        'rtmp://127.0.0.1/live/stream',
        'rtmp',
        NULL,
        NULL,
        NULL,
        1280,
        720,
        25.0,
        'faster',
        'libx264',
        'aac',
        'crf',
        23,
        2400,
        128
    ),
    (3, 1, 'desktop', '', '', NULL, NULL, NULL, NULL, 1280, 720, 25.0, NULL, NULL, NULL, NULL, NULL, NULL, NULL);

INSERT OR IGNORE INTO
    configurations (id, channel_id, output_id)
VALUES
    (1, 1, 1);
