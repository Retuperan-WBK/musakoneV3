/// HTTP handlers for the "Mix" page: recommendations, autoplay control, insights.
import autoplay
import db/queries
import gleam/dict
import gleam/dynamic/decode
import gleam/erlang/process
import gleam/float
import gleam/http/response.{type Response}
import gleam/int
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import gleam/time/timestamp
import handlers/http.{type AppState}
import mist.{type ResponseData}
import mopidy_rpc
import playback_state
import recommend

const day_ms = 86_400_000

/// Run `next` with the authenticated user id, or answer 401
fn authed(
  state: AppState,
  auth_header: String,
  next: fn(Int) -> Response(ResponseData),
) -> Response(ResponseData) {
  case http.extract_token(auth_header) {
    Error(e) -> http.error_response(e, 401)
    Ok(token) ->
      case http.verify_jwt_token(token, state.jwt_secret) {
        Error(e) ->
          http.error_response("Invalid or expired token: " <> string.inspect(e), 401)
        Ok(jwt_data) ->
          case http.get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> next(user_id)
            Error(e) -> http.error_response("Invalid token: " <> e, 401)
          }
      }
  }
}

// ─── Recommendations ────────────────────────────────────────────────

pub fn get_recommendations(
  state: AppState,
  auth_header: String,
  scope_param: String,
  limit: Int,
) -> Response(ResponseData) {
  use user_id <- authed(state, auth_header)
  let scope = case scope_param {
    "room" -> recommend.ForRoom
    _ -> recommend.ForUser(user_id)
  }
  let queued =
    mopidy_rpc.get_tl_tracks(state.rpc)
    |> result.unwrap([])
    |> list.map(fn(t) { t.track.uri })
  let discovery = autoplay.load_settings(state.db).discovery
  let opts =
    recommend.Options(
      limit: int.clamp(limit, 1, 50),
      discovery: discovery,
      exclude_uris: queued,
    )

  case recommend.recommend(state.db, state.rpc, scope, opts) {
    Ok(items) ->
      json.object([
        #("scope", json.string(scope_param)),
        #("generated_at", json.int(now_ms())),
        #("items", json.array(items, recommend.to_json)),
      ])
      |> json.to_string
      |> http.respond_json(200)
    Error(e) -> http.error_response("Could not build recommendations: " <> e, 502)
  }
}

// ─── Autoplay ───────────────────────────────────────────────────────

pub fn get_autoplay(state: AppState, auth_header: String) -> Response(ResponseData) {
  use _user_id <- authed(state, auth_header)
  autoplay_status(state) |> json.to_string |> http.respond_json(200)
}

