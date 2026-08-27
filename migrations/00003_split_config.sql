ALTER TABLE global RENAME TO config_global;
ALTER TABLE roles RENAME TO auth_roles;
ALTER TABLE user RENAME TO auth_user;
ALTER TABLE user_channels RENAME TO auth_user_channels;
ALTER TABLE refresh_tokens RENAME TO auth_refresh_tokens;
ALTER TABLE text_presets RENAME TO legacy_text_preset;
ALTER TABLE outputs RENAME TO legacy_output;
ALTER TABLE recordings RENAME TO legacy_recording;

ALTER TABLE config_global ADD COLUMN notification_server TEXT NOT NULL DEFAULT '';
ALTER TABLE config_global ADD COLUMN notification_token TEXT NOT NULL DEFAULT '';

DROP INDEX IF EXISTS refresh_tokens_family_idx;
DROP INDEX IF EXISTS refresh_tokens_expiry_idx;
DROP INDEX IF EXISTS idx_user_channels_unique;

CREATE INDEX auth_refresh_tokens_family_idx ON auth_refresh_tokens(family_id);
CREATE INDEX auth_refresh_tokens_expiry_idx ON auth_refresh_tokens(expires_at);
CREATE UNIQUE INDEX auth_user_channels_unique ON auth_user_channels(channel_id, user_id);

