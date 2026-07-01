ALTER TABLE outputs
ADD COLUMN hls_variants TEXT NOT NULL DEFAULT '';

-- Dedicated output fields replacing values previously parsed from the
-- free-text ffmpeg command in `parameters`.
ALTER TABLE outputs
ADD COLUMN stream_url TEXT NOT NULL DEFAULT '';

ALTER TABLE outputs
ADD COLUMN hls_playlist_path TEXT DEFAULT NULL;

ALTER TABLE outputs
ADD COLUMN hls_segment_duration INTEGER DEFAULT NULL;

ALTER TABLE outputs
ADD COLUMN hls_list_size INTEGER DEFAULT NULL;

UPDATE outputs
SET
  hls_playlist_path = 'live/stream.m3u8',
  hls_segment_duration = 6,
  hls_list_size = 600
WHERE
  name = 'hls';

-- The old command always placed the output target last. Extract it once so
-- customized stream and HLS presets survive the schema upgrade.
UPDATE outputs
SET
  stream_url = (
    WITH RECURSIVE
      tokens (rest, token) AS (
        SELECT
          trim(parameters) || ' ',
          ''
        UNION ALL
        SELECT
          ltrim(substr(rest, instr (rest, ' ') + 1)),
          substr(rest, 1, instr (rest, ' ') - 1)
        FROM
          tokens
        WHERE
          rest <> ''
      )
    SELECT
      token
    FROM
      tokens
    WHERE
      rest = ''
      AND token <> ''
    LIMIT
      1
  )
WHERE
  name = 'stream'
  AND trim(parameters) <> '';

UPDATE outputs
SET
  hls_playlist_path = (
    WITH RECURSIVE
      tokens (rest, token) AS (
        SELECT
          trim(parameters) || ' ',
          ''
        UNION ALL
        SELECT
          ltrim(substr(rest, instr (rest, ' ') + 1)),
          substr(rest, 1, instr (rest, ' ') - 1)
        FROM
          tokens
        WHERE
          rest <> ''
      )
    SELECT
      token
    FROM
      tokens
    WHERE
      rest = ''
      AND token <> ''
    LIMIT
      1
  )
WHERE
  name = 'hls'
  AND trim(parameters) <> '';

UPDATE outputs
SET
  hls_segment_duration = CAST(
    substr(
      ltrim(
        substr(
          parameters,
          instr (parameters, '-hls_time') + length('-hls_time')
        )
      ),
      1,
      instr (
        ltrim(
          substr(
            parameters,
            instr (parameters, '-hls_time') + length('-hls_time')
          )
        ),
        ' '
      ) - 1
    ) AS INTEGER
  )
WHERE
  name = 'hls'
  AND instr (parameters, '-hls_time') > 0;

UPDATE outputs
SET
  hls_list_size = CAST(
    substr(
      ltrim(
        substr(
          parameters,
          instr (parameters, '-hls_list_size') + length('-hls_list_size')
        )
      ),
      1,
      instr (
        ltrim(
          substr(
            parameters,
            instr (parameters, '-hls_list_size') + length('-hls_list_size')
          )
        ),
        ' '
      ) - 1
    ) AS INTEGER
  )
WHERE
  name = 'hls'
  AND instr (parameters, '-hls_list_size') > 0;
