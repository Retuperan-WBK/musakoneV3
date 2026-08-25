import gleam/dynamic/decode
import gleam/option.{type Option}
import gleam/result
import gleam/string
import gleam/list
import sqlight

pub type User {
  User(id: Int, username: String, created_at: Int, last_login: Option(Int))
}

/// Get user by username and password hash
pub fn get_user_by_username_and_password(
  db: sqlight.Connection,
  username: String,
  password_hash: String,
) -> Result(List(User), sqlight.Error) {
  let sql =
    "SELECT id, username, created_at, last_login
FROM users
WHERE username = ? AND password_hash = ?"

  sqlight.query(
    sql,
    db,
    [sqlight.text(username), sqlight.text(password_hash)],
    user_decoder(),
  )
}

/// Get user by username only
pub fn get_user_by_username(
  db: sqlight.Connection,
  username: String,
) -> Result(List(User), sqlight.Error) {
  let sql =
    "SELECT id, username, created_at, last_login
FROM users
WHERE username = ?"

  sqlight.query(sql, db, [sqlight.text(username)], user_decoder())
}

/// Get user by ID
pub fn get_user_by_id(
  db: sqlight.Connection,
  user_id: Int,
) -> Result(List(User), sqlight.Error) {
  let sql =
    "SELECT id, username, created_at, last_login
FROM users
WHERE id = ?"

  sqlight.query(sql, db, [sqlight.int(user_id)], user_decoder())
}

/// Create a new user
pub fn create_user(
  db: sqlight.Connection,
  username: String,
  password_hash: String,
  created_at: Int,
) -> Result(List(User), sqlight.Error) {
  let sql =
    "INSERT INTO users (username, password_hash, created_at)
VALUES (?, ?, ?)
RETURNING id, username, created_at, last_login"

  sqlight.query(
    sql,
    db,
    [
      sqlight.text(username),
      sqlight.text(password_hash),
      sqlight.int(created_at),
    ],
    user_decoder(),
  )
}

/// Update user's last login timestamp
pub fn update_last_login(
  db: sqlight.Connection,
  user_id: Int,
  last_login: Option(Int),
) -> Result(Nil, sqlight.Error) {
  let sql =
    "UPDATE users
SET last_login = ?
WHERE id = ?"

  sqlight.query(
    sql,
    db,
    [sqlight.nullable(sqlight.int, last_login), sqlight.int(user_id)],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

fn user_decoder() -> decode.Decoder(User) {
  use id <- decode.field(0, decode.int)
  use username <- decode.field(1, decode.string)
  use created_at <- decode.field(2, decode.int)
  use last_login <- decode.field(3, decode.optional(decode.int))
  decode.success(User(id:, username:, created_at:, last_login:))
}

// ============================================================================
// PLAYBACK EVENT TRACKING
// ============================================================================

pub type PlaybackEvent {
  PlaybackEvent(
    id: Int,
    user_id: Int,
    timestamp_ms: Int,
    event_type: String,
    track_uri: Option(String),
    track_name: Option(String),
    artist_name: Option(String),
    album_name: Option(String),
    track_duration_ms: Option(Int),
    position_ms: Option(Int),
    seek_to_ms: Option(Int),
    volume_level: Option(Int),
    playback_flags: Option(String),
  )
}

/// Insert a playback event
pub fn log_playback_event(
  db: sqlight.Connection,
  user_id: Int,
  timestamp_ms: Int,
  event_type: String,
  track_uri: Option(String),
  track_name: Option(String),
  artist_name: Option(String),
  album_name: Option(String),
  track_duration_ms: Option(Int),
  position_ms: Option(Int),
  seek_to_ms: Option(Int),
  volume_level: Option(Int),
  playback_flags: Option(String),
) -> Result(Nil, sqlight.Error) {
  let sql =
    "INSERT INTO playback_events
     (user_id, timestamp_ms, event_type, track_uri, track_name,
      artist_name, album_name, track_duration_ms, position_ms,
      seek_to_ms, volume_level, playback_flags)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.int(timestamp_ms),
      sqlight.text(event_type),
      sqlight.nullable(sqlight.text, track_uri),
      sqlight.nullable(sqlight.text, track_name),
      sqlight.nullable(sqlight.text, artist_name),
      sqlight.nullable(sqlight.text, album_name),
      sqlight.nullable(sqlight.int, track_duration_ms),
      sqlight.nullable(sqlight.int, position_ms),
      sqlight.nullable(sqlight.int, seek_to_ms),
      sqlight.nullable(sqlight.int, volume_level),
      sqlight.nullable(sqlight.text, playback_flags),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get playback events for a user (newest first)
pub fn get_playback_events(
  db: sqlight.Connection,
  user_id: Int,
  limit: Int,
) -> Result(List(PlaybackEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       track_uri, track_name, artist_name, album_name, track_duration_ms,
       position_ms, seek_to_ms, volume_level, playback_flags
     FROM playback_events
     WHERE user_id = ?
     ORDER BY timestamp_ms DESC
     LIMIT ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(user_id), sqlight.int(limit)],
    playback_event_decoder(),
  )
}

/// Export all playback events for ML training (chronological, paginated)
pub fn export_playback_events(
  db: sqlight.Connection,
  offset: Int,
  limit: Int,
) -> Result(List(PlaybackEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       track_uri, track_name, artist_name, album_name, track_duration_ms,
       position_ms, seek_to_ms, volume_level, playback_flags
     FROM playback_events
     ORDER BY timestamp_ms ASC
     LIMIT ? OFFSET ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(limit), sqlight.int(offset)],
    playback_event_decoder(),
  )
}

fn playback_event_decoder() -> decode.Decoder(PlaybackEvent) {
  use id <- decode.field(0, decode.int)
  use user_id <- decode.field(1, decode.int)
  use timestamp_ms <- decode.field(2, decode.int)
  use event_type <- decode.field(3, decode.string)
  use track_uri <- decode.field(4, decode.optional(decode.string))
  use track_name <- decode.field(5, decode.optional(decode.string))
  use artist_name <- decode.field(6, decode.optional(decode.string))
  use album_name <- decode.field(7, decode.optional(decode.string))
  use track_duration_ms <- decode.field(8, decode.optional(decode.int))
  use position_ms <- decode.field(9, decode.optional(decode.int))
  use seek_to_ms <- decode.field(10, decode.optional(decode.int))
  use volume_level <- decode.field(11, decode.optional(decode.int))
  use playback_flags <- decode.field(12, decode.optional(decode.string))
  decode.success(PlaybackEvent(
    id:,
    user_id:,
    timestamp_ms:,
    event_type:,
    track_uri:,
    track_name:,
    artist_name:,
    album_name:,
    track_duration_ms:,
    position_ms:,
    seek_to_ms:,
    volume_level:,
    playback_flags:,
  ))
}

// ============================================================================
// QUEUE EVENT TRACKING
// ============================================================================

pub type QueueEvent {
  QueueEvent(
    id: Int,
    user_id: Int,
    timestamp_ms: Int,
    event_type: String,
    track_uris: Option(String),
    track_names: Option(String),
    at_position: Option(Int),
    from_position: Option(Int),
    to_position: Option(Int),
    queue_length: Option(Int),
  )
}

/// Insert a queue event
pub fn log_queue_event(
  db: sqlight.Connection,
  user_id: Int,
  timestamp_ms: Int,
  event_type: String,
  track_uris: Option(String),
  track_names: Option(String),
  at_position: Option(Int),
  from_position: Option(Int),
  to_position: Option(Int),
  queue_length: Option(Int),
) -> Result(Nil, sqlight.Error) {
  let sql =
    "INSERT INTO queue_events
     (user_id, timestamp_ms, event_type, track_uris, track_names,
      at_position, from_position, to_position, queue_length)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.int(timestamp_ms),
      sqlight.text(event_type),
      sqlight.nullable(sqlight.text, track_uris),
      sqlight.nullable(sqlight.text, track_names),
      sqlight.nullable(sqlight.int, at_position),
      sqlight.nullable(sqlight.int, from_position),
      sqlight.nullable(sqlight.int, to_position),
      sqlight.nullable(sqlight.int, queue_length),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get queue events for a user
pub fn get_queue_events(
  db: sqlight.Connection,
  user_id: Int,
  limit: Int,
) -> Result(List(QueueEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       track_uris, track_names, at_position,
       from_position, to_position, queue_length
     FROM queue_events
     WHERE user_id = ?
     ORDER BY timestamp_ms DESC
     LIMIT ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(user_id), sqlight.int(limit)],
    queue_event_decoder(),
  )
}

/// Export all queue events for ML training
pub fn export_queue_events(
  db: sqlight.Connection,
  offset: Int,
  limit: Int,
) -> Result(List(QueueEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       track_uris, track_names, at_position,
       from_position, to_position, queue_length
     FROM queue_events
     ORDER BY timestamp_ms ASC
     LIMIT ? OFFSET ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(limit), sqlight.int(offset)],
    queue_event_decoder(),
  )
}

