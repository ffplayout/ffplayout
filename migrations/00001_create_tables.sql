-- Add migration script here
PRAGMA foreign_keys = ON;

CREATE TABLE
    global (
        id INTEGER PRIMARY KEY,
        secret TEXT NOT NULL,
        logs TEXT NOT NULL DEFAULT "/var/log/ffplayout",
        playlists TEXT NOT NULL DEFAULT "/var/lib/ffplayout/playlists",
        public TEXT NOT NULL DEFAULT "/usr/share/ffplayout/public",
        storage TEXT NOT NULL DEFAULT "/var/lib/ffplayout/tv-media",
        shared INTEGER NOT NULL DEFAULT 0,
        UNIQUE (secret)
    );

CREATE TABLE
    roles (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        UNIQUE (name)
    );

CREATE TABLE
    channels (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        preview_url TEXT NOT NULL,
        extra_extensions TEXT NOT NULL DEFAULT 'jpg,jpeg,png',
        active INTEGER NOT NULL DEFAULT 0,
        public TEXT NOT NULL DEFAULT "/usr/share/ffplayout/public",
        playlists TEXT NOT NULL DEFAULT "/var/lib/ffplayout/playlists",
        storage TEXT NOT NULL DEFAULT "/var/lib/ffplayout/tv-media",
        last_date TEXT,
        time_shift REAL NOT NULL DEFAULT 0
    );

CREATE TABLE
    presets (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        text TEXT NOT NULL,
        x TEXT NOT NULL,
        y TEXT NOT NULL,
        fontsize TEXT NOT NULL,
        line_spacing TEXT NOT NULL,
        fontcolor TEXT NOT NULL,
        box TEXT NOT NULL,
        boxcolor TEXT NOT NULL,
        boxborderw TEXT NOT NULL,
        alpha TEXT NOT NULL,
        channel_id INTEGER NOT NULL DEFAULT 1,
        FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE
    );

CREATE TABLE
    user (
        id INTEGER PRIMARY KEY,
        mail TEXT NOT NULL,
        username TEXT NOT NULL,
        password TEXT NOT NULL,
        role_id INTEGER NOT NULL DEFAULT 3,
        FOREIGN KEY (role_id) REFERENCES roles (id) ON UPDATE SET NULL ON DELETE SET DEFAULT,
        UNIQUE (mail, username)
    );

CREATE TABLE
    user_channels (
        id INTEGER PRIMARY KEY,
        channel_id INTEGER NOT NULL,
        user_id INTEGER NOT NULL,
        FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE,
        FOREIGN KEY (user_id) REFERENCES user (id) ON UPDATE CASCADE ON DELETE CASCADE
    );

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_channels_unique ON user_channels (channel_id, user_id);

