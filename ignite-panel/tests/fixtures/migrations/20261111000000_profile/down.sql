DROP TABLE IF EXISTS user_avatars;

ALTER TABLE users
    DROP COLUMN avatar_mime,
    DROP COLUMN profile;
