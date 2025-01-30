ALTER TABLE channels ADD advanced_id INTEGER REFERENCES advanced_configurations(id)
    ON UPDATE CASCADE
    ON DELETE SET DEFAULT;

ALTER TABLE advanced_configurations DROP filter_pad_scale_w;
ALTER TABLE advanced_configurations DROP filter_pad_scale_h;
ALTER TABLE advanced_configurations ADD name TEXT;