CREATE TABLE
    configurations (
        id INTEGER PRIMARY KEY,
        channel_id INTEGER NOT NULL DEFAULT 1,
        general_help TEXT NOT NULL DEFAULT "Sometimes it can happen that a file is corrupt but still playable. This can produce a streaming error for all following files. The only solution in this case is to stop ffplayout and start it again.\n'stop_threshold' stops ffplayout if it is asynchronous in time above this value. A number below 3 can cause unexpected errors.",
        general_stop_threshold REAL NOT NULL DEFAULT 11.0,
        mail_help TEXT NOT NULL DEFAULT "Send error messages to an email address, such as missing playlist, invalid JSON format, or missing clip path. Leave the recipient blank if you don't need this.\n'mail_level' can be INFO, WARNING, or ERROR.\n'interval' refers to the number of seconds until a new email is sent; the value must be in increments of 10.",
        mail_subject TEXT NOT NULL DEFAULT "Playout Error",
        mail_smtp TEXT NOT NULL DEFAULT "mail.example.org",
        mail_addr TEXT NOT NULL DEFAULT "ffplayout@example.org",
        mail_pass TEXT NOT NULL DEFAULT "",
        mail_recipient TEXT NOT NULL DEFAULT "",
        mail_starttls INTEGER NOT NULL DEFAULT 0,
        mail_level TEXT NOT NULL DEFAULT "ERROR",
        mail_interval INTEGER NOT NULL DEFAULT 120,
        logging_help TEXT NOT NULL DEFAULT "'ffmpeg_level/ingest_level' can be INFO, WARNING, or ERROR.\n'detect_silence' logs an error message if the audio line is silent for 15 seconds during the validation process.\n'ignore' allows logging to ignore strings that contain matched lines; the format is a semicolon-separated list.",
        logging_ffmpeg_level TEXT NOT NULL DEFAULT "ERROR",
        logging_ingest_level TEXT NOT NULL DEFAULT "ERROR",
        logging_detect_silence INTEGER NOT NULL DEFAULT 0,
        logging_ignore TEXT NOT NULL DEFAULT "P sub_mb_type 4 out of range at;error while decoding MB;negative number of zero coeffs at;out of range intra chroma pred mode;non-existing SPS 0 referenced in buffering period",
        processing_help TEXT NOT NULL DEFAULT "Default processing for all clips ensures uniqueness. The mode can be either 'playlist' or 'folder'.\nThe 'aspect' parameter must be a float number.\nThe 'audio_tracks' parameter specifies how many audio tracks should be processed.'audio_channels' can be used if the audio has more channels than stereo.\nThe 'logo' is used only if the path exists; the path is relative to your storage folder.\n'logo_scale' scales the logo to the target size. Leave it blank if no scaling is needed. The format is 'width:height', for example, '100:-1' for proportional scaling. The 'logo_opacity' option allows the logo to become transparent.'logo_position' is specified in the format 'x:y', which sets the logo's position.\nWith 'custom_filter', it is possible to apply additional filters. The filter outputs should end with [c_v_out] for video filters and [c_a_out] for audio filters.\n'vtt_enable' can only be used in HLS mode, and only when *.vtt files with the same filename as the video file exist.",
        processing_mode TEXT NOT NULL DEFAULT "playlist",
        processing_audio_only INTEGER NOT NULL DEFAULT 0,
        processing_copy_audio INTEGER NOT NULL DEFAULT 0,
        processing_copy_video INTEGER NOT NULL DEFAULT 0,
        processing_width INTEGER NOT NULL DEFAULT 1280,
        processing_height INTEGER NOT NULL DEFAULT 720,
        processing_aspect REAL NOT NULL DEFAULT 1.778,
        processing_fps REAL NOT NULL DEFAULT 25.0,
        processing_add_logo INTEGER NOT NULL DEFAULT 1,
        processing_logo TEXT NOT NULL DEFAULT "00-assets/logo.png",
        processing_logo_scale TEXT NOT NULL DEFAULT "",
        processing_logo_opacity REAL NOT NULL DEFAULT 0.7,
        processing_logo_position TEXT NOT NULL DEFAULT "W-w-12:12",
        processing_audio_tracks INTEGER NOT NULL DEFAULT 1,
        processing_audio_track_index INTEGER NOT NULL DEFAULT -1,
        processing_audio_channels INTEGER NOT NULL DEFAULT 2,
        processing_volume REAL NOT NULL DEFAULT 1.0,
        processing_filter TEXT NOT NULL DEFAULT "",
        processing_vtt_enable INTEGER NOT NULL DEFAULT 0,
        processing_vtt_dummy TEXT NULL DEFAULT "00-assets/dummy.vtt",
        ingest_help "Run a server for an ingest stream. This stream will override the normal streaming until it is finished. There is only a very simple authentication mechanism, which checks if the stream name is correct.\n'custom_filter' can be used in the same way as the one in the process section.",
        ingest_enable INTEGER NOT NULL DEFAULT 0,
        ingest_param TEXT NOT NULL DEFAULT "-f live_flv -listen 1 -i rtmp://127.0.0.1:1936/live/stream",
        ingest_filter TEXT NOT NULL DEFAULT "",
        playlist_help TEXT NOT NULL DEFAULT "'day_start' indicates at what time the playlist should start; leave 'day_start' blank if the playlist should always start at the beginning. 'length' represents the target length of the playlist; when it is blank, the real length will not be considered.\n'infinite: true' works with a single playlist file and loops it infinitely.",
        playlist_day_start TEXT NOT NULL DEFAULT "05:59:25",
        playlist_length TEXT NOT NULL DEFAULT "24:00:00",
        playlist_infinit INTEGER NOT NULL DEFAULT 0,
        storage_help TEXT NOT NULL DEFAULT "'filler' is used to play in place of a missing file or to fill the remaining time to reach a total of 24 hours. It can be a file or folder and will loop when necessary.\n'extensions' specifies which files to search for by this extension. Activate 'shuffle' to pick files randomly.",
        storage_filler TEXT NOT NULL DEFAULT "filler/filler.mp4",
        storage_extensions TEXT NOT NULL DEFAULT "mp4;mkv;webm",
        storage_shuffle INTEGER NOT NULL DEFAULT 1,
        text_help TEXT NOT NULL DEFAULT "Overlay text in combination with libzmq for remote text manipulation. 'font' is a relative path to your storage folder.\n'text_from_filename' activates the extraction of text from a filename. With 'style', you can define the drawtext parameters, such as position, color, etc. Posting text over the API will override this. With 'regex', you can format file names to extract a title from them.",
        text_add INTEGER NOT NULL DEFAULT 1,
        text_from_filename INTEGER NOT NULL DEFAULT 0,
        text_font TEXT NOT NULL DEFAULT "00-assets/DejaVuSans.ttf",
        text_style TEXT NOT NULL DEFAULT "x=(w-tw)/2:y=(h-line_h)*0.9:fontsize=24:fontcolor=#ffffff:box=1:boxcolor=#000000:boxborderw=4",
        text_regex TEXT NOT NULL DEFAULT "^.+[/\\](.*)(.mp4|.mkv|.webm)$",
        task_help TEXT NOT NULL DEFAULT "Run an external program with a given media object. The media object is in JSON format and contains all the information about the current clip. The external program can be a script or a binary, but it should only run for a short time.",
        task_enable INTEGER NOT NULL DEFAULT 0,
        task_path TEXT NOT NULL DEFAULT "",
        output_help TEXT NOT NULL DEFAULT "The final playout encoding, set the settings according to your needs. 'mode' has the options 'desktop', 'hls', 'null', and 'stream'. Use 'stream' and adjust the 'output_param:' settings when you want to stream to an RTMP/RTSP/SRT/... server.\nIn production, don't serve HLS playlists with ffplayout; use Nginx or another web server!",
        output_mode TEXT NOT NULL DEFAULT "hls",
        output_param TEXT NOT NULL DEFAULT "-c:v libx264 -crf 23 -x264-params keyint=50:min-keyint=25:scenecut=-1 -maxrate 1300k -bufsize 2600k -preset faster -tune zerolatency -profile:v Main -level 3.1 -c:a aac -ar 44100 -b:a 128k -flags +cgop -muxpreload 0 -muxdelay 0 -f hls -hls_time 6 -hls_list_size 600 -hls_flags append_list+delete_segments+omit_endlist -hls_segment_filename live/stream-%d.ts live/stream.m3u8",
        FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE
    );

