-- Stats queries ask "when was this uri first played after time T" for every
-- queued uri; the (uri, type, time) index turns that from a scan into a seek.
CREATE INDEX IF NOT EXISTS idx_psl_uri_type_ts
    ON playback_state_log(track_uri, event_type, timestamp_ms);

-- Range scans of one event type ordered by time (per-day, per-hour, sessions).
CREATE INDEX IF NOT EXISTS idx_psl_type_ts
    ON playback_state_log(event_type, timestamp_ms);

CREATE INDEX IF NOT EXISTS idx_qe_type_ts
    ON queue_events(event_type, timestamp_ms);

-- A conversion row is one add (not one per uri); uri_count says how many tracks it carried.
ALTER TABLE search_conversions ADD COLUMN uri_count INTEGER NOT NULL DEFAULT 1;
CREATE INDEX IF NOT EXISTS idx_se_type_ts ON search_events(event_type, timestamp_ms);
