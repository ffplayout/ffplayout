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
CREATE UNIQUE INDEX config_output_audio_one_default ON config_output_audio (output_id)
WHERE
    default_track = 1;

-- Preserve the source selection of existing installations; new rows normalize all sources.
ALTER TABLE config_audio
ADD COLUMN loudness_scope TEXT NOT NULL DEFAULT 'all' CHECK (loudness_scope IN ('all', 'live', 'off'));

UPDATE config_audio
SET
    loudness_scope = CASE
        WHEN live_loudness_enable = 1 THEN 'live'
        ELSE 'off'
    END;

ALTER TABLE config_audio
ADD COLUMN compressor_ratio REAL NOT NULL DEFAULT 3.0;

ALTER TABLE config_audio
ADD COLUMN compressor_threshold_dbfs REAL NOT NULL DEFAULT -26.0;

ALTER TABLE config_audio
ADD COLUMN pause_threshold_dbfs REAL NOT NULL DEFAULT -55.0;

-- Loudness processing now applies to the selected program sources.
ALTER TABLE config_audio
RENAME COLUMN live_loudness_enable TO loudness_enable;

ALTER TABLE config_audio
RENAME COLUMN live_loudness_target_lufs TO loudness_target_lufs;

ALTER TABLE config_audio
RENAME COLUMN live_loudness_dead_band_lu TO loudness_dead_band_lu;

ALTER TABLE config_audio
RENAME COLUMN live_loudness_max_gain_db TO loudness_max_gain_db;

ALTER TABLE config_audio
RENAME COLUMN live_loudness_max_attenuation_db TO loudness_max_attenuation_db;

ALTER TABLE config_audio
RENAME COLUMN live_loudness_gain_up_db_per_second TO loudness_gain_up_db_per_second;

ALTER TABLE config_audio
RENAME COLUMN live_loudness_gain_down_db_per_second TO loudness_gain_down_db_per_second;

ALTER TABLE config_audio
RENAME COLUMN live_loudness_silence_gate_lufs TO loudness_silence_gate_lufs;

ALTER TABLE config_audio
RENAME COLUMN live_loudness_true_peak_ceiling_dbtp TO loudness_true_peak_ceiling_dbtp;

ALTER TABLE config_audio
ADD COLUMN compressor_attack_ms REAL NOT NULL DEFAULT 5.0;

ALTER TABLE config_audio
ADD COLUMN compressor_hold_ms REAL NOT NULL DEFAULT 100.0;

ALTER TABLE config_audio
ADD COLUMN compressor_release_ms REAL NOT NULL DEFAULT 1200.0;

ALTER TABLE config_audio
ADD COLUMN compressor_strong_release_ms REAL NOT NULL DEFAULT 500.0;

ALTER TABLE config_audio
ADD COLUMN compressor_knee_db REAL NOT NULL DEFAULT 6.0;

ALTER TABLE config_audio
ADD COLUMN pause_hold_ms REAL NOT NULL DEFAULT 300.0;

ALTER TABLE config_audio
ADD COLUMN pause_return_delay_ms REAL NOT NULL DEFAULT 2000.0;

ALTER TABLE config_audio
ADD COLUMN loudness_output_max_correction_db REAL NOT NULL DEFAULT 3.0;

ALTER TABLE config_audio
ADD COLUMN loudness_output_gain_up_db_per_second REAL NOT NULL DEFAULT 0.1;

ALTER TABLE config_audio
ADD COLUMN loudness_output_gain_down_db_per_second REAL NOT NULL DEFAULT 0.25;
