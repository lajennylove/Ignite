ALTER TABLE sessions DROP FOREIGN KEY sessions_user_fk;

ALTER TABLE users DROP COLUMN disabled_at;
