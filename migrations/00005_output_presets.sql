CREATE TABLE
    outputs (
        id INTEGER PRIMARY KEY,
        channel_id INTEGER NOT NULL DEFAULT 1,
        name TEXT NOT NULL,
        parameters TEXT NOT NULL,
        FOREIGN KEY (channel_id) REFERENCES channels (id) ON UPDATE CASCADE ON DELETE CASCADE
    );

-- Temporarily store old output_mode values
ALTER TABLE configurations
ADD COLUMN output_mode_old TEXT;

UPDATE configurations
SET
    output_mode_old = output_mode;

ALTER TABLE configurations
DROP COLUMN output_mode;

ALTER TABLE configurations
ADD COLUMN output_id INTEGER NOT NULL DEFAULT 0;

INSERT INTO
    outputs (channel_id, name, parameters)
SELECT
    id,
    'hls',
    '-c:v libx264 -crf 23 -x264-params keyint=50:min-keyint=25:scenecut=-1 -maxrate 1300k -bufsize 2600k -preset faster -tune zerolatency -profile:v Main -level 3.1 -c:a aac -ar 44100 -b:a 128k -flags +cgop -muxpreload 0 -muxdelay 0 -f hls -hls_time 6 -hls_list_size 600 -hls_flags append_list+delete_segments+omit_endlist -hls_segment_filename live/stream-%d.ts live/stream.m3u8'
FROM
    channels;

INSERT INTO
    outputs (channel_id, name, parameters)
SELECT
    id,
    'stream',
    '-c:v libx264 -crf 23 -x264-params keyint=50:min-keyint=25:scenecut=-1 -maxrate 1300k -bufsize 2600k -preset faster -tune zerolatency -profile:v Main -level 3.1 -c:a aac -ar 44100 -b:a 128k -flags +global_header -f flv rtmp://127.0.0.1/live/stream'
FROM
    channels;

INSERT INTO
    outputs (channel_id, name, parameters)
SELECT
    id,
    'desktop',
    ''
FROM
    channels;

INSERT INTO
    outputs (channel_id, name, parameters)
SELECT
    id,
    'null',
    '-f null -'
FROM
    channels;

UPDATE configurations
SET
    output_id = (
        SELECT
            outputs.id
        FROM
            outputs
        WHERE
            outputs.name = configurations.output_mode_old
            AND outputs.channel_id = configurations.channel_id
    )
WHERE
    output_mode_old IS NOT NULL;

UPDATE outputs
SET
    parameters = (
        SELECT
            configurations.output_param
        FROM
            configurations
        WHERE
            configurations.output_id = outputs.id
            AND configurations.channel_id = outputs.channel_id
        LIMIT
            1
    )
WHERE
    EXISTS (
        SELECT
            1
        FROM
            configurations
        WHERE
            configurations.output_id = outputs.id
            AND configurations.channel_id = outputs.channel_id
    );

ALTER TABLE configurations
DROP COLUMN output_mode_old;

ALTER TABLE configurations
DROP COLUMN output_param;
