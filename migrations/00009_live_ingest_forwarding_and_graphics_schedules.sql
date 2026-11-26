ALTER TABLE config_output ADD COLUMN forward_listener_id INTEGER
    REFERENCES config_live_input(id) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE config_output ADD COLUMN forward_mode TEXT NOT NULL DEFAULT 'auto'
    CHECK (forward_mode IN ('auto', 'copy', 'transcode'));

CREATE TABLE config_graphics_schedule (
    channel_id INTEGER NOT NULL,
    layer_id INTEGER NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    appointment TEXT NOT NULL CHECK (json_valid(appointment) AND json_type(appointment) = 'object'),
    PRIMARY KEY (channel_id, layer_id, position),
    FOREIGN KEY (channel_id, layer_id)
        REFERENCES config_graphics(channel_id, layer_id) ON DELETE CASCADE
);
