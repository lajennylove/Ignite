-- A disabled user cannot sign in, and their sessions end. NULL means the user is enabled.
ALTER TABLE users ADD COLUMN disabled_at DATETIME NULL;

-- Deleting a user deletes their sessions with them. Rows left by an earlier delete have no
-- user to belong to, so they go before the key is added.
DELETE FROM sessions WHERE user_id NOT IN (SELECT id FROM users);
ALTER TABLE sessions
    ADD CONSTRAINT sessions_user_fk FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE;
