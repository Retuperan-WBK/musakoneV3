//// Room and per-user listening statistics, computed on demand from
//// playback_state_log (what the speakers actually did) and queue_events
//// (what people asked for). "Heard" means wall-clock: a play lasts until the
//// next track starts, never longer than the track; position_ms from Mopidy
//// is not trusted (it reports full length on stream failures).

import gleam/dynamic/decode
import gleam/int
import gleam/result
import sqlight

/// Window for a query: events at or after `since_ms`, local dates shifted by `tz_s` seconds.
pub type Window {
  Window(since_ms: Int, tz_s: Int, now_s: Int)
}

/// id of the system user whose queue adds are not human
const autoplay_user = 23

fn n(i: Int) -> String {
  int.to_string(i)
}

/// SQL fragment: local calendar date of a ms timestamp column
fn day(w: Window, col: String) -> String {
  "date((" <> col <> "/1000)+" <> n(w.tz_s) <> ",'unixepoch')"
}

/// SQL fragment: track starts in the window with their wall-clock heard time
fn starts(w: Window) -> String {
  "SELECT id, timestamp_ms, track_uri, track_name, artist_name, track_duration_ms, user_id,
          min(coalesce(LEAD(timestamp_ms) OVER (ORDER BY timestamp_ms, id) - timestamp_ms, track_duration_ms, 0),
              coalesce(track_duration_ms, LEAD(timestamp_ms) OVER (ORDER BY timestamp_ms, id) - timestamp_ms, 0)) heard_ms
   FROM playback_state_log
   WHERE event_type = 'track_started' AND timestamp_ms >= "
  <> n(w.since_ms)
}

fn human_adds(w: Window) -> String {
  "SELECT q.id, q.user_id, q.timestamp_ms ts, q.queue_length ql, json_array_length(q.track_uris) n, j.value uri
   FROM queue_events q, json_each(q.track_uris) j
   WHERE q.event_type IN ('add','add_at_position') AND q.user_id != "
  <> n(autoplay_user)
  <> " AND q.timestamp_ms >= "
  <> n(w.since_ms)
}

/// first play of the uri at or after the add, minus the add time (ms)
const wait_expr =
  "((SELECT min(p.timestamp_ms) FROM playback_state_log p
                    WHERE p.track_uri = a.uri AND p.event_type = 'track_started' AND p.timestamp_ms >= a.ts) - a.ts)"

pub type Totals {
  Totals(
    plays: Int,
    tracks: Int,
    artists: Int,
    play_days: Int,
    hours: Float,
    sessions: Int,
    djs: Int,
    adds: Int,
    searches: Int,
  )
}

pub fn totals(
  db: sqlight.Connection,
  w: Window,
) -> Result(Totals, sqlight.Error) {
  let sql = "WITH s AS (" <> starts(w) <> "),
     g AS (SELECT CASE WHEN timestamp_ms - LAG(timestamp_ms) OVER (ORDER BY timestamp_ms) > 1800000 THEN 1 ELSE 0 END brk FROM s)
     SELECT (SELECT count(*) FROM s),
            (SELECT count(DISTINCT track_uri) FROM s),
            (SELECT count(DISTINCT artist_name) FROM s WHERE artist_name IS NOT NULL AND artist_name != ''),
            (SELECT count(DISTINCT " <> day(w, "timestamp_ms") <> ") FROM s),
            (SELECT CAST(coalesce(sum(heard_ms), 0) / 3600000.0 AS REAL) FROM s),
            (SELECT coalesce(sum(brk), 0) + min(count(*), 1) FROM g),
            (SELECT count(DISTINCT user_id) FROM queue_events WHERE event_type IN ('add','add_at_position') AND user_id != " <> n(
      autoplay_user,
    ) <> " AND timestamp_ms >= " <> n(w.since_ms) <> "),
            (SELECT count(*) FROM queue_events WHERE event_type IN ('add','add_at_position') AND user_id != " <> n(
      autoplay_user,
    ) <> " AND timestamp_ms >= " <> n(w.since_ms) <> "),
            (SELECT count(*) FROM search_events WHERE event_type = 'search' AND timestamp_ms >= " <> n(
      w.since_ms,
    ) <> ")"
  let decoder = {
    use plays <- decode.field(0, decode.int)
    use tracks <- decode.field(1, decode.int)
    use artists <- decode.field(2, decode.int)
    use play_days <- decode.field(3, decode.int)
    use hours <- decode.field(4, decode.float)
    use sessions <- decode.field(5, decode.int)
    use djs <- decode.field(6, decode.int)
    use adds <- decode.field(7, decode.int)
    use searches <- decode.field(8, decode.int)
    decode.success(Totals(
      plays:,
      tracks:,
      artists:,
      play_days:,
      hours:,
      sessions:,
      djs:,
      adds:,
      searches:,
    ))
  }
  one(db, sql, decoder, Totals(0, 0, 0, 0, 0.0, 0, 0, 0, 0))
}

