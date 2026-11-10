-- Lets the panel delete the rows whose window has ended without scanning the table.
CREATE INDEX login_throttles_window_start_index ON login_throttles (window_start);
