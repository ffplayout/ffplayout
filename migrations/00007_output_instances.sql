-- Preserve the previous selection while allowing several active instances.
DROP INDEX config_output_one_active;

ALTER TABLE config_output
ADD COLUMN priority INTEGER NOT NULL DEFAULT 1;

UPDATE config_output
SET
    priority = 0
WHERE
    active = 1;

CREATE INDEX config_output_active ON config_output (config_id, active, priority, id);

-- Keep configured, disabled instances separate from unused legacy presets.
ALTER TABLE config_output
ADD COLUMN configured BOOLEAN NOT NULL DEFAULT 0;

ALTER TABLE config_output
ADD COLUMN label TEXT NOT NULL DEFAULT '';

ALTER TABLE config_output
ADD COLUMN inherit_resolution BOOLEAN NOT NULL DEFAULT 0;

UPDATE config_output
SET
    configured = 1
WHERE
    active = 1;

ALTER TABLE config
ADD COLUMN program_format TEXT;

ALTER TABLE config_output
ADD COLUMN share_encoding BOOLEAN NOT NULL DEFAULT 1;

ALTER TABLE config_output
ADD COLUMN encoding_source_id INTEGER;

ALTER TABLE config_output
ADD COLUMN encoding_variant TEXT NOT NULL DEFAULT '';

-- Public MoQ playback is an explicit per-output opt-in.
ALTER TABLE config_output
ADD COLUMN public_moq BOOLEAN NOT NULL DEFAULT 0;
