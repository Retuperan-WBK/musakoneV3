//// GET /api/stats/room, /api/stats/me, /api/stats/today — JSON for the Stats page.

import db/stats
import gleam/float
import gleam/http/response.{type Response}
import gleam/int
import gleam/json
import gleam/list
import gleam/result
import gleam/string
import gleam/time/timestamp
import handlers/http as h
import logging
import mist.{type ResponseData}
import sqlight

/// `days` 0 = everything; `tz` minutes east of UTC from the browser
pub fn window(days: Int, tz_min: Int) -> stats.Window {
  let now_s =
    timestamp.system_time() |> timestamp.to_unix_seconds() |> float.truncate
  let since_ms = case days {
    0 -> 0
    d -> { now_s - d * 86_400 } * 1000
  }
  stats.Window(since_ms:, tz_s: tz_min * 60, now_s:)
}

fn with_user(
  state: h.AppState,
  auth_header: String,
  f: fn(Int) -> Response(ResponseData),
) -> Response(ResponseData) {
  let user =
    h.extract_token(auth_header)
    |> result.try(fn(t) {
      h.verify_jwt_token(t, state.jwt_secret)
      |> result.replace_error("Invalid or expired token")
    })
    |> result.try(h.get_user_id_from_jwt)
  case user {
    Ok(uid) -> f(uid)
    Error(e) -> h.error_response(e, 401)
  }
}

fn pct(part: Int, whole: Int) -> json.Json {
  case whole {
    0 -> json.int(0)
    _ -> json.int(100 * part / whole)
  }
}

fn f1(x: Float) -> json.Json {
  json.float(float.to_precision(x, 1))
}

