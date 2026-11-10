-- Each user's colour theme. Every user starts on the light theme, and the
-- panel's toggle changes it.
ALTER TABLE users ADD COLUMN theme VARCHAR(8) NOT NULL DEFAULT 'light';
