ALTER TABLE config_output ADD COLUMN muxer_options TEXT NOT NULL DEFAULT '{}';
ALTER TABLE config_output ADD COLUMN audio_options TEXT NOT NULL DEFAULT '{}';
ALTER TABLE config_output ADD COLUMN protocol_options TEXT NOT NULL DEFAULT '{}';
ALTER TABLE config_output ADD COLUMN device_backend TEXT NOT NULL DEFAULT '';
ALTER TABLE config_output ADD COLUMN device_identifier TEXT NOT NULL DEFAULT '';
ALTER TABLE config_output ADD COLUMN device_options TEXT NOT NULL DEFAULT '{}';

ALTER TABLE config_audio ADD COLUMN program_layout TEXT NOT NULL DEFAULT 'stereo';
ALTER TABLE config_recording ADD COLUMN audio_options TEXT NOT NULL DEFAULT '{}';

CREATE TABLE config_source (
    config_id INTEGER PRIMARY KEY,
    demuxer_options TEXT NOT NULL DEFAULT '{}',
    protocol_options TEXT NOT NULL DEFAULT '{}',
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO config_source(config_id)
SELECT id FROM config;

-- Live inputs describe selectable source backends and their playout takeover
-- policy. The former RTMP-only config_ingest row is migrated below.
CREATE TABLE config_live_input (
    id INTEGER PRIMARY KEY,
    config_id INTEGER NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0 CHECK (priority >= 0),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    name TEXT NOT NULL DEFAULT '',
    backend TEXT NOT NULL,
    identifier TEXT NOT NULL DEFAULT '',
    options TEXT NOT NULL DEFAULT '{}',
    takeover_mode TEXT NOT NULL DEFAULT 'manual'
        CHECK (takeover_mode IN ('manual', 'external_trigger', 'signal_presence', 'connection', 'duration')),
    signal_loss_grace_seconds REAL NOT NULL DEFAULT 5.0 CHECK (signal_loss_grace_seconds >= 0),
    max_duration_seconds REAL NOT NULL DEFAULT 0 CHECK (max_duration_seconds >= 0),
    FOREIGN KEY (config_id) REFERENCES config(id) ON UPDATE CASCADE ON DELETE CASCADE
);

INSERT INTO config_live_input (
    config_id, priority, enabled, backend, identifier, takeover_mode
)
SELECT config_id, 0, enable, 'rtmp', url, 'connection'
FROM config_ingest;

CREATE UNIQUE INDEX config_live_input_rtmp_listener
ON config_live_input(config_id)
WHERE backend = 'rtmp' AND takeover_mode = 'connection';

DROP TABLE config_ingest;

CREATE TABLE config_output_audio (
    id INTEGER PRIMARY KEY,
    output_id INTEGER NOT NULL,
    position INTEGER NOT NULL,
    source TEXT NOT NULL DEFAULT 'program',
    source_index INTEGER NOT NULL DEFAULT 0,
    title TEXT NOT NULL DEFAULT '',
    language TEXT NOT NULL DEFAULT '',
    default_track INTEGER NOT NULL DEFAULT 0 CHECK (default_track IN (0, 1)),
    channel_layout TEXT NOT NULL DEFAULT 'stereo',
    codec TEXT,
    bitrate INTEGER,
    encoder_options TEXT NOT NULL DEFAULT '{}',
    channel_map TEXT NOT NULL DEFAULT '{}',
    FOREIGN KEY (output_id) REFERENCES config_output(id) ON UPDATE CASCADE ON DELETE CASCADE,
    UNIQUE (output_id, position)
);