/// (date, plays, hours, distinct tracks, human adds)
pub fn per_day(
  db: sqlight.Connection,
  w: Window,
) -> Result(List(#(String, Int, Float, Int, Int)), sqlight.Error) {
  let sql = "WITH s AS (" <> starts(w) <> "),
     d AS (SELECT " <> day(w, "timestamp_ms") <> " d, count(*) plays, CAST(sum(heard_ms) / 3600000.0 AS REAL) hours, count(DISTINCT track_uri) tracks FROM s GROUP BY 1),
     a AS (SELECT " <> day(w, "timestamp_ms") <> " d, count(*) adds FROM queue_events WHERE event_type IN ('add','add_at_position') AND user_id != " <> n(
      autoplay_user,
    ) <> " AND timestamp_ms >= " <> n(w.since_ms) <> " GROUP BY 1)
     SELECT d.d, d.plays, d.hours, d.tracks, coalesce(a.adds, 0) FROM d LEFT JOIN a ON a.d = d.d ORDER BY d.d"
  let decoder = {
    use d <- decode.field(0, decode.string)
    use plays <- decode.field(1, decode.int)
    use hours <- decode.field(2, decode.float)
    use tracks <- decode.field(3, decode.int)
    use adds <- decode.field(4, decode.int)
    decode.success(#(d, plays, hours, tracks, adds))
  }
  sqlight.query(sql, db, [], decoder)
}

/// (hour 0-23, plays) and (weekday 0=Sunday, plays)
pub fn by_hour(
  db: sqlight.Connection,
  w: Window,
) -> Result(List(#(Int, Int)), sqlight.Error) {
  bucket(db, w, "%H")
}

pub fn by_weekday(
  db: sqlight.Connection,
  w: Window,
) -> Result(List(#(Int, Int)), sqlight.Error) {
  bucket(db, w, "%w")
}

fn bucket(
  db: sqlight.Connection,
  w: Window,
  fmt: String,
) -> Result(List(#(Int, Int)), sqlight.Error) {
  let sql =
    "SELECT CAST(strftime('"
    <> fmt
    <> "', (timestamp_ms/1000)+"
    <> n(w.tz_s)
    <> ", 'unixepoch') AS INTEGER) b, count(*)
     FROM playback_state_log WHERE event_type = 'track_started' AND timestamp_ms >= "
    <> n(w.since_ms)
    <> " GROUP BY 1 ORDER BY 1"
  sqlight.query(sql, db, [], pair_decoder())
}

fn pair_decoder() -> decode.Decoder(#(Int, Int)) {
  use a <- decode.field(0, decode.int)
  use b <- decode.field(1, decode.int)
  decode.success(#(a, b))
}

pub type TopTrack {
  TopTrack(
    uri: String,
    name: String,
    artist: String,
    plays: Int,
    days: Int,
    last_ms: Int,
    heard_pct: Int,
  )
}

pub fn top_tracks(
  db: sqlight.Connection,
  w: Window,
  user: Int,
  limit: Int,
) -> Result(List(TopTrack), sqlight.Error) {
  let who = case user {
    0 -> ""
    u -> " AND user_id = " <> n(u)
  }
  let sql = "WITH s AS (" <> starts(w) <> ")
     SELECT track_uri, coalesce(track_name, ''), coalesce(artist_name, ''), count(*) plays,
            count(DISTINCT " <> day(w, "timestamp_ms") <> "), max(timestamp_ms),
            CAST(coalesce(100 * sum(heard_ms) / max(sum(track_duration_ms), 1), 0) AS INTEGER)
     FROM s WHERE track_uri IS NOT NULL" <> who <> " GROUP BY track_uri ORDER BY plays DESC, max(timestamp_ms) DESC LIMIT " <> n(
      limit,
    )
  let decoder = {
    use uri <- decode.field(0, decode.string)
    use name <- decode.field(1, decode.string)
    use artist <- decode.field(2, decode.string)
    use plays <- decode.field(3, decode.int)
    use days <- decode.field(4, decode.int)
    use last_ms <- decode.field(5, decode.int)
    use heard_pct <- decode.field(6, decode.int)
    decode.success(TopTrack(
      uri:,
      name:,
      artist:,
      plays:,
      days:,
      last_ms:,
      heard_pct:,
    ))
  }
  sqlight.query(sql, db, [], decoder)
}

/// (artist, plays, distinct tracks)
pub fn top_artists(
  db: sqlight.Connection,
  w: Window,
  user: Int,
  limit: Int,
) -> Result(List(#(String, Int, Int)), sqlight.Error) {
  let who = case user {
    0 -> ""
    u -> " AND user_id = " <> n(u)
  }
  let sql = "SELECT artist_name, count(*) plays, count(DISTINCT track_uri)
     FROM playback_state_log WHERE event_type = 'track_started' AND timestamp_ms >= " <> n(
      w.since_ms,
    ) <> "
       AND artist_name IS NOT NULL AND artist_name != ''" <> who <> "
     GROUP BY 1 ORDER BY 2 DESC LIMIT " <> n(limit)
  let decoder = {
    use artist <- decode.field(0, decode.string)
    use plays <- decode.field(1, decode.int)
    use tracks <- decode.field(2, decode.int)
    decode.success(#(artist, plays, tracks))
  }
  sqlight.query(sql, db, [], decoder)
}

/// (plays, same-day repeat plays, loop days (>3 plays per distinct track), play days)
pub fn repeats(
  db: sqlight.Connection,
  w: Window,
) -> Result(#(Int, Int, Int, Int), sqlight.Error) {
  let sql =
    "WITH d AS (SELECT track_uri, "
    <> day(w, "timestamp_ms")
    <> " d, count(*) c FROM playback_state_log
                WHERE event_type = 'track_started' AND timestamp_ms >= "
    <> n(w.since_ms)
    <> " GROUP BY 1, 2),
     dd AS (SELECT d, sum(c) plays, count(*) dt FROM d GROUP BY d)
     SELECT coalesce(sum(c), 0), coalesce(sum(CASE WHEN c > 1 THEN c - 1 ELSE 0 END), 0),
            (SELECT count(*) FROM dd WHERE plays * 1.0 / dt > 3), (SELECT count(*) FROM dd) FROM d"
  let decoder = {
    use a <- decode.field(0, decode.int)
    use b <- decode.field(1, decode.int)
    use c <- decode.field(2, decode.int)
    use d <- decode.field(3, decode.int)
    decode.success(#(a, b, c, d))
  }
  one(db, sql, decoder, #(0, 0, 0, 0))
}

/// (decade label, plays) from track_features.release_date
pub fn decades(
  db: sqlight.Connection,
  w: Window,
) -> Result(List(#(String, Int)), sqlight.Error) {
  let sql = "SELECT CASE WHEN substr(t.release_date,1,4) < '1970' THEN '<1970'
                 WHEN substr(t.release_date,1,4) < '1980' THEN '70s'
                 WHEN substr(t.release_date,1,4) < '1990' THEN '80s'
                 WHEN substr(t.release_date,1,4) < '2000' THEN '90s'
                 WHEN substr(t.release_date,1,4) < '2010' THEN '00s'
                 WHEN substr(t.release_date,1,4) < '2020' THEN '10s'
                 ELSE '20s' END decade, count(*)
     FROM playback_state_log p JOIN track_features t ON t.uri = p.track_uri
     WHERE p.event_type = 'track_started' AND p.timestamp_ms >= " <> n(
      w.since_ms,
    ) <> "
       AND t.release_date IS NOT NULL AND length(t.release_date) >= 4
     GROUP BY 1 ORDER BY min(substr(t.release_date,1,4))"
  let decoder = {
    use label <- decode.field(0, decode.string)
    use plays <- decode.field(1, decode.int)
    decode.success(#(label, plays))
  }
  sqlight.query(sql, db, [], decoder)
}

/// (next commands, within 10 s of track start) and the artists skipped most
pub fn skips(
  db: sqlight.Connection,
  w: Window,
) -> Result(#(Int, Int), sqlight.Error) {
  let sql =
    "SELECT count(*), coalesce(sum(position_ms < 10000), 0) FROM playback_events
     WHERE event_type = 'next' AND timestamp_ms >= " <> n(w.since_ms)
  one(db, sql, pair_decoder(), #(0, 0))
}

pub fn skip_magnets(
  db: sqlight.Connection,
  w: Window,
  limit: Int,
) -> Result(List(#(String, Int, Int)), sqlight.Error) {
  let sql = "SELECT e.artist_name, count(*) early,
            (SELECT count(*) FROM playback_state_log s WHERE s.event_type = 'track_started' AND s.artist_name = e.artist_name AND s.timestamp_ms >= " <> n(
      w.since_ms,
    ) <> ")
     FROM playback_events e
     WHERE e.event_type = 'next' AND e.position_ms < 10000 AND e.artist_name IS NOT NULL AND e.artist_name != '' AND e.timestamp_ms >= " <> n(
      w.since_ms,
    ) <> "
     GROUP BY 1 ORDER BY 2 DESC LIMIT " <> n(limit)
  let decoder = {
    use artist <- decode.field(0, decode.string)
    use early <- decode.field(1, decode.int)
    use plays <- decode.field(2, decode.int)
    decode.success(#(artist, early, plays))
  }
  sqlight.query(sql, db, [], decoder)
}

pub type Dj {
  Dj(
    user_id: Int,
    username: String,
    adds: Int,
    uris: Int,
    heard_24h: Int,
    plays_attributed: Int,
  )
}

pub fn djs(
  db: sqlight.Connection,
  w: Window,
  limit: Int,
) -> Result(List(Dj), sqlight.Error) {
  let sql = "WITH a AS (" <> human_adds(w) <> "),
     x AS (SELECT a.user_id, a.id, " <> wait_expr <> " w FROM a)
     SELECT u.id, u.username, count(DISTINCT x.id), count(*), coalesce(sum(x.w < 86400000), 0),
            (SELECT count(*) FROM playback_state_log p WHERE p.event_type = 'track_started' AND p.user_id = u.id AND p.timestamp_ms >= " <> n(
      w.since_ms,
    ) <> ")
     FROM x JOIN users u ON u.id = x.user_id GROUP BY u.id ORDER BY 4 DESC LIMIT " <> n(
      limit,
    )
  let decoder = {
    use user_id <- decode.field(0, decode.int)
    use username <- decode.field(1, decode.string)
    use adds <- decode.field(2, decode.int)
    use uris <- decode.field(3, decode.int)
    use heard_24h <- decode.field(4, decode.int)
    use plays_attributed <- decode.field(5, decode.int)
    decode.success(Dj(
      user_id:,
      username:,
      adds:,
      uris:,
      heard_24h:,
      plays_attributed:,
    ))
  }
  sqlight.query(sql, db, [], decoder)
}

pub type QueueHealth {
  QueueHealth(
    uris: Int,
    behind_100: Int,
    bulk_uris: Int,
    bulk_never_heard: Int,
    median_wait_min: Float,
    heard_24h: Int,
  )
}

/// `user` = 0 for the room, otherwise only that user's adds
pub fn queue_health(
  db: sqlight.Connection,
  w: Window,
  user: Int,
) -> Result(QueueHealth, sqlight.Error) {
  let who = case user {
    0 -> ""
    u -> " AND q.user_id = " <> n(u)
  }
  let sql = "WITH a AS (" <> human_adds(w) <> who <> "),
     x AS (SELECT a.n, a.ql, " <> wait_expr <> " / 60000.0 wait FROM a),
     single AS (SELECT wait FROM x WHERE n = 1 AND wait IS NOT NULL AND wait < 1440 ORDER BY wait)
     SELECT count(*), coalesce(sum(ql >= 100), 0), coalesce(sum(n > 20), 0), coalesce(sum(n > 20 AND wait IS NULL), 0),
            CAST(coalesce((SELECT wait FROM single LIMIT 1 OFFSET (SELECT count(*) / 2 FROM single)), 0) AS REAL),
            coalesce(sum(wait < 1440), 0)
     FROM x"
  let decoder = {
    use uris <- decode.field(0, decode.int)
    use behind_100 <- decode.field(1, decode.int)
    use bulk_uris <- decode.field(2, decode.int)
    use bulk_never_heard <- decode.field(3, decode.int)
    use median_wait_min <- decode.field(4, decode.float)
    use heard_24h <- decode.field(5, decode.int)
    decode.success(QueueHealth(
      uris:,
      behind_100:,
      bulk_uris:,
      bulk_never_heard:,
      median_wait_min:,
      heard_24h:,
    ))
  }
  one(db, sql, decoder, QueueHealth(0, 0, 0, 0, 0.0, 0))
}

/// (searches, converted to an add within 30 s, zero-result searches, searches with a known result count)
pub fn search(
  db: sqlight.Connection,
  w: Window,
  user: Int,
) -> Result(#(Int, Int, Int, Int), sqlight.Error) {
  let who = case user {
    0 -> ""
    u -> " AND s.user_id = " <> n(u)
  }
  let sql = "SELECT count(*),
            coalesce(sum(EXISTS (SELECT 1 FROM queue_events q WHERE q.user_id = s.user_id AND q.event_type IN ('add','add_at_position')
                                 AND q.timestamp_ms BETWEEN s.timestamp_ms AND s.timestamp_ms + 30000)), 0),
            coalesce(sum(result_count = 0), 0), coalesce(sum(result_count IS NOT NULL), 0)
     FROM search_events s WHERE s.event_type = 'search' AND s.timestamp_ms >= " <> n(
      w.since_ms,
    ) <> who
  let decoder = {
    use a <- decode.field(0, decode.int)
    use b <- decode.field(1, decode.int)
    use c <- decode.field(2, decode.int)
    use d <- decode.field(3, decode.int)
    decode.success(#(a, b, c, d))
  }
  one(db, sql, decoder, #(0, 0, 0, 0))
}

/// (uri, plays today) for every track started on the local calendar day
pub fn today_counts(
  db: sqlight.Connection,
  w: Window,
) -> Result(List(#(String, Int)), sqlight.Error) {
  let sql = "SELECT track_uri, count(*) FROM playback_state_log
     WHERE event_type = 'track_started' AND track_uri IS NOT NULL
       AND timestamp_ms >= " <> n({ w.now_s - 86_400 } * 1000) <> "
       AND " <> day(w, "timestamp_ms") <> " = date(" <> n(w.now_s + w.tz_s) <> ", 'unixepoch')
     GROUP BY 1"
  let decoder = {
    use uri <- decode.field(0, decode.string)
    use c <- decode.field(1, decode.int)
    decode.success(#(uri, c))
  }
  sqlight.query(sql, db, [], decoder)
}

/// Plays, heard hours and distinct tracks attributed to one user
pub fn user_plays(
  db: sqlight.Connection,
  w: Window,
  user: Int,
) -> Result(#(Int, Float, Int), sqlight.Error) {
  let sql = "WITH s AS (" <> starts(w) <> ")
     SELECT count(*), CAST(coalesce(sum(heard_ms), 0) / 3600000.0 AS REAL), count(DISTINCT track_uri) FROM s WHERE user_id = " <> n(
      user,
    )
  let decoder = {
    use plays <- decode.field(0, decode.int)
    use hours <- decode.field(1, decode.float)
    use tracks <- decode.field(2, decode.int)
    decode.success(#(plays, hours, tracks))
  }
  one(db, sql, decoder, #(0, 0.0, 0))
}

/// Other people who queued the same artists as this user: (username, shared artists)
pub fn kindred(
  db: sqlight.Connection,
  w: Window,
  user: Int,
  limit: Int,
) -> Result(List(#(String, Int)), sqlight.Error) {
  let sql =
    "WITH mine AS (SELECT DISTINCT artist_name a FROM playback_state_log WHERE event_type = 'track_started' AND user_id = "
    <> n(user)
    <> "
                   AND artist_name IS NOT NULL AND artist_name != '' AND timestamp_ms >= "
    <> n(w.since_ms)
    <> ")
     SELECT u.username, count(DISTINCT p.artist_name)
     FROM playback_state_log p JOIN users u ON u.id = p.user_id
     WHERE p.event_type = 'track_started' AND p.user_id != "
    <> n(user)
    <> " AND p.user_id != "
    <> n(autoplay_user)
    <> "
       AND p.timestamp_ms >= "
    <> n(w.since_ms)
    <> " AND p.artist_name IN (SELECT a FROM mine)
     GROUP BY u.id ORDER BY 2 DESC LIMIT "
    <> n(limit)
  let decoder = {
    use name <- decode.field(0, decode.string)
    use shared <- decode.field(1, decode.int)
    decode.success(#(name, shared))
  }
  sqlight.query(sql, db, [], decoder)
}

fn one(
  db: sqlight.Connection,
  sql: String,
  decoder: decode.Decoder(a),
  default: a,
) -> Result(a, sqlight.Error) {
  sqlight.query(sql, db, [], decoder)
  |> result.map(fn(rows) {
    case rows {
      [row, ..] -> row
      [] -> default
    }
  })
}

pub type QueryStat {
  QueryStat(
    query: String,
    searches: Int,
    converted: Int,
    avg_results: Int,
    zero_results: Int,
  )
}

/// Most searched queries with their outcome; `order` is "searches" or "zero_results"
pub fn top_queries(
  db: sqlight.Connection,
  w: Window,
  limit: Int,
  order: String,
) -> Result(List(QueryStat), sqlight.Error) {
  let order_by = case order {
    "zero" -> "zero_results DESC, searches DESC"
    "lost" -> "(searches - converted) DESC, searches DESC"
    _ -> "searches DESC"
  }
  let sql = "SELECT lower(trim(s.query_text)) q, count(*) searches,
            coalesce(sum(EXISTS (SELECT 1 FROM queue_events e WHERE e.user_id = s.user_id AND e.event_type IN ('add','add_at_position')
                                 AND e.timestamp_ms BETWEEN s.timestamp_ms AND s.timestamp_ms + 30000)), 0) converted,
            CAST(coalesce(avg(s.result_count), -1) AS INTEGER) avg_results, coalesce(sum(s.result_count = 0), 0) zero_results
     FROM search_events s
     WHERE s.event_type = 'search' AND s.query_text IS NOT NULL AND trim(s.query_text) != '' AND s.timestamp_ms >= " <> n(
      w.since_ms,
    ) <> "
     GROUP BY 1 HAVING searches >= 1 ORDER BY " <> order_by <> " LIMIT " <> n(
      limit,
    )
  let decoder = {
    use query <- decode.field(0, decode.string)
    use searches <- decode.field(1, decode.int)
    use converted <- decode.field(2, decode.int)
    use avg_results <- decode.field(3, decode.int)
    use zero_results <- decode.field(4, decode.int)
    decode.success(QueryStat(
      query:,
      searches:,
      converted:,
      avg_results:,
      zero_results:,
    ))
  }
  sqlight.query(sql, db, [], decoder)
}

pub type SearchShape {
  SearchShape(
    searches: Int,
    refinements: Int,
    repeats: Int,
    short_queries: Int,
    long_queries: Int,
    median_results: Int,
    picks: Int,
    pick_top1: Int,
    pick_top3: Int,
    pick_after_10: Int,
    picks_tidal: Int,
    picks_local: Int,
    median_pick_s: Int,
  )
}

/// How people search and what they pick: refinements (another search < 3 s later),
/// exact repeats, query length, result counts, rank and backend of the picked track.
pub fn search_shape(
  db: sqlight.Connection,
  w: Window,
) -> Result(SearchShape, sqlight.Error) {
  let sql = "WITH s AS (SELECT user_id, timestamp_ms, query_text, result_count,
                       LAG(query_text) OVER (PARTITION BY user_id ORDER BY timestamp_ms) pq,
                       LAG(timestamp_ms) OVER (PARTITION BY user_id ORDER BY timestamp_ms) pt
                FROM search_events WHERE event_type = 'search' AND query_text IS NOT NULL AND timestamp_ms >= " <> n(
      w.since_ms,
    ) <> "),
     rc AS (SELECT result_count r FROM s WHERE result_count IS NOT NULL ORDER BY r),
     c AS (SELECT result_position pos, result_uri uri, time_to_action_ms t FROM search_conversions
           WHERE action = 'add' AND timestamp_ms >= " <> n(w.since_ms) <> "),
     tt AS (SELECT t FROM c WHERE t IS NOT NULL ORDER BY t)
     SELECT (SELECT count(*) FROM s),
            (SELECT coalesce(sum(timestamp_ms - pt < 3000 AND lower(query_text) != lower(coalesce(pq, ''))), 0) FROM s),
            (SELECT coalesce(sum(lower(query_text) = lower(pq)), 0) FROM s),
            (SELECT coalesce(sum(length(trim(query_text)) <= 3), 0) FROM s),
            (SELECT coalesce(sum(length(trim(query_text)) >= 13), 0) FROM s),
            coalesce((SELECT r FROM rc LIMIT 1 OFFSET (SELECT count(*) / 2 FROM rc)), -1),
            (SELECT count(*) FROM c),
            (SELECT coalesce(sum(pos = 1), 0) FROM c),
            (SELECT coalesce(sum(pos <= 3), 0) FROM c),
            (SELECT coalesce(sum(pos > 10), 0) FROM c),
            (SELECT coalesce(sum(uri LIKE 'tidal:%'), 0) FROM c),
            (SELECT coalesce(sum(uri LIKE 'local:%' OR uri LIKE 'file:%'), 0) FROM c),
            coalesce((SELECT t / 1000 FROM tt LIMIT 1 OFFSET (SELECT count(*) / 2 FROM tt)), -1)"
  let decoder = {
    use searches <- decode.field(0, decode.int)
    use refinements <- decode.field(1, decode.int)
    use repeats <- decode.field(2, decode.int)
    use short_queries <- decode.field(3, decode.int)
    use long_queries <- decode.field(4, decode.int)
    use median_results <- decode.field(5, decode.int)
    use picks <- decode.field(6, decode.int)
    use pick_top1 <- decode.field(7, decode.int)
    use pick_top3 <- decode.field(8, decode.int)
    use pick_after_10 <- decode.field(9, decode.int)
    use picks_tidal <- decode.field(10, decode.int)
    use picks_local <- decode.field(11, decode.int)
    use median_pick_s <- decode.field(12, decode.int)
    decode.success(SearchShape(
      searches:,
      refinements:,
      repeats:,
      short_queries:,
      long_queries:,
      median_results:,
      picks:,
      pick_top1:,
      pick_top3:,
      pick_after_10:,
      picks_tidal:,
      picks_local:,
      median_pick_s:,
    ))
  }
  one(db, sql, decoder, SearchShape(0, 0, 0, 0, 0, -1, 0, 0, 0, 0, 0, 0, -1))
}
