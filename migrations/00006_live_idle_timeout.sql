ALTER TABLE config_live_input ADD COLUMN idle_timeout_ms INTEGER NOT NULL DEFAULT 2000
    CHECK (idle_timeout_ms BETWEEN 1 AND 4294967295);
