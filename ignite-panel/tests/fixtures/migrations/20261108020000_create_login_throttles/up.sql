-- Failed sign-in attempts, counted per key (an email or an IP) in a window.
-- The table lives in the database, so the count holds across restarts and processes.
CREATE TABLE IF NOT EXISTS login_throttles (
    throttle_key VARCHAR(255) NOT NULL PRIMARY KEY,
    hits INT NOT NULL,
    window_start DATETIME NOT NULL
);
