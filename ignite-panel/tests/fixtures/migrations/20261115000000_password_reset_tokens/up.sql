-- Password reset links, one row for each email with a link out. The token is stored as a
-- hash, as Laravel stores it, so a copy of the table cannot reset a password. A new link
-- replaces the row, and a used or expired link is deleted.
CREATE TABLE IF NOT EXISTS password_reset_tokens (
    email VARCHAR(255) NOT NULL PRIMARY KEY,
    token VARCHAR(255) NOT NULL,
    created_at DATETIME(6) NOT NULL
);
