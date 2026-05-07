UPDATE channels
SET
    preview_url = 'http://127.0.0.1:8787/public/1/live/stream.m3u8'
WHERE
    preview_url = 'http://127.0.0.1:8787/1/live/stream.m3u8';
