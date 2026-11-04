ALTER TABLE config_live_input
ADD COLUMN auto_takeover INTEGER NOT NULL DEFAULT 1 CHECK (auto_takeover IN (0, 1));

-- Persistent manual program graphics.
CREATE TABLE config_graphics_state (
    channel_id INTEGER PRIMARY KEY REFERENCES channels (id) ON DELETE CASCADE,
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0)
);

CREATE TABLE config_graphics (
    channel_id INTEGER NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
    layer_id INTEGER NOT NULL CHECK (layer_id BETWEEN 0 AND 9007199254740991),
    name TEXT NOT NULL DEFAULT '',
    settings TEXT NOT NULL CHECK (json_valid (settings)),
    running INTEGER NOT NULL DEFAULT 0 CHECK (running IN (0, 1)),
    visible INTEGER NOT NULL DEFAULT 1 CHECK (visible IN (0, 1)),
    PRIMARY KEY (channel_id, layer_id)
);
