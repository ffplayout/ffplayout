ALTER TABLE configurations ADD processing_override_filter INTEGER NOT NULL DEFAULT 0;
ALTER TABLE channels ADD advanced_id INTEGER REFERENCES advanced_configurations (id) ON UPDATE CASCADE ON DELETE SET DEFAULT;

ALTER TABLE advanced_configurations
DROP filter_pad_scale_w;

ALTER TABLE advanced_configurations
DROP filter_pad_scale_h;

ALTER TABLE advanced_configurations ADD name TEXT;

UPDATE advanced_configurations SET name = 'None';
