-- The profile: the picture's type (NULL when the user keeps their initials), and the
-- values of the fields the app adds, as a JSON object. The picture itself is a file in the
-- panel's storage directory, as WordPress keeps uploads on disk: its own table keeps the
-- file's path, relative to the storage directory (such as `avatars/42.png`), and its type.
ALTER TABLE users
    ADD COLUMN avatar_mime VARCHAR(32) NULL,
    ADD COLUMN profile TEXT NULL;

CREATE TABLE IF NOT EXISTS user_avatars (
    user_id BIGINT NOT NULL PRIMARY KEY,
    path VARCHAR(255) NOT NULL,
    mime VARCHAR(32) NOT NULL,
    CONSTRAINT user_avatars_user_fk FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);