pub fn room(
  state: h.AppState,
  auth_header: String,
  days: Int,
  tz_min: Int,
) -> Response(ResponseData) {
  use _ <- with_user(state, auth_header)
  let w = window(days, tz_min)
  let db = state.db
  let r = {
    use t <- result.try(stats.totals(db, w))
    use per_day <- result.try(stats.per_day(db, w))
    use by_hour <- result.try(stats.by_hour(db, w))
    use by_weekday <- result.try(stats.by_weekday(db, w))
    use top_tracks <- result.try(stats.top_tracks(db, w, 0, 20))
    use top_artists <- result.try(stats.top_artists(db, w, 0, 15))
    use #(plays, repeat_plays, loop_days, play_days) <- result.try(
      stats.repeats(db, w),
    )
    use decades <- result.try(stats.decades(db, w))
    use #(nexts, early) <- result.try(stats.skips(db, w))
    use magnets <- result.try(stats.skip_magnets(db, w, 5))
    use djs <- result.try(stats.djs(db, w, 15))
    use q <- result.try(stats.queue_health(db, w, 0))
    use #(searches, converted, zero, counted) <- result.try(stats.search(
      db,
      w,
      0,
    ))
    use shape <- result.try(stats.search_shape(db, w))
    use top_q <- result.try(stats.top_queries(db, w, 15, "searches"))
    use zero_q <- result.try(stats.top_queries(db, w, 10, "zero"))
    use lost_q <- result.try(stats.top_queries(db, w, 10, "lost"))
    Ok(
      json.object([
        #("days", json.int(days)),
        #("since_ms", json.int(w.since_ms)),
        #(
          "totals",
          json.object([
            #("plays", json.int(t.plays)),
            #("tracks", json.int(t.tracks)),
            #("artists", json.int(t.artists)),
            #("play_days", json.int(t.play_days)),
            #("hours", f1(t.hours)),
            #("sessions", json.int(t.sessions)),
            #("djs", json.int(t.djs)),
            #("adds", json.int(t.adds)),
            #("searches", json.int(t.searches)),
          ]),
        ),
        #(
          "per_day",
          json.array(per_day, fn(d) {
            json.object([
              #("date", json.string(d.0)),
              #("plays", json.int(d.1)),
              #("hours", f1(d.2)),
              #("tracks", json.int(d.3)),
              #("adds", json.int(d.4)),
            ])
          }),
        ),
        #(
          "by_hour",
          json.array(by_hour, fn(b) { json.array([b.0, b.1], json.int) }),
        ),
        #(
          "by_weekday",
          json.array(by_weekday, fn(b) { json.array([b.0, b.1], json.int) }),
        ),
        #("top_tracks", json.array(top_tracks, track_json)),
        #(
          "top_artists",
          json.array(top_artists, fn(a) {
            json.object([
              #("artist", json.string(a.0)),
              #("plays", json.int(a.1)),
              #("tracks", json.int(a.2)),
            ])
          }),
        ),
        #(
          "repeats",
          json.object([
            #("plays", json.int(plays)),
            #("same_day_repeat_pct", pct(repeat_plays, plays)),
            #("loop_days", json.int(loop_days)),
            #("play_days", json.int(play_days)),
          ]),
        ),
        #(
          "decades",
          json.array(decades, fn(d) {
            json.object([
              #("decade", json.string(d.0)),
              #("plays", json.int(d.1)),
            ])
          }),
        ),
        #(
          "skips",
          json.object([
            #("next_commands", json.int(nexts)),
            #("early_pct", pct(early, nexts)),
            #(
              "magnets",
              json.array(magnets, fn(m) {
                json.object([
                  #("artist", json.string(m.0)),
                  #("early_skips", json.int(m.1)),
                  #("plays", json.int(m.2)),
                ])
              }),
            ),
          ]),
        ),
        #(
          "djs",
          json.array(djs, fn(d) {
            json.object([
              #("user_id", json.int(d.user_id)),
              #("username", json.string(d.username)),
              #("adds", json.int(d.adds)),
              #("tracks_added", json.int(d.uris)),
              #("heard_24h_pct", pct(d.heard_24h, d.uris)),
              #("plays_attributed", json.int(d.plays_attributed)),
            ])
          }),
        ),
        #("queue", queue_json(q)),
        #(
          "search",
          json.object([
            #("searches", json.int(searches)),
            #("converted_30s_pct", pct(converted, searches)),
            #("zero_result_pct", pct(zero, counted)),
            #("with_result_count", json.int(counted)),
            #("refinement_pct", pct(shape.refinements, shape.searches)),
            #("repeat_pct", pct(shape.repeats, shape.searches)),
            #("short_query_pct", pct(shape.short_queries, shape.searches)),
            #("long_query_pct", pct(shape.long_queries, shape.searches)),
            #("median_results", json.int(shape.median_results)),
            #("picks", json.int(shape.picks)),
            #("pick_top1_pct", pct(shape.pick_top1, shape.picks)),
            #("pick_top3_pct", pct(shape.pick_top3, shape.picks)),
            #("pick_after_10_pct", pct(shape.pick_after_10, shape.picks)),
            #("picks_tidal_pct", pct(shape.picks_tidal, shape.picks)),
            #("picks_local_pct", pct(shape.picks_local, shape.picks)),
            #("median_pick_s", json.int(shape.median_pick_s)),
            #("top_queries", json.array(top_q, query_json)),
            #(
              "zero_result_queries",
              json.array(
                list.filter(zero_q, fn(q) { q.zero_results > 0 }),
                query_json,
              ),
            ),
            #(
              "lost_queries",
              json.array(
                list.filter(lost_q, fn(q) { q.searches > q.converted }),
                query_json,
              ),
            ),
          ]),
        ),
      ]),
    )
  }
  respond(r)
}

fn query_json(q: stats.QueryStat) -> json.Json {
  json.object([
    #("query", json.string(q.query)),
    #("searches", json.int(q.searches)),
    #("converted_pct", pct(q.converted, q.searches)),
    #("avg_results", json.int(q.avg_results)),
    #("zero_results", json.int(q.zero_results)),
  ])
}

