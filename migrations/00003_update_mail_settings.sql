ALTER TABLE global RENAME COLUMN mail_smtp TO smtp_server;
ALTER TABLE global RENAME COLUMN mail_user TO smtp_user;
ALTER TABLE global RENAME COLUMN mail_password TO smtp_password;
ALTER TABLE global RENAME COLUMN mail_starttls TO smtp_starttls;
ALTER TABLE global ADD smtp_port INTEGER NOT NULL DEFAULT 465;
