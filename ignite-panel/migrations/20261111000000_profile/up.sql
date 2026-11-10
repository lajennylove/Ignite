-- The profile: the picture's type (NULL when the user keeps their initials), and the
-- values of the fields the app adds, as a JSON object. The picture bytes live in their
-- own table, so the row the panel reads on every request stays small.
ALTER TABLE users
    ADD COLUMN avatar_mime VARCHAR(32) NULL,
    ADD COLUMN profile TEXT NULL;

CREATE TABLE IF NOT EXISTS user_avatars (
    user_id BIGINT NOT NULL PRIMARY KEY,
    data MEDIUMBLOB NOT NULL,
    CONSTRAINT user_avatars_user_fk FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);
