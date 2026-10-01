ALTER TABLE config_output
ADD COLUMN metadata_options TEXT NOT NULL DEFAULT '{}';

ALTER TABLE config_live_input
ADD COLUMN demuxer_options TEXT NOT NULL DEFAULT '{}';

-- Listener identity and priority now arbitrate multiple inputs per channel.
DROP INDEX config_live_input_rtmp_listener;

-- Migration 4 allowed arbitrary non-negative priorities. Preserve their order
-- as far as possible while bringing them into the new 0–100 range.
UPDATE config_live_input
SET
    priority = 100
WHERE
    priority > 100;

-- The listener migrated from the former single-RTMP setting wins over new
-- listeners by default. Users may subsequently change its priority.
UPDATE config_live_input
SET
    priority = 100
WHERE
    backend = 'rtmp'
    AND takeover_mode = 'connection'
    AND priority = 0;

CREATE TRIGGER config_live_input_priority_insert
BEFORE INSERT ON config_live_input WHEN NEW.priority > 100
BEGIN
SELECT
    RAISE (
        ABORT,
        'live input priority must be between 0 and 100'
    );

END;

CREATE TRIGGER config_live_input_priority_update
BEFORE UPDATE OF priority ON config_live_input WHEN NEW.priority > 100
BEGIN
SELECT
    RAISE (
        ABORT,
        'live input priority must be between 0 and 100'
    );

END;

CREATE INDEX config_live_input_channel_order ON config_live_input (config_id, priority DESC, id);

-- Multiple audio tracks may share an output, but only one can be its default.
CREATE UNIQUE INDEX config_output_audio_one_default
ON config_output_audio (output_id)
WHERE default_track = 1;