pub fn me(
  state: h.AppState,
  auth_header: String,
  days: Int,
  tz_min: Int,
) -> Response(ResponseData) {
  use uid <- with_user(state, auth_header)
  let w = window(days, tz_min)
  let db = state.db
  let r = {
    use #(plays, hours, tracks) <- result.try(stats.user_plays(db, w, uid))
    use q <- result.try(stats.queue_health(db, w, uid))
    use room_q <- result.try(stats.queue_health(db, w, 0))
    use top_tracks <- result.try(stats.top_tracks(db, w, uid, 10))
    use top_artists <- result.try(stats.top_artists(db, w, uid, 10))
    use #(searches, converted, _, _) <- result.try(stats.search(db, w, uid))
    use djs <- result.try(stats.djs(db, w, 100))
    use kindred <- result.try(stats.kindred(db, w, uid, 5))
    let rank =
      list.index_map(djs, fn(d, i) { #(d.user_id, i + 1) })
      |> list.key_find(uid)
      |> result.unwrap(0)
    Ok(
      json.object([
        #("days", json.int(days)),
        #("user_id", json.int(uid)),
        #(
          "plays",
          json.object([
            #("plays", json.int(plays)),
            #("hours", f1(hours)),
            #("tracks", json.int(tracks)),
          ]),
        ),
        #("queue", queue_json(q)),
        #("share_of_room_pct", pct(q.uris, room_q.uris)),
        #("dj_rank", json.int(rank)),
        #("dj_count", json.int(list.length(djs))),
        #("top_tracks", json.array(top_tracks, track_json)),
        #(
          "top_artists",
          json.array(top_artists, fn(a) {
            json.object([
              #("artist", json.string(a.0)),
              #("plays", json.int(a.1)),
              #("tracks", json.int(a.2)),
            ])
          }),
        ),
        #(
          "search",
          json.object([
            #("searches", json.int(searches)),
            #("converted_30s_pct", pct(converted, searches)),
          ]),
        ),
        #(
          "kindred",
          json.array(kindred, fn(k) {
            json.object([
              #("username", json.string(k.0)),
              #("shared_artists", json.int(k.1)),
            ])
          }),
        ),
        #(
          "attribution_note",
          json.string(
            "Plays are credited to whoever queued the track; reliable from September 2026.",
          ),
        ),
      ]),
    )
  }
  respond(r)
}

/// { "<uri>": plays today, ... } for queue badges
pub fn today(
  state: h.AppState,
  auth_header: String,
  tz_min: Int,
) -> Response(ResponseData) {
  use _ <- with_user(state, auth_header)
  let w = window(1, tz_min)
  stats.today_counts(state.db, w)
  |> result.map(fn(rows) {
    json.object(list.map(rows, fn(r) { #(r.0, json.int(r.1)) }))
  })
  |> respond
}

fn track_json(t: stats.TopTrack) -> json.Json {
  json.object([
    #("uri", json.string(t.uri)),
    #("name", json.string(t.name)),
    #("artist", json.string(t.artist)),
    #("plays", json.int(t.plays)),
    #("days", json.int(t.days)),
    #("last_played_ms", json.int(t.last_ms)),
    #("heard_pct", json.int(int.min(t.heard_pct, 100))),
  ])
}

fn queue_json(q: stats.QueueHealth) -> json.Json {
  json.object([
    #("tracks_added", json.int(q.uris)),
    #("heard_24h_pct", pct(q.heard_24h, q.uris)),
    #("behind_100_pct", pct(q.behind_100, q.uris)),
    #("bulk_pct", pct(q.bulk_uris, q.uris)),
    #("bulk_never_heard_pct", pct(q.bulk_never_heard, q.bulk_uris)),
    #("median_wait_min", f1(q.median_wait_min)),
  ])
}

fn respond(r: Result(json.Json, sqlight.Error)) -> Response(ResponseData) {
  case r {
    Ok(j) -> h.respond_json(json.to_string(j), 200)
    Error(e) -> {
      logging.log(logging.Error, "stats query failed: " <> string.inspect(e))
      h.error_response("Stats query failed: " <> e.message, 500)
    }
  }
}
