-- Server-side sign-in sessions. The signed cookie holds only the id.
CREATE TABLE IF NOT EXISTS sessions (
    id VARCHAR(64) NOT NULL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    expires_at DATETIME NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    INDEX sessions_user_index (user_id)
);