CREATE TABLE
    advanced_configurations (
        id INTEGER PRIMARY KEY,
        channel_id INTEGER NOT NULL DEFAULT 1,
        decoder_input_param TEXT,
        decoder_output_param TEXT,
        encoder_input_param TEXT,
        ingest_input_param TEXT,
        filter_deinterlace TEXT,
        filter_pad_scale_w TEXT,
        filter_pad_scale_h TEXT,
        filter_pad_video TEXT,
        filter_fps TEXT,
        filter_scale TEXT,
        filter_set_dar TEXT,
        filter_fade_in TEXT,
        filter_fade_out TEXT,
        filter_overlay_logo_scale TEXT,
        filter_overlay_logo_fade_in TEXT,
        filter_overlay_logo_fade_out TEXT,
        filter_overlay_logo TEXT,
        filter_tpad TEXT,
        filter_drawtext_from_file TEXT,
        filter_drawtext_from_zmq TEXT,
        filter_aevalsrc TEXT,
        filter_afade_in TEXT,
        filter_afade_out TEXT,
        filter_apad TEXT,
        filter_volume TEXT,
        filter_split TEXT,
        FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE
    );

-------------------------------------------------------------------------------
-- set defaults
INSERT INTO
    roles (name)
VALUES
    ('global_admin'),
    ('channel_admin'),
    ('user'),
    ('guest');

INSERT INTO
    channels (name, preview_url, extra_extensions, active)
VALUES
    (
        'Channel 1',
        'http://127.0.0.1:8787/1/live/stream.m3u8',
        'jpg,jpeg,png',
        0
    );

INSERT INTO
    presets (
        name,
        text,
        x,
        y,
        fontsize,
        line_spacing,
        fontcolor,
        box,
        boxcolor,
        boxborderw,
        alpha,
        channel_id
    )
VALUES
    (
        'Default',
        'Wellcome to ffplayout messenger!',
        '(w-text_w)/2',
        '(h-text_h)/2',
        '24',
        '4',
        '#ffffff@0xff',
        '0',
        '#000000@0x80',
        '4',
        '1.0',
        '1'
    ),
    (
        'Empty Text',
        '',
        '0',
        '0',
        '24',
        '4',
        '#000000',
        '0',
        '#000000',
        '0',
        '0',
        '1'
    ),
    (
        'Bottom Text fade in',
        'The upcoming event will be delayed by a few minutes.',
        '(w-text_w)/2',
        '(h-line_h)*0.9',
        '24',
        '4',
        '#ffffff',
        '1',
        '#000000@0x80',
        '4',
        'ifnot(ld(1),st(1,t));if(lt(t,ld(1)+1),0,if(lt(t,ld(1)+2),(t-(ld(1)+1))/1,if(lt(t,ld(1)+8),1,if(lt(t,ld(1)+9),(1-(t-(ld(1)+8)))/1,0))))',
        '1'
    ),
    (
        'Scrolling Text',
        'We have a very important announcement to make.',
        'ifnot(ld(1),st(1,t));if(lt(t,ld(1)+1),w+4,w-w/12*mod(t-ld(1),12*(w+tw)/w))',
        '(h-line_h)*0.9',
        '24',
        '4',
        '#ffffff',
        '1',
        '#000000@0x80',
        '4',
        '1.0',
        '1'
    );

INSERT INTO
    configurations DEFAULT
VALUES;

INSERT INTO
    advanced_configurations DEFAULT
VALUES;
