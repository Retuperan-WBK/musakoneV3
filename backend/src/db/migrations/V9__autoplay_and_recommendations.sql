-- Autoplay + recommendation engine support.

-- Runtime-mutable settings (autoplay switches etc.), edited from the app.
CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);

INSERT OR IGNORE INTO settings (key, value, updated_ms) VALUES
    ('autoplay.enabled',    '0',    0),
    ('autoplay.min_ahead',  '2',    0),
    ('autoplay.batch_size', '5',    0),
    ('autoplay.discovery',  '0.5',  0),
    ('autoplay.mode',       'room', 0);

-- Artist name -> Mopidy artist URI, learned from what actually plays.
-- Affinity tables only know artist *names*; browsing an artist's catalogue needs the URI.
CREATE TABLE IF NOT EXISTS artist_index (
    artist_name TEXT PRIMARY KEY,
    artist_uri  TEXT NOT NULL,
    updated_ms  INTEGER NOT NULL
);

-- Cache for slow Mopidy/Tidal browse calls made by the recommender.
CREATE TABLE IF NOT EXISTS rpc_cache (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_qe_timestamp ON queue_events(timestamp_ms DESC);
CREATE INDEX IF NOT EXISTS idx_psl_type_ts ON playback_state_log(event_type, timestamp_ms DESC);

-- System account that owns autoplay's queue additions so they show up in the
-- activity log like any other user's. It cannot log in: passwords are stored as
-- SHA-256 hex, which never equals '!system'.
INSERT OR IGNORE INTO users (username, password_hash, created_at)
VALUES ('autoplay', '!system', strftime('%s', 'now'));