fn queue_event_decoder() -> decode.Decoder(QueueEvent) {
  use id <- decode.field(0, decode.int)
  use user_id <- decode.field(1, decode.int)
  use timestamp_ms <- decode.field(2, decode.int)
  use event_type <- decode.field(3, decode.string)
  use track_uris <- decode.field(4, decode.optional(decode.string))
  use track_names <- decode.field(5, decode.optional(decode.string))
  use at_position <- decode.field(6, decode.optional(decode.int))
  use from_position <- decode.field(7, decode.optional(decode.int))
  use to_position <- decode.field(8, decode.optional(decode.int))
  use queue_length <- decode.field(9, decode.optional(decode.int))
  decode.success(QueueEvent(
    id:,
    user_id:,
    timestamp_ms:,
    event_type:,
    track_uris:,
    track_names:,
    at_position:,
    from_position:,
    to_position:,
    queue_length:,
  ))
}

// ============================================================================
// SEARCH EVENT TRACKING
// ============================================================================

pub type SearchEvent {
  SearchEvent(
    id: Int,
    user_id: Int,
    timestamp_ms: Int,
    event_type: String,
    query_text: Option(String),
    browse_uri: Option(String),
    result_count: Option(Int),
  )
}

/// Insert a search/browse event
pub fn log_search_event(
  db: sqlight.Connection,
  user_id: Int,
  timestamp_ms: Int,
  event_type: String,
  query_text: Option(String),
  browse_uri: Option(String),
  result_count: Option(Int),
) -> Result(Nil, sqlight.Error) {
  let sql =
    "INSERT INTO search_events
     (user_id, timestamp_ms, event_type, query_text, browse_uri, result_count)
     VALUES (?, ?, ?, ?, ?, ?)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.int(timestamp_ms),
      sqlight.text(event_type),
      sqlight.nullable(sqlight.text, query_text),
      sqlight.nullable(sqlight.text, browse_uri),
      sqlight.nullable(sqlight.int, result_count),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get search events for a user
pub fn get_search_events(
  db: sqlight.Connection,
  user_id: Int,
  limit: Int,
) -> Result(List(SearchEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       query_text, browse_uri, result_count
     FROM search_events
     WHERE user_id = ?
     ORDER BY timestamp_ms DESC
     LIMIT ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(user_id), sqlight.int(limit)],
    search_event_decoder(),
  )
}

/// Export all search events for ML training
pub fn export_search_events(
  db: sqlight.Connection,
  offset: Int,
  limit: Int,
) -> Result(List(SearchEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       query_text, browse_uri, result_count
     FROM search_events
     ORDER BY timestamp_ms ASC
     LIMIT ? OFFSET ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(limit), sqlight.int(offset)],
    search_event_decoder(),
  )
}

fn search_event_decoder() -> decode.Decoder(SearchEvent) {
  use id <- decode.field(0, decode.int)
  use user_id <- decode.field(1, decode.int)
  use timestamp_ms <- decode.field(2, decode.int)
  use event_type <- decode.field(3, decode.string)
  use query_text <- decode.field(4, decode.optional(decode.string))
  use browse_uri <- decode.field(5, decode.optional(decode.string))
  use result_count <- decode.field(6, decode.optional(decode.int))
  decode.success(SearchEvent(
    id:,
    user_id:,
    timestamp_ms:,
    event_type:,
    query_text:,
    browse_uri:,
    result_count:,
  ))
}

// ============================================================================
// AGGREGATE STATS
// ============================================================================