pub fn put_autoplay(
  state: AppState,
  auth_header: String,
  body: String,
) -> Response(ResponseData) {
  use _user_id <- authed(state, auth_header)
  let current = autoplay.load_settings(state.db)

  let number = decode.one_of(decode.float, [decode.int |> decode.map(int.to_float)])
  let patch_decoder = {
    use enabled <- decode.optional_field("enabled", None, decode.optional(decode.bool))
    use min_ahead <- decode.optional_field("min_ahead", None, decode.optional(decode.int))
    use batch_size <- decode.optional_field("batch_size", None, decode.optional(decode.int))
    use discovery <- decode.optional_field("discovery", None, decode.optional(number))
    use mode <- decode.optional_field("mode", None, decode.optional(decode.string))
    decode.success(#(enabled, min_ahead, batch_size, discovery, mode))
  }

  case json.parse(body, patch_decoder) {
    Error(_) -> http.error_response("Invalid autoplay settings payload", 400)
    Ok(#(enabled, min_ahead, batch_size, discovery, mode)) -> {
      let mode = option.unwrap(mode, current.mode)
      let valid_mode = mode == "room" || {
        case mode {
          "user:" <> id -> result.is_ok(int.parse(id))
          _ -> False
        }
      }
      case valid_mode {
        False -> http.error_response("mode must be \"room\" or \"user:<id>\"", 400)
        True -> {
          let updated =
            autoplay.Settings(
              enabled: option.unwrap(enabled, current.enabled),
              min_ahead: int.clamp(option.unwrap(min_ahead, current.min_ahead), 0, 20),
              batch_size: int.clamp(option.unwrap(batch_size, current.batch_size), 1, 20),
              discovery: float.clamp(option.unwrap(discovery, current.discovery), 0.0, 1.0),
              mode: mode,
            )
          case autoplay.save_settings(state.db, updated) {
            Error(e) -> http.error_response(e, 500)
            Ok(Nil) -> {
              process.send(state.autoplay, autoplay.ReloadSettings)
              autoplay_status(state) |> json.to_string |> http.respond_json(200)
            }
          }
        }
      }
    }
  }
}

pub fn post_autoplay_fill(state: AppState, auth_header: String) -> Response(ResponseData) {
  use _user_id <- authed(state, auth_header)
  // Building a batch can take a while when Tidal listings are not cached yet
  case process.call(state.autoplay, 90_000, autoplay.Fill) {
    Ok(count) ->
      json.object([#("added", json.int(count))])
      |> json.to_string
      |> http.respond_json(200)
    Error(e) -> http.error_response("Autoplay could not fill the queue: " <> e, 409)
  }
}

fn autoplay_status(state: AppState) -> json.Json {
  let settings = autoplay.load_settings(state.db)
  let now = now_ms()
  let added_24h =
    queries.scalar_int(
      state.db,
      "SELECT COUNT(*) FROM queue_events WHERE event_type = 'autoplay' AND timestamp_ms > ?",
      [sqlight_int(now - day_ms)],
    )
    |> result.unwrap(0)
  json.object([
    #("settings", autoplay.settings_to_json(settings)),
    #("added_24h", json.int(added_24h)),
  ])
}

// ─── Insights ───────────────────────────────────────────────────────

pub fn get_insights(state: AppState, auth_header: String) -> Response(ResponseData) {
  use user_id <- authed(state, auth_header)
  let db = state.db
  let now = now_ms()

  // Room totals
  let plays =
    queries.scalar_int(db, "SELECT COUNT(*) FROM playback_state_log WHERE event_type = 'track_started'", [])
    |> result.unwrap(0)
  let plays_24h =
    queries.scalar_int(
      db,
      "SELECT COUNT(*) FROM playback_state_log WHERE event_type = 'track_started' AND timestamp_ms > ?",
      [sqlight_int(now - day_ms)],
    )
    |> result.unwrap(0)
  let listen_ms =
    queries.scalar_int(
      db,
      "SELECT COALESCE(SUM(position_ms), 0) FROM playback_state_log WHERE event_type = 'track_ended'",
      [],
    )
    |> result.unwrap(0)
  let queue_adds =
    queries.scalar_int(
      db,
      "SELECT COUNT(*) FROM queue_events WHERE event_type IN ('add', 'add_at_position')",
      [],
    )
    |> result.unwrap(0)
  let autoplay_adds_24h =
    queries.scalar_int(
      db,
      "SELECT COUNT(*) FROM queue_events WHERE event_type = 'autoplay' AND timestamp_ms > ?",
      [sqlight_int(now - day_ms)],
    )
    |> result.unwrap(0)
  let user_count =
    queries.scalar_int(db, "SELECT COUNT(*) FROM users WHERE username != 'autoplay'", [])
    |> result.unwrap(0)

  // Users
  let all_users = queries.get_all_users_with_activity(db) |> result.unwrap([])
  let usernames =
    all_users
    |> list.map(fn(u) { #(u.0, u.1) })
    |> dict.from_list
  let username_of = fn(id: Option(Int)) -> Option(String) {
    case id {
      Some(id) -> dict.get(usernames, id) |> option.from_result
      None -> None
    }
  }
  let active_users = queries.get_active_users(db, now - 7 * day_ms) |> result.unwrap([])

  // Top content
  let room_tracks = queries.get_room_top_tracks(db, 10) |> result.unwrap([])
  let my_tracks = queries.get_user_track_affinities(db, user_id, 8) |> result.unwrap([])
  let meta =
    queries.get_track_meta_many(
      db,
      list.append(
        list.map(room_tracks, fn(t) { t.0 }),
        list.map(my_tracks, fn(t) { t.track_uri }),
      ),
    )
    |> result.unwrap([])
    |> list.map(fn(m) { #(m.uri, m) })
    |> dict.from_list
  let track_json = fn(uri: String, score: Float, extra: List(#(String, json.Json))) {
    let m = dict.get(meta, uri)
    json.object(
      list.append(
        [
          #("uri", json.string(uri)),
          #("name", json.nullable(result.try(m, fn(m) { option.to_result(m.name, Nil) }) |> option.from_result, json.string)),
          #("artist", json.nullable(result.try(m, fn(m) { option.to_result(m.artist_name, Nil) }) |> option.from_result, json.string)),
          #("plays", json.int(result.map(m, fn(m) { m.play_count }) |> result.unwrap(0))),
          #("score", json.float(score)),
        ],
        extra,
      ),
    )
  }

  let room_artists = queries.get_room_top_artists(db, 10) |> result.unwrap([])
  let taste_map =
    queries.get_users_top_artists(db, 3)
    |> result.unwrap([])
    |> list.group(fn(row) { row.0 })
    |> dict.to_list
    |> list.filter_map(fn(entry) {
      let #(uid, rows) = entry
      case dict.get(usernames, uid) {
        Error(_) -> Error(Nil)
        Ok(name) if name == "autoplay" -> Error(Nil)
        Ok(name) ->
          Ok(
            json.object([
              #("user_id", json.int(uid)),
              #("username", json.string(name)),
              #(
                "artists",
                json.array(list.reverse(rows), fn(r) {
                  json.object([#("name", json.string(r.1)), #("score", json.float(r.2))])
                }),
              ),
            ]),
          )
      }
    })

  // Timelines and activity
  let hourly = queries.get_hourly_plays(db, now - 7 * day_ms) |> result.unwrap([])
  let daily = queries.get_daily_plays(db, now - 14 * day_ms) |> result.unwrap([])
  let recent_adds = queries.get_recent_queue_adds(db, 15) |> result.unwrap([])
  let recent_plays = queries.get_recent_plays(db, 15) |> result.unwrap([])

  // Me
  let my_artists = queries.get_user_artist_affinities(db, user_id, 8) |> result.unwrap([])
  let my_stats = queries.get_user_stats(db, user_id) |> result.unwrap([])
  let my_listen_ms =
    queries.scalar_int(
      db,
      "SELECT COALESCE(SUM(total_listen_ms), 0) FROM user_track_affinity WHERE user_id = ?",
      [sqlight_int(user_id)],
    )
    |> result.unwrap(0)

  let now_playing = process.call(state.playback_state, 2000, playback_state.GetState)

  json.object([
    #("generated_at", json.int(now)),
    #("now_playing", http.encode_playback_snapshot(now_playing)),
    #("autoplay", autoplay_status(state)),
    #(
      "room",
      json.object([
        #(
          "totals",
          json.object([
            #("plays", json.int(plays)),
            #("plays_24h", json.int(plays_24h)),
            #("listen_minutes", json.int(listen_ms / 60_000)),
            #("queue_adds", json.int(queue_adds)),
            #("autoplay_adds_24h", json.int(autoplay_adds_24h)),
            #("users", json.int(user_count)),
          ]),
        ),
        #(
          "top_tracks",
          json.array(room_tracks, fn(t) {
            track_json(t.0, t.1, [#("users", json.int(t.2))])
          }),
        ),
        #(
          "top_artists",
          json.array(room_artists, fn(a) {
            json.object([
              #("name", json.string(a.0)),
              #("score", json.float(a.1)),
              #("users", json.int(a.2)),
              #("plays", json.int(a.3)),
            ])
          }),
        ),
        #("taste_map", json.preprocessed_array(taste_map)),
        #(
          "hourly",
          json.array(hourly, fn(h) {
            json.object([#("hour", json.int(h.0)), #("plays", json.int(h.1))])
          }),
        ),
        #(
          "daily",
          json.array(daily, fn(d) {
            json.object([#("day", json.string(d.0)), #("plays", json.int(d.1))])
          }),
        ),
        #(
          "active_users",
          json.array(active_users, fn(u) {
            json.object([
              #("id", json.int(u.0)),
              #("username", json.string(u.1)),
              #("last_activity", json.int(u.2)),
            ])
          }),
        ),
        #(
          "recent_adds",
          json.array(recent_adds, fn(a) {
            let #(ts, username, names, _uris, event_type) = a
            json.object([
              #("timestamp_ms", json.int(ts)),
              #("username", json.string(username)),
              #("tracks", json.array(parse_string_list(names), json.string)),
              #("autoplay", json.bool(event_type == "autoplay")),
            ])
          }),
        ),
        #(
          "recent_plays",
          json.array(recent_plays, fn(p) {
            let #(ts, uri, name, artist, uid) = p
            json.object([
              #("timestamp_ms", json.int(ts)),
              #("uri", json.nullable(uri, json.string)),
              #("name", json.nullable(name, json.string)),
              #("artist", json.nullable(artist, json.string)),
              #("username", json.nullable(username_of(uid), json.string)),
            ])
          }),
        ),
      ]),
    ),
    #(
      "me",
      json.object([
        #("user_id", json.int(user_id)),
        #(
          "top_artists",
          json.array(my_artists, fn(a) {
            json.object([
              #("name", json.string(a.artist_name)),
              #("score", json.float(a.affinity_score)),
              #("plays", json.int(a.play_count)),
            ])
          }),
        ),
        #(
          "top_tracks",
          json.array(my_tracks, fn(t) {
            track_json(t.track_uri, t.affinity_score, [
              #("my_plays", json.int(t.play_count)),
            ])
          }),
        ),
        #(
          "stats",
          json.object(
            list.append(
              list.map(my_stats, fn(s) { #(s.0, json.int(s.1)) }),
              [#("listen_minutes", json.int(my_listen_ms / 60_000))],
            ),
          ),
        ),
      ]),
    ),
  ])
  |> json.to_string
  |> http.respond_json(200)
}

/// queue_events.track_names holds a JSON array of strings (or nothing)
fn parse_string_list(raw: Option(String)) -> List(String) {
  case raw {
    None -> []
    Some(text) ->
      json.parse(text, decode.list(decode.string))
      |> result.unwrap([text])
  }
}

fn sqlight_int(n: Int) -> sqlight.Value {
  sqlight.int(n)
}

import sqlight

fn now_ms() -> Int {
  timestamp.system_time()
  |> timestamp.to_unix_seconds()
  |> float.multiply(1000.0)
  |> float.round
}