CREATE TABLE config (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL UNIQUE,
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_general (
    config_id INTEGER PRIMARY KEY,
    stop_threshold REAL NOT NULL DEFAULT 11.0,
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_mail (
    config_id INTEGER PRIMARY KEY,
    subject TEXT NOT NULL DEFAULT 'Playout Error',
    recipient TEXT NOT NULL DEFAULT '',
    level TEXT NOT NULL DEFAULT 'ERROR',
    interval INTEGER NOT NULL DEFAULT 120,
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_notification (
    config_id INTEGER PRIMARY KEY,
    topic TEXT NOT NULL DEFAULT '',
    level TEXT NOT NULL DEFAULT 'FATAL',
    tags TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_logging (
    config_id INTEGER PRIMARY KEY,
    ffmpeg_level TEXT NOT NULL DEFAULT 'ERROR',
    ingest_level TEXT NOT NULL DEFAULT 'ERROR',
    detect_silence INTEGER NOT NULL DEFAULT 0,
    ignore_lines TEXT NOT NULL DEFAULT 'P sub_mb_type 4 out of range at;error while decoding MB;negative number of zero coeffs at;out of range intra chroma pred mode;non-existing SPS 0 referenced in buffering period',
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_processing (
    config_id INTEGER PRIMARY KEY,
    mode TEXT NOT NULL DEFAULT 'playlist',
    add_logo INTEGER NOT NULL DEFAULT 1,
    logo TEXT NOT NULL DEFAULT '00-assets/logo.png',
    logo_scale TEXT NOT NULL DEFAULT '',
    logo_opacity REAL NOT NULL DEFAULT 0.7,
    logo_position TEXT NOT NULL DEFAULT 'W-w-12:12',
    vtt_enable INTEGER NOT NULL DEFAULT 0,
    vtt_dummy TEXT DEFAULT '00-assets/dummy.vtt',
    vtt_name TEXT NOT NULL DEFAULT 'Subtitles',
    vtt_language TEXT NOT NULL DEFAULT 'en-US',
    vtt_default INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_audio (
    config_id INTEGER PRIMARY KEY,
    volume REAL NOT NULL DEFAULT 1.0,
    live_loudness_enable INTEGER NOT NULL DEFAULT 0,
    live_loudness_target_lufs REAL NOT NULL DEFAULT -23.0,
    live_loudness_dead_band_lu REAL NOT NULL DEFAULT 1.0,
    live_loudness_max_gain_db REAL NOT NULL DEFAULT 8.0,
    live_loudness_max_attenuation_db REAL NOT NULL DEFAULT -12.0,
    live_loudness_gain_up_db_per_second REAL NOT NULL DEFAULT 0.5,
    live_loudness_gain_down_db_per_second REAL NOT NULL DEFAULT 2.0,
    live_loudness_silence_gate_lufs REAL NOT NULL DEFAULT -60.0,
    live_loudness_true_peak_ceiling_dbtp REAL NOT NULL DEFAULT -1.0,
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_ingest (
    config_id INTEGER PRIMARY KEY,
    enable INTEGER NOT NULL DEFAULT 0,
    url TEXT NOT NULL DEFAULT 'rtmp://127.0.0.1:1936/live/stream',
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_playlist (
    config_id INTEGER PRIMARY KEY,
    day_start TEXT NOT NULL DEFAULT '05:59:25',
    length TEXT NOT NULL DEFAULT '24:00:00',
    infinit INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_storage (
    config_id INTEGER PRIMARY KEY,
    filler TEXT NOT NULL DEFAULT 'filler/filler.mp4',
    extensions TEXT NOT NULL DEFAULT 'mp4;mkv;webm',
    shuffle INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_text_presets (
    id INTEGER PRIMARY KEY,
    config_id INTEGER NOT NULL,
    persistent INTEGER NOT NULL DEFAULT 0 CHECK (persistent IN (0, 1)),
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
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE UNIQUE INDEX config_text_presets_one_persistent ON config_text_presets(config_id) WHERE persistent = 1;

CREATE TABLE config_task (
    config_id INTEGER PRIMARY KEY,
    enable INTEGER NOT NULL DEFAULT 0,
    path TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE TABLE config_output (
    id INTEGER PRIMARY KEY,
    config_id INTEGER NOT NULL,
    active INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
    name TEXT NOT NULL,
    hls_variants TEXT NOT NULL DEFAULT '',
    stream_url TEXT NOT NULL DEFAULT '',
    stream_type TEXT,
    stream_format TEXT,
    hls_playlist_name TEXT,
    hls_segment_duration INTEGER,
    hls_list_size INTEGER,
    desktop_fullscreen INTEGER NOT NULL DEFAULT 0,
    width INTEGER NOT NULL DEFAULT 1280,
    height INTEGER NOT NULL DEFAULT 720,
    fps REAL NOT NULL DEFAULT 25.0,
    video_codec TEXT,
    video_options TEXT NOT NULL DEFAULT '{}',
    audio_codec TEXT,
    audio_bitrate INTEGER,
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

CREATE UNIQUE INDEX config_output_one_active ON config_output(config_id) WHERE active = 1;

CREATE TABLE config_recording (
    config_id INTEGER PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'stream',
    source_output_id INTEGER,
    hls_variant TEXT NOT NULL DEFAULT '',
    path TEXT NOT NULL DEFAULT '/var/lib/ffplayout/recordings',
    segment_duration INTEGER NOT NULL DEFAULT 300,
    retention_days INTEGER NOT NULL DEFAULT 62,
    minimum_free_space_gb INTEGER NOT NULL DEFAULT 0,
    width INTEGER NOT NULL DEFAULT 0,
    height INTEGER NOT NULL DEFAULT 0,
    video_codec TEXT NOT NULL DEFAULT 'libx264',
    video_options TEXT NOT NULL DEFAULT '{"preset":"faster","rate_control":"crf","quality":"23","maxrate":"2400"}',
    audio_codec TEXT NOT NULL DEFAULT 'aac',
    audio_bitrate INTEGER NOT NULL DEFAULT 128,
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (source_output_id) REFERENCES config_output(id) ON UPDATE CASCADE ON DELETE SET NULL
);

INSERT INTO config(id, channel_id)
SELECT id, channel_id FROM configurations;

INSERT INTO config_general(config_id, stop_threshold)
SELECT id, general_stop_threshold FROM configurations;

INSERT INTO config_mail(config_id, subject, recipient, level, interval)
SELECT id, mail_subject, mail_recipient, mail_level, mail_interval FROM configurations;

INSERT INTO config_notification(config_id)
SELECT id FROM config;

INSERT INTO config_logging(config_id, ffmpeg_level, ingest_level, detect_silence, ignore_lines)
SELECT id, logging_ffmpeg_level, logging_ingest_level, logging_detect_silence, logging_ignore FROM configurations;

INSERT INTO config_processing(config_id, mode, add_logo, logo, logo_scale, logo_opacity, logo_position, vtt_enable, vtt_dummy, vtt_name, vtt_language, vtt_default)
SELECT id, processing_mode, processing_add_logo, processing_logo, processing_logo_scale, processing_logo_opacity, processing_logo_position, processing_vtt_enable, processing_vtt_dummy, processing_vtt_name, processing_vtt_language, processing_vtt_default FROM configurations;

INSERT INTO config_audio(config_id, volume, live_loudness_enable, live_loudness_target_lufs, live_loudness_dead_band_lu, live_loudness_max_gain_db, live_loudness_max_attenuation_db, live_loudness_gain_up_db_per_second, live_loudness_gain_down_db_per_second, live_loudness_silence_gate_lufs, live_loudness_true_peak_ceiling_dbtp)
SELECT c.id, a.volume, a.live_loudness_enable, a.live_loudness_target_lufs, a.live_loudness_dead_band_lu, a.live_loudness_max_gain_db, a.live_loudness_max_attenuation_db, a.live_loudness_gain_up_db_per_second, a.live_loudness_gain_down_db_per_second, a.live_loudness_silence_gate_lufs, a.live_loudness_true_peak_ceiling_dbtp
FROM configurations c JOIN audio_config a ON a.channel_id = c.channel_id;

INSERT INTO config_ingest(config_id, enable, url)
SELECT id, ingest_enable, ingest_url FROM configurations;

INSERT INTO config_playlist(config_id, day_start, length, infinit)
SELECT id, playlist_day_start, playlist_length, playlist_infinit FROM configurations;

INSERT INTO config_storage(config_id, filler, extensions, shuffle)
SELECT id, storage_filler, storage_extensions, storage_shuffle FROM configurations;

INSERT INTO config_text_presets(id, config_id, persistent, name, text, use_filename, font_family, font_weight, filename_regex, position_x, position_y, font_size, line_spacing, text_color, text_opacity, background_enabled, background_color, background_opacity, background_padding, opacity, scroll_direction, scroll_speed, scroll_repeat, fade_in_seconds, fade_out_seconds)
SELECT p.id, c.id, COALESCE(p.id = c.text_preset_id, 0), p.name, p.text, p.use_filename, p.font_family, p.font_weight, p.filename_regex, p.position_x, p.position_y, p.font_size, p.line_spacing, p.text_color, p.text_opacity, p.background_enabled, p.background_color, p.background_opacity, p.background_padding, p.opacity, p.scroll_direction, p.scroll_speed, p.scroll_repeat, p.fade_in_seconds, p.fade_out_seconds
FROM legacy_text_preset p JOIN configurations c ON c.channel_id = p.channel_id;

INSERT INTO config_task(config_id, enable, path)
SELECT id, task_enable, task_path FROM configurations;

INSERT INTO config_output(id, config_id, active, name, hls_variants, stream_url, stream_type, stream_format, hls_playlist_name, hls_segment_duration, hls_list_size, desktop_fullscreen, width, height, fps, video_codec, video_options, audio_codec, audio_bitrate)
SELECT o.id, c.id, o.id = c.output_id, o.name, o.hls_variants, o.stream_url, o.stream_type, o.stream_format, o.hls_playlist_name, o.hls_segment_duration, o.hls_list_size, o.desktop_fullscreen, o.width, o.height, o.fps, o.video_codec, o.video_options, o.audio_codec, o.audio_bitrate
FROM legacy_output o JOIN configurations c ON c.channel_id = o.channel_id;

INSERT INTO config_recording(config_id, enabled, source, source_output_id, hls_variant, path, segment_duration, retention_days, minimum_free_space_gb, width, height, video_codec, video_options, audio_codec, audio_bitrate)
SELECT c.id, r.enabled, r.source, r.source_output_id, r.hls_variant, r.path, r.segment_duration, r.retention_days, r.minimum_free_space_gb, r.width, r.height, r.video_codec, r.video_options, r.audio_codec, r.audio_bitrate
FROM legacy_recording r JOIN configurations c ON c.channel_id = r.channel_id;

DROP TABLE legacy_recording;
DROP TABLE audio_config;
DROP TABLE configurations;
DROP TABLE legacy_output;
DROP TABLE legacy_text_preset;