/// Get per-table event counts for a user
pub fn get_user_stats(
  db: sqlight.Connection,
  user_id: Int,
) -> Result(List(#(String, Int)), sqlight.Error) {
  let sql =
    "SELECT 'playback' as category, COUNT(*) as count
       FROM playback_events WHERE user_id = ?
     UNION ALL
     SELECT 'queue', COUNT(*) FROM queue_events WHERE user_id = ?
     UNION ALL
     SELECT 'search', COUNT(*) FROM search_events WHERE user_id = ?"

  let decoder = {
    use category <- decode.field(0, decode.string)
    use count <- decode.field(1, decode.int)
    decode.success(#(category, count))
  }

  sqlight.query(
    sql,
    db,
    [sqlight.int(user_id), sqlight.int(user_id), sqlight.int(user_id)],
    decoder,
  )
}

/// Get total row counts across all tables (for ML export pagination)
pub fn get_event_counts(
  db: sqlight.Connection,
) -> Result(List(#(String, Int)), sqlight.Error) {
  let sql =
    "SELECT 'playback' as tbl, COUNT(*) as cnt FROM playback_events
     UNION ALL
     SELECT 'queue', COUNT(*) FROM queue_events
     UNION ALL
     SELECT 'search', COUNT(*) FROM search_events"

  let decoder = {
    use tbl <- decode.field(0, decode.string)
    use cnt <- decode.field(1, decode.int)
    decode.success(#(tbl, cnt))
  }

  sqlight.query(sql, db, [], decoder)
}

// ============================================================================
// RECENT EVENTS (for dashboard)
// ============================================================================

/// Get recent playback events since timestamp
pub fn get_recent_playback_events(
  db: sqlight.Connection,
  since_ms: Int,
) -> Result(List(PlaybackEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       track_uri, track_name, artist_name, album_name,
       track_duration_ms, position_ms, seek_to_ms, volume_level, playback_flags
     FROM playback_events
     WHERE timestamp_ms >= ?
     ORDER BY timestamp_ms DESC
     LIMIT 100"

  sqlight.query(sql, db, [sqlight.int(since_ms)], playback_event_decoder())
}

/// Get recent queue events since timestamp
pub fn get_recent_queue_events(
  db: sqlight.Connection,
  since_ms: Int,
) -> Result(List(QueueEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       track_uris, track_names, at_position, from_position, to_position, queue_length
     FROM queue_events
     WHERE timestamp_ms >= ?
     ORDER BY timestamp_ms DESC
     LIMIT 100"

  sqlight.query(sql, db, [sqlight.int(since_ms)], queue_event_decoder())
}

/// Get recent search events since timestamp
pub fn get_recent_search_events(
  db: sqlight.Connection,
  since_ms: Int,
) -> Result(List(SearchEvent), sqlight.Error) {
  let sql =
    "SELECT id, user_id, timestamp_ms, event_type,
       query_text, browse_uri, result_count
     FROM search_events
     WHERE timestamp_ms >= ?
     ORDER BY timestamp_ms DESC
     LIMIT 100"

  sqlight.query(sql, db, [sqlight.int(since_ms)], search_event_decoder())
}

// ============================================================================
// ADMIN ANALYTICS (all users)
// ============================================================================

/// Get user activity summary for admin dashboard
pub fn get_user_activity_summary(
  db: sqlight.Connection,
) -> Result(List(#(String, Int, Int, Int, Int)), sqlight.Error) {
  let sql =
    "SELECT
       u.username,
       COALESCE(SUM(e.kind = 'playback'), 0) as playback_events,
       COALESCE(SUM(e.kind = 'queue'), 0) as queue_events,
       COALESCE(SUM(e.kind = 'search'), 0) as search_events,
       COUNT(e.kind) as total_events
     FROM users u
     LEFT JOIN (
       SELECT user_id, 'playback' as kind FROM playback_events
       UNION ALL
       SELECT user_id, 'queue' FROM queue_events
       UNION ALL
       SELECT user_id, 'search' FROM search_events
     ) e ON e.user_id = u.id
     GROUP BY u.id, u.username
     ORDER BY total_events DESC"

  let decoder = {
    use username <- decode.field(0, decode.string)
    use playback <- decode.field(1, decode.int)
    use queue <- decode.field(2, decode.int)
    use search <- decode.field(3, decode.int)
    use total <- decode.field(4, decode.int)
    decode.success(#(username, playback, queue, search, total))
  }

  sqlight.query(sql, db, [], decoder)
}

/// Get system-wide event counts by hour for the last 24 hours
pub fn get_hourly_activity(
  db: sqlight.Connection,
) -> Result(List(#(Int, Int)), sqlight.Error) {
  let sql =
    "SELECT
       strftime('%H', datetime(timestamp_ms / 1000, 'unixepoch')) as hour,
       COUNT(*) as events
     FROM (
       SELECT timestamp_ms FROM playback_events
       UNION ALL
       SELECT timestamp_ms FROM queue_events
       UNION ALL
       SELECT timestamp_ms FROM search_events
     )
     WHERE timestamp_ms >= (strftime('%s', 'now') * 1000) - (24 * 60 * 60 * 1000)
     GROUP BY hour
     ORDER BY hour"

  let decoder = {
    use hour <- decode.field(0, decode.int)
    use events <- decode.field(1, decode.int)
    decode.success(#(hour, events))
  }

  sqlight.query(sql, db, [], decoder)
}

/// Get top tracks by aggregated affinity score across all users.
/// Uses the user_track_affinity table which factors in listen %,
/// queue adds, skips, playlist adds — not just raw play count.
pub fn get_popular_tracks(
  db: sqlight.Connection,
  limit: Int,
) -> Result(List(#(String, String, Float, Int)), sqlight.Error) {
  let sql =
    "SELECT
       COALESCE(
         (SELECT pe.track_name FROM playback_events pe
          WHERE pe.track_uri = agg.track_uri AND pe.track_name IS NOT NULL
          ORDER BY pe.timestamp_ms DESC LIMIT 1),
         'Unknown'
       ) as track,
       COALESCE(
         (SELECT pe.artist_name FROM playback_events pe
          WHERE pe.track_uri = agg.track_uri AND pe.artist_name IS NOT NULL
          ORDER BY pe.timestamp_ms DESC LIMIT 1),
         'Unknown'
       ) as artist,
       agg.total_score,
       agg.unique_users
     FROM (
       SELECT
         track_uri,
         ROUND(SUM(affinity_score), 1) as total_score,
         COUNT(DISTINCT user_id) as unique_users
       FROM user_track_affinity
       GROUP BY track_uri
       HAVING SUM(affinity_score) > 0
       ORDER BY total_score DESC
       LIMIT ?
     ) agg"

  let decoder = {
    use track <- decode.field(0, decode.string)
    use artist <- decode.field(1, decode.string)
    use score <- decode.field(2, decode.float)
    use users <- decode.field(3, decode.int)
    decode.success(#(track, artist, score, users))
  }

  sqlight.query(sql, db, [sqlight.int(limit)], decoder)
}

/// Get most searched terms
pub fn get_popular_searches(
  db: sqlight.Connection,
  limit: Int,
) -> Result(List(#(String, Int, Int)), sqlight.Error) {
  let sql =
    "SELECT
       query_text,
       COUNT(*) as search_count,
       COUNT(DISTINCT user_id) as unique_users
     FROM search_events
     WHERE event_type = 'search' AND query_text IS NOT NULL
     GROUP BY query_text
     ORDER BY search_count DESC
     LIMIT ?"

  let decoder = {
    use query <- decode.field(0, decode.string)
    use searches <- decode.field(1, decode.int)
    use users <- decode.field(2, decode.int)
    decode.success(#(query, searches, users))
  }

  sqlight.query(sql, db, [sqlight.int(limit)], decoder)
}

/// Get event type distribution
pub fn get_event_type_distribution(
  db: sqlight.Connection,
) -> Result(List(#(String, Int)), sqlight.Error) {
  let sql =
    "SELECT event_type, COUNT(*) as count
     FROM (
       SELECT event_type FROM playback_events
       UNION ALL
       SELECT event_type FROM queue_events
       UNION ALL
       SELECT event_type FROM search_events
     )
     GROUP BY event_type
     ORDER BY count DESC"

  let decoder = {
    use event_type <- decode.field(0, decode.string)
    use count <- decode.field(1, decode.int)
    decode.success(#(event_type, count))
  }

  sqlight.query(sql, db, [], decoder)
}

// ============================================================================
// PLAYBACK STATE LOG (global ground truth)
// ============================================================================

pub type PlaybackStateLogEntry {
  PlaybackStateLogEntry(
    id: Int,
    timestamp_ms: Int,
    event_type: String,
    track_uri: Option(String),
    track_name: Option(String),
    artist_name: Option(String),
    album_name: Option(String),
    track_duration_ms: Option(Int),
    position_ms: Option(Int),
    volume_level: Option(Int),
    queue_length: Option(Int),
    user_id: Option(Int),
  )
}

/// Insert a playback state change into the global log
pub fn log_playback_state_change(
  db: sqlight.Connection,
  timestamp_ms: Int,
  event_type: String,
  track_uri: Option(String),
  track_name: Option(String),
  artist_name: Option(String),
  album_name: Option(String),
  track_duration_ms: Option(Int),
  position_ms: Option(Int),
  volume_level: Option(Int),
  queue_length: Option(Int),
  user_id: Option(Int),
) -> Result(Nil, sqlight.Error) {
  let sql =
    "INSERT INTO playback_state_log
     (timestamp_ms, event_type, track_uri, track_name, artist_name,
      album_name, track_duration_ms, position_ms, volume_level,
      queue_length, user_id)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(timestamp_ms),
      sqlight.text(event_type),
      sqlight.nullable(sqlight.text, track_uri),
      sqlight.nullable(sqlight.text, track_name),
      sqlight.nullable(sqlight.text, artist_name),
      sqlight.nullable(sqlight.text, album_name),
      sqlight.nullable(sqlight.int, track_duration_ms),
      sqlight.nullable(sqlight.int, position_ms),
      sqlight.nullable(sqlight.int, volume_level),
      sqlight.nullable(sqlight.int, queue_length),
      sqlight.nullable(sqlight.int, user_id),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get playback state history (newest first)
pub fn get_playback_state_history(
  db: sqlight.Connection,
  limit: Int,
) -> Result(List(PlaybackStateLogEntry), sqlight.Error) {
  let sql =
    "SELECT id, timestamp_ms, event_type, track_uri, track_name,
       artist_name, album_name, track_duration_ms, position_ms,
       volume_level, queue_length, user_id
     FROM playback_state_log
     ORDER BY timestamp_ms DESC
     LIMIT ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(limit)],
    playback_state_log_decoder(),
  )
}

fn playback_state_log_decoder() -> decode.Decoder(PlaybackStateLogEntry) {
  use id <- decode.field(0, decode.int)
  use timestamp_ms <- decode.field(1, decode.int)
  use event_type <- decode.field(2, decode.string)
  use track_uri <- decode.field(3, decode.optional(decode.string))
  use track_name <- decode.field(4, decode.optional(decode.string))
  use artist_name <- decode.field(5, decode.optional(decode.string))
  use album_name <- decode.field(6, decode.optional(decode.string))
  use track_duration_ms <- decode.field(7, decode.optional(decode.int))
  use position_ms <- decode.field(8, decode.optional(decode.int))
  use volume_level <- decode.field(9, decode.optional(decode.int))
  use queue_length <- decode.field(10, decode.optional(decode.int))
  use user_id <- decode.field(11, decode.optional(decode.int))
  decode.success(PlaybackStateLogEntry(
    id:,
    timestamp_ms:,
    event_type:,
    track_uri:,
    track_name:,
    artist_name:,
    album_name:,
    track_duration_ms:,
    position_ms:,
    volume_level:,
    queue_length:,
    user_id:,
  ))
}

/// Get all users with their last activity
pub fn get_all_users_with_activity(
  db: sqlight.Connection,
) -> Result(List(#(Int, String, Option(Int), Int)), sqlight.Error) {
  let sql =
    "SELECT
       u.id,
       u.username,
       MAX(e.timestamp_ms) as last_activity,
       COUNT(e.id) as total_events
     FROM users u
     LEFT JOIN (
       SELECT user_id, id, timestamp_ms FROM playback_events
       UNION ALL
       SELECT user_id, id, timestamp_ms FROM queue_events
       UNION ALL
       SELECT user_id, id, timestamp_ms FROM search_events
     ) e ON u.id = e.user_id
     GROUP BY u.id, u.username
     ORDER BY last_activity DESC NULLS LAST"

  let decoder = {
    use id <- decode.field(0, decode.int)
    use username <- decode.field(1, decode.string)
    use last_activity <- decode.field(2, decode.optional(decode.int))
    use total_events <- decode.field(3, decode.int)
    decode.success(#(id, username, last_activity, total_events))
  }

  sqlight.query(sql, db, [], decoder)
}

// ============================================================================
// TRACK FEATURES
// ============================================================================

pub type TrackFeature {
  TrackFeature(
    uri: String,
    name: Option(String),
    artist_name: Option(String),
    album_name: Option(String),
    duration_ms: Option(Int),
    genre: Option(String),
    release_date: Option(String),
    musicbrainz_id: Option(String),
    track_no: Option(Int),
    disc_no: Option(Int),
    first_seen_ms: Int,
    play_count: Int,
    skip_count: Int,
  )
}

/// Upsert track features. On conflict, update metadata and increment
/// play_count or skip_count based on flags.
pub fn upsert_track_features(
  db: sqlight.Connection,
  uri: String,
  name: Option(String),
  artist_name: Option(String),
  album_name: Option(String),
  duration_ms: Option(Int),
  genre: Option(String),
  release_date: Option(String),
  musicbrainz_id: Option(String),
  track_no: Option(Int),
  disc_no: Option(Int),
  now_ms: Int,
  is_play: Bool,
  is_skip: Bool,
) -> Result(Nil, sqlight.Error) {
  let play_inc = case is_play {
    True -> 1
    False -> 0
  }
  let skip_inc = case is_skip {
    True -> 1
    False -> 0
  }

  let sql =
    "INSERT INTO track_features
     (uri, name, artist_name, album_name, duration_ms, genre, release_date,
      musicbrainz_id, track_no, disc_no, first_seen_ms, play_count, skip_count)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(uri) DO UPDATE SET
       name = COALESCE(excluded.name, track_features.name),
       artist_name = COALESCE(excluded.artist_name, track_features.artist_name),
       album_name = COALESCE(excluded.album_name, track_features.album_name),
       duration_ms = COALESCE(excluded.duration_ms, track_features.duration_ms),
       genre = COALESCE(excluded.genre, track_features.genre),
       release_date = COALESCE(excluded.release_date, track_features.release_date),
       musicbrainz_id = COALESCE(excluded.musicbrainz_id, track_features.musicbrainz_id),
       track_no = COALESCE(excluded.track_no, track_features.track_no),
       disc_no = COALESCE(excluded.disc_no, track_features.disc_no),
       play_count = track_features.play_count + excluded.play_count,
       skip_count = track_features.skip_count + excluded.skip_count"

  sqlight.query(
    sql,
    db,
    [
      sqlight.text(uri),
      sqlight.nullable(sqlight.text, name),
      sqlight.nullable(sqlight.text, artist_name),
      sqlight.nullable(sqlight.text, album_name),
      sqlight.nullable(sqlight.int, duration_ms),
      sqlight.nullable(sqlight.text, genre),
      sqlight.nullable(sqlight.text, release_date),
      sqlight.nullable(sqlight.text, musicbrainz_id),
      sqlight.nullable(sqlight.int, track_no),
      sqlight.nullable(sqlight.int, disc_no),
      sqlight.int(now_ms),
      sqlight.int(play_inc),
      sqlight.int(skip_inc),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get track features for a single track
pub fn get_track_features(
  db: sqlight.Connection,
  uri: String,
) -> Result(List(TrackFeature), sqlight.Error) {
  let sql =
    "SELECT uri, name, artist_name, album_name, duration_ms, genre,
       release_date, musicbrainz_id, track_no, disc_no,
       first_seen_ms, play_count, skip_count
     FROM track_features WHERE uri = ?"

  sqlight.query(sql, db, [sqlight.text(uri)], track_feature_decoder())
}

fn track_feature_decoder() -> decode.Decoder(TrackFeature) {
  use uri <- decode.field(0, decode.string)
  use name <- decode.field(1, decode.optional(decode.string))
  use artist_name <- decode.field(2, decode.optional(decode.string))
  use album_name <- decode.field(3, decode.optional(decode.string))
  use duration_ms <- decode.field(4, decode.optional(decode.int))
  use genre <- decode.field(5, decode.optional(decode.string))
  use release_date <- decode.field(6, decode.optional(decode.string))
  use musicbrainz_id <- decode.field(7, decode.optional(decode.string))
  use track_no <- decode.field(8, decode.optional(decode.int))
  use disc_no <- decode.field(9, decode.optional(decode.int))
  use first_seen_ms <- decode.field(10, decode.int)
  use play_count <- decode.field(11, decode.int)
  use skip_count <- decode.field(12, decode.int)
  decode.success(TrackFeature(
    uri:,
    name:,
    artist_name:,
    album_name:,
    duration_ms:,
    genre:,
    release_date:,
    musicbrainz_id:,
    track_no:,
    disc_no:,
    first_seen_ms:,
    play_count:,
    skip_count:,
  ))
}

// ============================================================================
// USER-TRACK AFFINITY
// ============================================================================

pub type UserTrackAffinity {
  UserTrackAffinity(
    user_id: Int,
    track_uri: String,
    play_count: Int,
    total_listen_ms: Int,
    avg_listen_pct: Float,
    queue_add_count: Int,
    queue_move_closer: Int,
    skip_count: Int,
    early_skip_count: Int,
    queue_remove_count: Int,
    playlist_add_count: Int,
    affinity_score: Float,
    last_interaction_ms: Int,
  )
}

/// Update track affinity from a listen event (track_ended).
/// Recomputes affinity_score atomically in SQL.
pub fn update_track_affinity_listen(
  db: sqlight.Connection,
  user_id: Int,
  track_uri: String,
  listen_ms: Int,
  listen_pct: Float,
  is_skip: Bool,
  is_early_skip: Bool,
  now_ms: Int,
) -> Result(Nil, sqlight.Error) {
  let play_inc = case is_skip {
    True -> 0
    False -> 1
  }
  let skip_inc = case is_skip {
    True -> 1
    False -> 0
  }
  let early_skip_inc = case is_early_skip {
    True -> 1
    False -> 0
  }

  // Upsert with atomic affinity_score recomputation.
  // avg_listen_pct uses incremental average: ((old_avg * old_count) + new) / new_count
  let sql =
    "INSERT INTO user_track_affinity
     (user_id, track_uri, play_count, total_listen_ms, avg_listen_pct,
      queue_add_count, queue_move_closer, skip_count, early_skip_count,
      queue_remove_count, playlist_add_count, affinity_score, last_interaction_ms)
     VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7, 0, 0, 0.0, ?8)
     ON CONFLICT(user_id, track_uri) DO UPDATE SET
       play_count = user_track_affinity.play_count + ?3,
       total_listen_ms = user_track_affinity.total_listen_ms + ?4,
       avg_listen_pct = CASE
         WHEN (user_track_affinity.play_count + user_track_affinity.skip_count + ?3 + ?6) > 0
         THEN ((user_track_affinity.avg_listen_pct * (user_track_affinity.play_count + user_track_affinity.skip_count)) + ?5)
              / (user_track_affinity.play_count + user_track_affinity.skip_count + ?3 + ?6)
         ELSE ?5
       END,
       skip_count = user_track_affinity.skip_count + ?6,
       early_skip_count = user_track_affinity.early_skip_count + ?7,
       last_interaction_ms = ?8,
       affinity_score =
         ((user_track_affinity.play_count + ?3) * 2.0)
         + (CASE
              WHEN (user_track_affinity.play_count + user_track_affinity.skip_count + ?3 + ?6) > 0
              THEN ((user_track_affinity.avg_listen_pct * (user_track_affinity.play_count + user_track_affinity.skip_count)) + ?5)
                   / (user_track_affinity.play_count + user_track_affinity.skip_count + ?3 + ?6)
              ELSE ?5
            END * 3.0)
         + (user_track_affinity.queue_add_count * 1.5)
         + (user_track_affinity.queue_move_closer * 2.0)
         + (user_track_affinity.playlist_add_count * 1.0)
         - ((user_track_affinity.skip_count + ?6) * 1.0)
         - ((user_track_affinity.early_skip_count + ?7) * 2.0)
         - (user_track_affinity.queue_remove_count * 1.5)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.text(track_uri),
      sqlight.int(play_inc),
      sqlight.int(listen_ms),
      sqlight.float(listen_pct),
      sqlight.int(skip_inc),
      sqlight.int(early_skip_inc),
      sqlight.int(now_ms),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Update track affinity from a queue action (add, remove, move_closer).
pub fn update_track_affinity_queue(
  db: sqlight.Connection,
  user_id: Int,
  track_uri: String,
  action: String,
  now_ms: Int,
) -> Result(Nil, sqlight.Error) {
  let #(add_inc, move_inc, remove_inc) = case action {
    "add" -> #(1, 0, 0)
    "move_closer" -> #(0, 1, 0)
    "remove" -> #(0, 0, 1)
    _ -> #(0, 0, 0)
  }

  let sql =
    "INSERT INTO user_track_affinity
     (user_id, track_uri, play_count, total_listen_ms, avg_listen_pct,
      queue_add_count, queue_move_closer, skip_count, early_skip_count,
      queue_remove_count, playlist_add_count, affinity_score, last_interaction_ms)
     VALUES (?1, ?2, 0, 0, 0.0, ?3, ?4, 0, 0, ?5, 0, 0.0, ?6)
     ON CONFLICT(user_id, track_uri) DO UPDATE SET
       queue_add_count = user_track_affinity.queue_add_count + ?3,
       queue_move_closer = user_track_affinity.queue_move_closer + ?4,
       queue_remove_count = user_track_affinity.queue_remove_count + ?5,
       last_interaction_ms = ?6,
       affinity_score =
         (user_track_affinity.play_count * 2.0)
         + (user_track_affinity.avg_listen_pct * 3.0)
         + ((user_track_affinity.queue_add_count + ?3) * 1.5)
         + ((user_track_affinity.queue_move_closer + ?4) * 2.0)
         + (user_track_affinity.playlist_add_count * 1.0)
         - (user_track_affinity.skip_count * 1.0)
         - (user_track_affinity.early_skip_count * 2.0)
         - ((user_track_affinity.queue_remove_count + ?5) * 1.5)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.text(track_uri),
      sqlight.int(add_inc),
      sqlight.int(move_inc),
      sqlight.int(remove_inc),
      sqlight.int(now_ms),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get top tracks by affinity score for a user
pub fn get_user_track_affinities(
  db: sqlight.Connection,
  user_id: Int,
  limit: Int,
) -> Result(List(UserTrackAffinity), sqlight.Error) {
  let sql =
    "SELECT user_id, track_uri, play_count, total_listen_ms, avg_listen_pct,
       queue_add_count, queue_move_closer, skip_count, early_skip_count,
       queue_remove_count, playlist_add_count, affinity_score, last_interaction_ms
     FROM user_track_affinity
     WHERE user_id = ?
     ORDER BY affinity_score DESC
     LIMIT ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(user_id), sqlight.int(limit)],
    user_track_affinity_decoder(),
  )
}

fn user_track_affinity_decoder() -> decode.Decoder(UserTrackAffinity) {
  use user_id <- decode.field(0, decode.int)
  use track_uri <- decode.field(1, decode.string)
  use play_count <- decode.field(2, decode.int)
  use total_listen_ms <- decode.field(3, decode.int)
  use avg_listen_pct <- decode.field(4, decode.float)
  use queue_add_count <- decode.field(5, decode.int)
  use queue_move_closer <- decode.field(6, decode.int)
  use skip_count <- decode.field(7, decode.int)
  use early_skip_count <- decode.field(8, decode.int)
  use queue_remove_count <- decode.field(9, decode.int)
  use playlist_add_count <- decode.field(10, decode.int)
  use affinity_score <- decode.field(11, decode.float)
  use last_interaction_ms <- decode.field(12, decode.int)
  decode.success(UserTrackAffinity(
    user_id:,
    track_uri:,
    play_count:,
    total_listen_ms:,
    avg_listen_pct:,
    queue_add_count:,
    queue_move_closer:,
    skip_count:,
    early_skip_count:,
    queue_remove_count:,
    playlist_add_count:,
    affinity_score:,
    last_interaction_ms:,
  ))
}

// ============================================================================
// USER-ARTIST AFFINITY
// ============================================================================

pub type UserArtistAffinity {
  UserArtistAffinity(
    user_id: Int,
    artist_name: String,
    play_count: Int,
    skip_count: Int,
    total_listen_ms: Int,
    affinity_score: Float,
  )
}

/// Update artist affinity from a listen event.
/// Score = play_count * 2.0 - skip_count * 1.0 + total_listen_ms / 60000.0
pub fn update_artist_affinity_listen(
  db: sqlight.Connection,
  user_id: Int,
  artist_name: String,
  listen_ms: Int,
  is_skip: Bool,
) -> Result(Nil, sqlight.Error) {
  let play_inc = case is_skip {
    True -> 0
    False -> 1
  }
  let skip_inc = case is_skip {
    True -> 1
    False -> 0
  }

  let sql =
    "INSERT INTO user_artist_affinity
     (user_id, artist_name, play_count, skip_count, total_listen_ms, affinity_score)
     VALUES (?1, ?2, ?3, ?4, ?5, 0.0)
     ON CONFLICT(user_id, artist_name) DO UPDATE SET
       play_count = user_artist_affinity.play_count + ?3,
       skip_count = user_artist_affinity.skip_count + ?4,
       total_listen_ms = user_artist_affinity.total_listen_ms + ?5,
       affinity_score =
         ((user_artist_affinity.play_count + ?3) * 2.0)
         - ((user_artist_affinity.skip_count + ?4) * 1.0)
         + ((user_artist_affinity.total_listen_ms + ?5) / 60000.0)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.text(artist_name),
      sqlight.int(play_inc),
      sqlight.int(skip_inc),
      sqlight.int(listen_ms),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get top artists by affinity score for a user
pub fn get_user_artist_affinities(
  db: sqlight.Connection,
  user_id: Int,
  limit: Int,
) -> Result(List(UserArtistAffinity), sqlight.Error) {
  let sql =
    "SELECT user_id, artist_name, play_count, skip_count,
       total_listen_ms, affinity_score
     FROM user_artist_affinity
     WHERE user_id = ?
     ORDER BY affinity_score DESC
     LIMIT ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(user_id), sqlight.int(limit)],
    user_artist_affinity_decoder(),
  )
}

fn user_artist_affinity_decoder() -> decode.Decoder(UserArtistAffinity) {
  use user_id <- decode.field(0, decode.int)
  use artist_name <- decode.field(1, decode.string)
  use play_count <- decode.field(2, decode.int)
  use skip_count <- decode.field(3, decode.int)
  use total_listen_ms <- decode.field(4, decode.int)
  use affinity_score <- decode.field(5, decode.float)
  decode.success(UserArtistAffinity(
    user_id:,
    artist_name:,
    play_count:,
    skip_count:,
    total_listen_ms:,
    affinity_score:,
  ))
}

// ============================================================================
// LISTENING SESSIONS
// ============================================================================

pub type ListeningSession {
  ListeningSession(
    id: Int,
    user_id: Int,
    started_ms: Int,
    ended_ms: Option(Int),
    track_count: Int,
    hour_of_day: Option(Int),
    day_of_week: Option(Int),
    dominant_genre: Option(String),
  )
}

/// Create a new listening session, returns the session id
pub fn create_session(
  db: sqlight.Connection,
  user_id: Int,
  started_ms: Int,
  hour_of_day: Int,
  day_of_week: Int,
) -> Result(Int, sqlight.Error) {
  let sql =
    "INSERT INTO listening_sessions
     (user_id, started_ms, track_count, hour_of_day, day_of_week)
     VALUES (?, ?, 0, ?, ?)
     RETURNING id"

  let id_decoder = {
    use id <- decode.field(0, decode.int)
    decode.success(id)
  }

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.int(started_ms),
      sqlight.int(hour_of_day),
      sqlight.int(day_of_week),
    ],
    id_decoder,
  )
  |> result.map(fn(rows) {
    case rows {
      [id, ..] -> id
      [] -> 0
    }
  })
}

/// Close a listening session by setting ended_ms and track_count
pub fn close_session(
  db: sqlight.Connection,
  session_id: Int,
  ended_ms: Int,
  track_count: Int,
  dominant_genre: Option(String),
) -> Result(Nil, sqlight.Error) {
  let sql =
    "UPDATE listening_sessions
     SET ended_ms = ?, track_count = ?, dominant_genre = ?
     WHERE id = ?"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(ended_ms),
      sqlight.int(track_count),
      sqlight.nullable(sqlight.text, dominant_genre),
      sqlight.int(session_id),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get listening sessions for a user (newest first)
pub fn get_user_sessions(
  db: sqlight.Connection,
  user_id: Int,
  limit: Int,
) -> Result(List(ListeningSession), sqlight.Error) {
  let sql =
    "SELECT id, user_id, started_ms, ended_ms, track_count,
       hour_of_day, day_of_week, dominant_genre
     FROM listening_sessions
     WHERE user_id = ?
     ORDER BY started_ms DESC
     LIMIT ?"

  sqlight.query(
    sql,
    db,
    [sqlight.int(user_id), sqlight.int(limit)],
    listening_session_decoder(),
  )
}

fn listening_session_decoder() -> decode.Decoder(ListeningSession) {
  use id <- decode.field(0, decode.int)
  use user_id <- decode.field(1, decode.int)
  use started_ms <- decode.field(2, decode.int)
  use ended_ms <- decode.field(3, decode.optional(decode.int))
  use track_count <- decode.field(4, decode.int)
  use hour_of_day <- decode.field(5, decode.optional(decode.int))
  use day_of_week <- decode.field(6, decode.optional(decode.int))
  use dominant_genre <- decode.field(7, decode.optional(decode.string))
  decode.success(ListeningSession(
    id:,
    user_id:,
    started_ms:,
    ended_ms:,
    track_count:,
    hour_of_day:,
    day_of_week:,
    dominant_genre:,
  ))
}

// ============================================================================
// SEARCH CONVERSIONS
// ============================================================================

pub type SearchConversion {
  SearchConversion(
    id: Int,
    user_id: Int,
    timestamp_ms: Int,
    query_text: Option(String),
    result_uri: Option(String),
    result_position: Option(Int),
    action: String,
    time_to_action_ms: Option(Int),
  )
}

/// Log a search conversion (search -> queue add within 30s)
pub fn log_search_conversion(
  db: sqlight.Connection,
  user_id: Int,
  timestamp_ms: Int,
  query_text: Option(String),
  result_uri: Option(String),
  action: String,
  time_to_action_ms: Option(Int),
) -> Result(Nil, sqlight.Error) {
  let sql =
    "INSERT INTO search_conversions
     (user_id, timestamp_ms, query_text, result_uri, action, time_to_action_ms)
     VALUES (?, ?, ?, ?, ?, ?)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.int(timestamp_ms),
      sqlight.nullable(sqlight.text, query_text),
      sqlight.nullable(sqlight.text, result_uri),
      sqlight.text(action),
      sqlight.nullable(sqlight.int, time_to_action_ms),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

// ============================================================================
// PLAYLISTS
// ============================================================================

pub type Playlist {
  Playlist(
    id: Int,
    user_id: Int,
    name: String,
    description: Option(String),
    created_at: Int,
    updated_at: Int,
    is_public: Bool,
  )
}

pub type PlaylistTrack {
  PlaylistTrack(playlist_id: Int, track_uri: String, position: Int)
}

fn playlist_decoder() -> decode.Decoder(Playlist) {
  use id <- decode.field(0, decode.int)
  use user_id <- decode.field(1, decode.int)
  use name <- decode.field(2, decode.string)
  use description <- decode.field(3, decode.optional(decode.string))
  use created_at <- decode.field(4, decode.int)
  use updated_at <- decode.field(5, decode.int)
  use is_public_int <- decode.field(6, decode.int)
  decode.success(Playlist(
    id:,
    user_id:,
    name:,
    description:,
    created_at:,
    updated_at:,
    is_public: is_public_int == 1,
  ))
}

fn playlist_track_decoder() -> decode.Decoder(PlaylistTrack) {
  use playlist_id <- decode.field(0, decode.int)
  use track_uri <- decode.field(1, decode.string)
  use position <- decode.field(2, decode.int)
  decode.success(PlaylistTrack(playlist_id:, track_uri:, position:))
}

/// Create a new playlist
pub fn create_playlist(
  db: sqlight.Connection,
  user_id: Int,
  name: String,
  description: Option(String),
  is_public: Bool,
  now_ms: Int,
) -> Result(List(Playlist), sqlight.Error) {
  let sql =
    "INSERT INTO playlists (user_id, name, description, is_public, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?)
     RETURNING id, user_id, name, description, created_at, updated_at, is_public"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.text(name),
      sqlight.nullable(sqlight.text, description),
      sqlight.int(case is_public {
        True -> 1
        False -> 0
      }),
      sqlight.int(now_ms),
      sqlight.int(now_ms),
    ],
    playlist_decoder(),
  )
}

/// Get all playlists for a user
pub fn get_user_playlists(
  db: sqlight.Connection,
  user_id: Int,
) -> Result(List(Playlist), sqlight.Error) {
  let sql =
    "SELECT id, user_id, name, description, created_at, updated_at, is_public
     FROM playlists
     WHERE user_id = ?
     ORDER BY updated_at DESC"

  sqlight.query(sql, db, [sqlight.int(user_id)], playlist_decoder())
}

/// Get all public playlists
pub fn get_public_playlists(
  db: sqlight.Connection,
) -> Result(List(Playlist), sqlight.Error) {
  let sql =
    "SELECT id, user_id, name, description, created_at, updated_at, is_public
     FROM playlists
     WHERE is_public = 1
     ORDER BY updated_at DESC"

  sqlight.query(sql, db, [], playlist_decoder())
}

/// Get a single playlist by ID
pub fn get_playlist_by_id(
  db: sqlight.Connection,
  playlist_id: Int,
) -> Result(List(Playlist), sqlight.Error) {
  let sql =
    "SELECT id, user_id, name, description, created_at, updated_at, is_public
     FROM playlists
     WHERE id = ?"

  sqlight.query(sql, db, [sqlight.int(playlist_id)], playlist_decoder())
}

/// Update playlist name, description, and is_public
pub fn update_playlist(
  db: sqlight.Connection,
  playlist_id: Int,
  name: String,
  description: Option(String),
  is_public: Bool,
  now_ms: Int,
) -> Result(List(Playlist), sqlight.Error) {
  let sql =
    "UPDATE playlists
     SET name = ?, description = ?, is_public = ?, updated_at = ?
     WHERE id = ?
     RETURNING id, user_id, name, description, created_at, updated_at, is_public"

  sqlight.query(
    sql,
    db,
    [
      sqlight.text(name),
      sqlight.nullable(sqlight.text, description),
      sqlight.int(case is_public {
        True -> 1
        False -> 0
      }),
      sqlight.int(now_ms),
      sqlight.int(playlist_id),
    ],
    playlist_decoder(),
  )
}

/// Delete a playlist (CASCADE removes tracks)
pub fn delete_playlist(
  db: sqlight.Connection,
  playlist_id: Int,
) -> Result(Nil, sqlight.Error) {
  sqlight.query(
    "DELETE FROM playlists WHERE id = ?",
    db,
    [sqlight.int(playlist_id)],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Get tracks for a playlist ordered by position
pub fn get_playlist_tracks(
  db: sqlight.Connection,
  playlist_id: Int,
) -> Result(List(PlaylistTrack), sqlight.Error) {
  let sql =
    "SELECT playlist_id, track_uri, position
     FROM playlist_tracks
     WHERE playlist_id = ?
     ORDER BY position ASC"

  sqlight.query(
    sql,
    db,
    [sqlight.int(playlist_id)],
    playlist_track_decoder(),
  )
}

/// Get IDs of user's playlists that contain a given track
pub fn get_playlists_containing_track(
  db: sqlight.Connection,
  user_id: Int,
  track_uri: String,
) -> Result(List(Int), sqlight.Error) {
  let sql =
    "SELECT p.id
     FROM playlists p
     JOIN playlist_tracks pt ON p.id = pt.playlist_id
     WHERE p.user_id = ? AND pt.track_uri = ?"

  let id_decoder = {
    use id <- decode.field(0, decode.int)
    decode.success(id)
  }

  sqlight.query(
    sql,
    db,
    [sqlight.int(user_id), sqlight.text(track_uri)],
    id_decoder,
  )
}

/// Add a track to a playlist at the end
pub fn add_track_to_playlist(
  db: sqlight.Connection,
  playlist_id: Int,
  track_uri: String,
) -> Result(Nil, sqlight.Error) {
  let sql =
    "INSERT INTO playlist_tracks (playlist_id, track_uri, position)
     VALUES (?1, ?2, COALESCE((SELECT MAX(position) + 1 FROM playlist_tracks WHERE playlist_id = ?1), 0))"

  sqlight.query(
    sql,
    db,
    [sqlight.int(playlist_id), sqlight.text(track_uri)],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Remove a track from a playlist
pub fn remove_track_from_playlist(
  db: sqlight.Connection,
  playlist_id: Int,
  track_uri: String,
) -> Result(Nil, sqlight.Error) {
  sqlight.query(
    "DELETE FROM playlist_tracks WHERE playlist_id = ? AND track_uri = ?",
    db,
    [sqlight.int(playlist_id), sqlight.text(track_uri)],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Reorder a track within a playlist to a new position
pub fn reorder_playlist_track(
  db: sqlight.Connection,
  playlist_id: Int,
  track_uri: String,
  new_position: Int,
) -> Result(Nil, sqlight.Error) {
  // Get current position
  let current_pos_result =
    sqlight.query(
      "SELECT position FROM playlist_tracks WHERE playlist_id = ? AND track_uri = ?",
      db,
      [sqlight.int(playlist_id), sqlight.text(track_uri)],
      {
        use pos <- decode.field(0, decode.int)
        decode.success(pos)
      },
    )

  case current_pos_result {
    Ok([old_position]) -> {
      // Shift other tracks
      case old_position < new_position {
        True -> {
          // Moving down: shift tracks between old+1..new up by -1
          let _ =
            sqlight.query(
              "UPDATE playlist_tracks SET position = position - 1
               WHERE playlist_id = ? AND position > ? AND position <= ?",
              db,
              [
                sqlight.int(playlist_id),
                sqlight.int(old_position),
                sqlight.int(new_position),
              ],
              decode.dynamic,
            )
          Nil
        }
        False -> {
          // Moving up: shift tracks between new..old-1 down by +1
          let _ =
            sqlight.query(
              "UPDATE playlist_tracks SET position = position + 1
               WHERE playlist_id = ? AND position >= ? AND position < ?",
              db,
              [
                sqlight.int(playlist_id),
                sqlight.int(new_position),
                sqlight.int(old_position),
              ],
              decode.dynamic,
            )
          Nil
        }
      }
      // Set the track's new position
      sqlight.query(
        "UPDATE playlist_tracks SET position = ? WHERE playlist_id = ? AND track_uri = ?",
        db,
        [
          sqlight.int(new_position),
          sqlight.int(playlist_id),
          sqlight.text(track_uri),
        ],
        decode.dynamic,
      )
      |> result.map(fn(_) { Nil })
    }
    _ -> Ok(Nil)
  }
}

/// Update track affinity from a playlist action (add or remove).
pub fn update_track_affinity_playlist(
  db: sqlight.Connection,
  user_id: Int,
  track_uri: String,
  action: String,
  now_ms: Int,
) -> Result(Nil, sqlight.Error) {
  let add_inc = case action {
    "add" -> 1
    _ -> 0
  }

  let sql =
    "INSERT INTO user_track_affinity
     (user_id, track_uri, play_count, total_listen_ms, avg_listen_pct,
      queue_add_count, queue_move_closer, skip_count, early_skip_count,
      queue_remove_count, playlist_add_count, affinity_score, last_interaction_ms)
     VALUES (?1, ?2, 0, 0, 0.0, 0, 0, 0, 0, 0, ?3, 0.0, ?4)
     ON CONFLICT(user_id, track_uri) DO UPDATE SET
       playlist_add_count = user_track_affinity.playlist_add_count + ?3,
       last_interaction_ms = ?4,
       affinity_score =
         (user_track_affinity.play_count * 2.0)
         + (user_track_affinity.avg_listen_pct * 3.0)
         + (user_track_affinity.queue_add_count * 1.5)
         + (user_track_affinity.queue_move_closer * 2.0)
         + ((user_track_affinity.playlist_add_count + ?3) * 1.0)
         - (user_track_affinity.skip_count * 1.0)
         - (user_track_affinity.early_skip_count * 2.0)
         - (user_track_affinity.queue_remove_count * 1.5)"

  sqlight.query(
    sql,
    db,
    [
      sqlight.int(user_id),
      sqlight.text(track_uri),
      sqlight.int(add_inc),
      sqlight.int(now_ms),
    ],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

// ============================================================================
// SETTINGS / ARTIST INDEX / RPC CACHE (V8)
// ============================================================================

pub fn get_settings(
  db: sqlight.Connection,
) -> Result(List(#(String, String)), sqlight.Error) {
  let decoder = {
    use key <- decode.field(0, decode.string)
    use value <- decode.field(1, decode.string)
    decode.success(#(key, value))
  }
  sqlight.query("SELECT key, value FROM settings", db, [], decoder)
}

pub fn set_setting(
  db: sqlight.Connection,
  key: String,
  value: String,
  now_ms: Int,
) -> Result(Nil, sqlight.Error) {
  sqlight.query(
    "INSERT INTO settings (key, value, updated_ms) VALUES (?, ?, ?)
     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_ms = excluded.updated_ms",
    db,
    [sqlight.text(key), sqlight.text(value), sqlight.int(now_ms)],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

pub fn upsert_artist_index(
  db: sqlight.Connection,
  artist_name: String,
  artist_uri: String,
  now_ms: Int,
) -> Result(Nil, sqlight.Error) {
  sqlight.query(
    "INSERT INTO artist_index (artist_name, artist_uri, updated_ms) VALUES (?, ?, ?)
     ON CONFLICT(artist_name) DO UPDATE SET artist_uri = excluded.artist_uri, updated_ms = excluded.updated_ms",
    db,
    [sqlight.text(artist_name), sqlight.text(artist_uri), sqlight.int(now_ms)],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

/// Whole artist name -> URI index (small: one row per artist ever played)
pub fn get_artist_index(
  db: sqlight.Connection,
) -> Result(List(#(String, String)), sqlight.Error) {
  let decoder = {
    use name <- decode.field(0, decode.string)
    use uri <- decode.field(1, decode.string)
    decode.success(#(name, uri))
  }
  sqlight.query("SELECT artist_name, artist_uri FROM artist_index", db, [], decoder)
}

/// Any track URI we have seen for an artist (used to resolve the artist URI via lookup)
pub fn get_track_uri_for_artist(
  db: sqlight.Connection,
  artist_name: String,
) -> Result(Option(String), sqlight.Error) {
  sqlight.query(
    "SELECT uri FROM track_features WHERE artist_name = ? ORDER BY play_count DESC LIMIT 1",
    db,
    [sqlight.text(artist_name)],
    decode.at([0], decode.string),
  )
  |> result.map(fn(rows) { option.from_result(list.first(rows)) })
}

pub fn cache_get(
  db: sqlight.Connection,
  key: String,
  max_age_ms: Int,
  now_ms: Int,
) -> Result(Option(String), sqlight.Error) {
  sqlight.query(
    "SELECT value FROM rpc_cache WHERE key = ? AND updated_ms > ?",
    db,
    [sqlight.text(key), sqlight.int(now_ms - max_age_ms)],
    decode.at([0], decode.string),
  )
  |> result.map(fn(rows) { option.from_result(list.first(rows)) })
}

pub fn cache_put(
  db: sqlight.Connection,
  key: String,
  value: String,
  now_ms: Int,
) -> Result(Nil, sqlight.Error) {
  sqlight.query(
    "INSERT INTO rpc_cache (key, value, updated_ms) VALUES (?, ?, ?)
     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_ms = excluded.updated_ms",
    db,
    [sqlight.text(key), sqlight.text(value), sqlight.int(now_ms)],
    decode.dynamic,
  )
  |> result.map(fn(_) { Nil })
}

// ============================================================================
// ATTRIBUTION FALLBACK
// ============================================================================

/// The (human) user who most recently added this track to the queue, if any.
/// Used to attribute natural track completions that no explicit command caused.
pub fn find_queued_by(
  db: sqlight.Connection,
  track_uri: String,
) -> Result(Option(Int), sqlight.Error) {
  sqlight.query(
    "SELECT qe.user_id FROM queue_events qe
     JOIN users u ON u.id = qe.user_id
     WHERE qe.event_type IN ('add', 'add_at_position')
       AND u.username != 'autoplay'
       AND qe.track_uris LIKE ?
     ORDER BY qe.timestamp_ms DESC LIMIT 1",
    db,
    [sqlight.text("%\"" <> track_uri <> "\"%")],
    decode.at([0], decode.int),
  )
  |> result.map(fn(rows) { option.from_result(list.first(rows)) })
}

// ============================================================================
// RECOMMENDATION INPUTS
// ============================================================================

/// Users with any activity since `since_ms`: (id, username, last_activity_ms), most recent first
pub fn get_active_users(
  db: sqlight.Connection,
  since_ms: Int,
) -> Result(List(#(Int, String, Int)), sqlight.Error) {
  let decoder = {
    use id <- decode.field(0, decode.int)
    use username <- decode.field(1, decode.string)
    use last <- decode.field(2, decode.int)
    decode.success(#(id, username, last))
  }
  sqlight.query(
    "SELECT u.id, u.username, MAX(e.ts) as last_activity
     FROM users u
     JOIN (
       SELECT user_id, timestamp_ms as ts FROM playback_events
       UNION ALL
       SELECT user_id, timestamp_ms FROM queue_events
       UNION ALL
       SELECT user_id, timestamp_ms FROM search_events
     ) e ON e.user_id = u.id
     WHERE e.ts > ? AND u.username != 'autoplay'
     GROUP BY u.id, u.username
     ORDER BY last_activity DESC",
    db,
    [sqlight.int(since_ms)],
    decoder,
  )
}

/// Users that have any positive affinity at all (fallback seed set when nobody is active)
pub fn get_users_with_affinity(
  db: sqlight.Connection,
) -> Result(List(Int), sqlight.Error) {
  sqlight.query(
    "SELECT DISTINCT user_id FROM user_artist_affinity WHERE affinity_score > 0",
    db,
    [],
    decode.at([0], decode.int),
  )
}

pub fn get_recently_played_uris(
  db: sqlight.Connection,
  since_ms: Int,
) -> Result(List(String), sqlight.Error) {
  sqlight.query(
    "SELECT DISTINCT track_uri FROM playback_state_log
     WHERE event_type = 'track_started' AND timestamp_ms > ? AND track_uri IS NOT NULL",
    db,
    [sqlight.int(since_ms)],
    decode.at([0], decode.string),
  )
}

/// Tracks the user has skipped early or thrown out of the queue and never grew to like
pub fn get_user_disliked_uris(
  db: sqlight.Connection,
  user_id: Int,
) -> Result(List(String), sqlight.Error) {
  sqlight.query(
    "SELECT track_uri FROM user_track_affinity
     WHERE user_id = ? AND (early_skip_count > 0 OR queue_remove_count > 0) AND affinity_score <= 0",
    db,
    [sqlight.int(user_id)],
    decode.at([0], decode.string),
  )
}

/// (uri, summed affinity, distinct users) across everyone
pub fn get_room_top_tracks(
  db: sqlight.Connection,
  limit: Int,
) -> Result(List(#(String, Float, Int)), sqlight.Error) {
  let decoder = {
    use uri <- decode.field(0, decode.string)
    use score <- decode.field(1, decode.float)
    use users <- decode.field(2, decode.int)
    decode.success(#(uri, score, users))
  }
  sqlight.query(
    "SELECT track_uri, SUM(affinity_score) * 1.0 as score, COUNT(DISTINCT user_id)
     FROM user_track_affinity
     GROUP BY track_uri
     HAVING SUM(affinity_score) > 0
     ORDER BY score DESC
     LIMIT ?",
    db,
    [sqlight.int(limit)],
    decoder,
  )
}

/// (artist, summed affinity, distinct users, plays) across everyone
pub fn get_room_top_artists(
  db: sqlight.Connection,
  limit: Int,
) -> Result(List(#(String, Float, Int, Int)), sqlight.Error) {
  let decoder = {
    use name <- decode.field(0, decode.string)
    use score <- decode.field(1, decode.float)
    use users <- decode.field(2, decode.int)
    use plays <- decode.field(3, decode.int)
    decode.success(#(name, score, users, plays))
  }
  sqlight.query(
    "SELECT artist_name, SUM(affinity_score) * 1.0 as score, COUNT(DISTINCT user_id), SUM(play_count)
     FROM user_artist_affinity
     GROUP BY artist_name
     HAVING SUM(affinity_score) > 0
     ORDER BY score DESC
     LIMIT ?",
    db,
    [sqlight.int(limit)],
    decoder,
  )
}

/// Each user's top artists (for the room taste map): (user_id, artist, score)
pub fn get_users_top_artists(
  db: sqlight.Connection,
  per_user: Int,
) -> Result(List(#(Int, String, Float)), sqlight.Error) {
  let decoder = {
    use user_id <- decode.field(0, decode.int)
    use name <- decode.field(1, decode.string)
    use score <- decode.field(2, decode.float)
    decode.success(#(user_id, name, score))
  }
  sqlight.query(
    "SELECT user_id, artist_name, affinity_score * 1.0 FROM (
       SELECT user_id, artist_name, affinity_score,
              ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY affinity_score DESC) as rn
       FROM user_artist_affinity WHERE affinity_score > 0
     ) WHERE rn <= ?
     ORDER BY user_id, affinity_score DESC",
    db,
    [sqlight.int(per_user)],
    decoder,
  )
}

pub type TrackMeta {
  TrackMeta(
    uri: String,
    name: Option(String),
    artist_name: Option(String),
    album_name: Option(String),
    duration_ms: Option(Int),
    play_count: Int,
    skip_count: Int,
  )
}

/// Known metadata for a batch of track URIs
pub fn get_track_meta_many(
  db: sqlight.Connection,
  uris: List(String),
) -> Result(List(TrackMeta), sqlight.Error) {
  case uris {
    [] -> Ok([])
    _ -> {
      let placeholders =
        list.map(uris, fn(_) { "?" }) |> string.join(", ")
      let decoder = {
        use uri <- decode.field(0, decode.string)
        use name <- decode.field(1, decode.optional(decode.string))
        use artist <- decode.field(2, decode.optional(decode.string))
        use album <- decode.field(3, decode.optional(decode.string))
        use duration <- decode.field(4, decode.optional(decode.int))
        use plays <- decode.field(5, decode.int)
        use skips <- decode.field(6, decode.int)
        decode.success(TrackMeta(
          uri:,
          name:,
          artist_name: artist,
          album_name: album,
          duration_ms: duration,
          play_count: plays,
          skip_count: skips,
        ))
      }
      sqlight.query(
        "SELECT uri, name, artist_name, album_name, duration_ms, play_count, skip_count
         FROM track_features WHERE uri IN ("
          <> placeholders
          <> ")",
        db,
        list.map(uris, sqlight.text),
        decoder,
      )
    }
  }
}

// ============================================================================
// INSIGHTS
// ============================================================================

/// Single integer result of an aggregate query
pub fn scalar_int(
  db: sqlight.Connection,
  sql: String,
  args: List(sqlight.Value),
) -> Result(Int, sqlight.Error) {
  sqlight.query(sql, db, args, decode.at([0], decode.int))
  |> result.map(fn(rows) {
    case rows {
      [n, ..] -> n
      [] -> 0
    }
  })
}

/// Plays per UTC hour-of-day since `since_ms` (from actual track starts, not clicks)
pub fn get_hourly_plays(
  db: sqlight.Connection,
  since_ms: Int,
) -> Result(List(#(Int, Int)), sqlight.Error) {
  let decoder = {
    use hour <- decode.field(0, decode.int)
    use plays <- decode.field(1, decode.int)
    decode.success(#(hour, plays))
  }
  sqlight.query(
    "SELECT CAST(strftime('%H', datetime(timestamp_ms / 1000, 'unixepoch')) AS INTEGER) as hour,
            COUNT(*)
     FROM playback_state_log
     WHERE event_type = 'track_started' AND timestamp_ms > ?
     GROUP BY hour ORDER BY hour",
    db,
    [sqlight.int(since_ms)],
    decoder,
  )
}

/// Plays per UTC day since `since_ms`: (YYYY-MM-DD, plays)
pub fn get_daily_plays(
  db: sqlight.Connection,
  since_ms: Int,
) -> Result(List(#(String, Int)), sqlight.Error) {
  let decoder = {
    use day <- decode.field(0, decode.string)
    use plays <- decode.field(1, decode.int)
    decode.success(#(day, plays))
  }
  sqlight.query(
    "SELECT strftime('%Y-%m-%d', datetime(timestamp_ms / 1000, 'unixepoch')) as day, COUNT(*)
     FROM playback_state_log
     WHERE event_type = 'track_started' AND timestamp_ms > ?
     GROUP BY day ORDER BY day",
    db,
    [sqlight.int(since_ms)],
    decoder,
  )
}

/// Latest queue additions with who did them: (timestamp, username, names json, uris json, event_type)
pub fn get_recent_queue_adds(
  db: sqlight.Connection,
  limit: Int,
) -> Result(
  List(#(Int, String, Option(String), Option(String), String)),
  sqlight.Error,
) {
  let decoder = {
    use ts <- decode.field(0, decode.int)
    use username <- decode.field(1, decode.string)
    use names <- decode.field(2, decode.optional(decode.string))
    use uris <- decode.field(3, decode.optional(decode.string))
    use event_type <- decode.field(4, decode.string)
    decode.success(#(ts, username, names, uris, event_type))
  }
  sqlight.query(
    "SELECT qe.timestamp_ms, u.username, qe.track_names, qe.track_uris, qe.event_type
     FROM queue_events qe JOIN users u ON u.id = qe.user_id
     WHERE qe.event_type IN ('add', 'add_at_position', 'autoplay')
     ORDER BY qe.timestamp_ms DESC LIMIT ?",
    db,
    [sqlight.int(limit)],
    decoder,
  )
}

/// Most recent track starts: (timestamp, uri, name, artist, user_id)
pub fn get_recent_plays(
  db: sqlight.Connection,
  limit: Int,
) -> Result(
  List(#(Int, Option(String), Option(String), Option(String), Option(Int))),
  sqlight.Error,
) {
  let decoder = {
    use ts <- decode.field(0, decode.int)
    use uri <- decode.field(1, decode.optional(decode.string))
    use name <- decode.field(2, decode.optional(decode.string))
    use artist <- decode.field(3, decode.optional(decode.string))
    use user_id <- decode.field(4, decode.optional(decode.int))
    decode.success(#(ts, uri, name, artist, user_id))
  }
  sqlight.query(
    "SELECT timestamp_ms, track_uri, track_name, artist_name, user_id
     FROM playback_state_log WHERE event_type = 'track_started'
     ORDER BY timestamp_ms DESC LIMIT ?",
    db,
    [sqlight.int(limit)],
    decoder,
  )
}
