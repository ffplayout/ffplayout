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

ALTER TABLE auth_user ADD COLUMN access_version INTEGER NOT NULL DEFAULT 0;

CREATE TRIGGER auth_user_access_version_on_insert
AFTER INSERT ON auth_user
BEGIN
    UPDATE auth_user SET access_version = random() WHERE id = NEW.id;
END;
