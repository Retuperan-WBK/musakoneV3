import auth/jwt.{type Jwt, type Verified}
import auth/service
import autoplay.{type AutoplayMessage}
import db/queries
import event_bus.{type BusMessage}
import gleam/bytes_tree
import gleam/dynamic/decode
import gleam/erlang/process.{type Subject}
import gleam/float
import gleam/http/response.{type Response}
import gleam/int
import gleam/json
import gleam/list
import gleam/option.{type Option, None}
import gleam/result
import gleam/string
import gleam/time/timestamp
import logging
import mist.{type ResponseData}
import mopidy_rpc.{type RpcMessage}
import playback_state.{type PlaybackStateMessage, type PlaybackStateSnapshot}
import sqlight

pub type AppState {
  AppState(
    db: sqlight.Connection,
    jwt_secret: String,
    event_bus: Subject(BusMessage),
    playback_state: Subject(PlaybackStateMessage),
    rpc: Subject(RpcMessage),
    autoplay: Subject(AutoplayMessage),
  )
}

pub type LoginRequest {
  LoginRequest(username: String, password: String)
}

/// Health check endpoint
pub fn health_check() -> Response(ResponseData) {
  json.object([
    #("status", json.string("ok")),
    #("service", json.string("musakone-backend")),
    #(
      "timestamp",
      json.int(
        timestamp.system_time()
        |> timestamp.to_unix_seconds()
        |> float.round,
      ),
    ),
  ])
  |> json.to_string
  |> respond_json(200)
}

/// Login endpoint
pub fn login(state: AppState, body: String) -> Response(ResponseData) {
  case parse_login_request(body) {
    Ok(req) -> {
      case service.authenticate(state.db, req.username, req.password) {
        Ok(user) -> {
          // Generate JWT token
          case create_jwt_token(user, state.jwt_secret) {
            Ok(token) -> {
              json.object([
                #("token", json.string(token)),
                #(
                  "user",
                  json.object([
                    #("id", json.int(user.id)),
                    #("username", json.string(user.username)),
                    #("created_at", json.int(user.created_at)),
                  ]),
                ),
              ])
              |> json.to_string
              |> respond_json(200)
            }
            Error(e) -> {
              logging.log(logging.Error, "Failed to create JWT: " <> e)
              error_response("Failed to create authentication token", 500)
            }
          }
        }
        Error(_) -> error_response("Invalid username or password", 401)
      }
    }
    Error(e) -> error_response("Invalid request: " <> e, 400)
  }
}

/// Register endpoint
pub fn register(state: AppState, body: String) -> Response(ResponseData) {
  let _ = logging.log(logging.Debug, "Register request received: " <> body)
  case parse_login_request(body) {
    Ok(req) -> {
      let _ = case service.create_user(state.db, req.username, req.password) {
        Ok(user) -> {
          let response_json =
            json.object([
              #("id", json.int(user.id)),
              #("username", json.string(user.username)),
              #("created_at", json.int(user.created_at)),
            ])
            |> json.to_string
          let _ =
            logging.log(
              logging.Info,
              "Registering successful: " <> response_json,
            )
          respond_json(response_json, 201)
        }
        Error(e) -> {
          let _ = logging.log(logging.Error, "Error creating user: " <> e)
          case string.contains(e, "UNIQUE constraint failed") {
            True -> error_response("Username already exists", 409)
            False -> error_response("Could not create user", 500)
          }
        }
      }
    }
    Error(e) -> {
      let _ = logging.log(logging.Error, "Error parsing payload: " <> e)
      error_response("Invalid request: " <> e, 400)
    }
  }
}

/// Get current user from token
pub fn me(state: AppState, auth_header: String) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              // Query database to get full user data
              case queries.get_user_by_id(state.db, user_id) {
                Ok([user, ..]) -> {
                  json.object([
                    #("id", json.int(user.id)),
                    #("username", json.string(user.username)),
                    #("created_at", json.int(user.created_at)),
                  ])
                  |> json.to_string
                  |> respond_json(200)
                }
                Ok([]) -> error_response("User not found", 404)
                Error(_) -> error_response("Database error", 500)
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) -> {
          error_response("Invalid or expired token: " <> string.inspect(e), 401)
        }
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Get recent events from all tables for a user
pub fn get_events(
  state: AppState,
  auth_header: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              let counts =
                queries.get_event_counts(state.db)
                |> result.unwrap([])

              let playback =
                queries.get_playback_events(state.db, user_id, 50)
                |> result.unwrap([])
              let queue =
                queries.get_queue_events(state.db, user_id, 50)
                |> result.unwrap([])
              let search =
                queries.get_search_events(state.db, user_id, 50)
                |> result.unwrap([])

              json.object([
                #(
                  "counts",
                  json.object(
                    list.map(counts, fn(c) {
                      let #(tbl, cnt) = c
                      #(tbl, json.int(cnt))
                    }),
                  ),
                ),
                #("offset", json.int(0)),
                #("limit", json.int(50)),
                #("playback", json.array(playback, encode_playback_event)),
                #("queue", json.array(queue, encode_queue_event)),
                #("search", json.array(search, encode_search_event)),
              ])
              |> json.to_string
              |> respond_json(200)
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) -> {
          error_response("Invalid or expired token: " <> string.inspect(e), 401)
        }
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Get user statistics
pub fn get_stats(state: AppState, auth_header: String) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case queries.get_user_stats(state.db, user_id) {
                Ok(stats) -> {
                  let stats_json =
                    stats
                    |> list.map(fn(stat) {
                      let #(action_type, count) = stat
                      #(action_type, json.int(count))
                    })
                    |> json.object

                  stats_json
                  |> json.to_string
                  |> respond_json(200)
                }
                Error(_) -> error_response("Failed to get statistics", 500)
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) -> {
          error_response("Invalid or expired token: " <> string.inspect(e), 401)
        }
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

// Helper functions

fn parse_login_request(body: String) -> Result(LoginRequest, String) {
  let decoder = {
    use username <- decode.field("username", decode.string)
    use password <- decode.field("password", decode.string)
    decode.success(LoginRequest(username:, password:))
  }

  json.parse(body, decoder)
  |> result.map_error(fn(e) { "Invalid JSON format" <> string.inspect(e) })
}

// ============================================================================
// JSON ENCODERS FOR EVENT TYPES
// ============================================================================

fn encode_playback_event(event: queries.PlaybackEvent) -> json.Json {
  json.object([
    #("id", json.int(event.id)),
    #("user_id", json.int(event.user_id)),
    #("timestamp_ms", json.int(event.timestamp_ms)),
    #("event_type", json.string(event.event_type)),
    #("track_uri", json.nullable(event.track_uri, json.string)),
    #("track_name", json.nullable(event.track_name, json.string)),
    #("artist_name", json.nullable(event.artist_name, json.string)),
    #("album_name", json.nullable(event.album_name, json.string)),
    #("track_duration_ms", json.nullable(event.track_duration_ms, json.int)),
    #("position_ms", json.nullable(event.position_ms, json.int)),
    #("seek_to_ms", json.nullable(event.seek_to_ms, json.int)),
    #("volume_level", json.nullable(event.volume_level, json.int)),
    #("playback_flags", json.nullable(event.playback_flags, json.string)),
  ])
}

fn encode_queue_event(event: queries.QueueEvent) -> json.Json {
  json.object([
    #("id", json.int(event.id)),
    #("user_id", json.int(event.user_id)),
    #("timestamp_ms", json.int(event.timestamp_ms)),
    #("event_type", json.string(event.event_type)),
    #("track_uris", json.nullable(event.track_uris, json.string)),
    #("track_names", json.nullable(event.track_names, json.string)),
    #("at_position", json.nullable(event.at_position, json.int)),
    #("from_position", json.nullable(event.from_position, json.int)),
    #("to_position", json.nullable(event.to_position, json.int)),
    #("queue_length", json.nullable(event.queue_length, json.int)),
  ])
}

fn encode_search_event(event: queries.SearchEvent) -> json.Json {
  json.object([
    #("id", json.int(event.id)),
    #("user_id", json.int(event.user_id)),
    #("timestamp_ms", json.int(event.timestamp_ms)),
    #("event_type", json.string(event.event_type)),
    #("query_text", json.nullable(event.query_text, json.string)),
    #("browse_uri", json.nullable(event.browse_uri, json.string)),
    #("result_count", json.nullable(event.result_count, json.int)),
  ])
}

/// Export all events for ML training (paginated, all users)
pub fn export_ml_data(
  state: AppState,
  auth_header: String,
  offset: Int,
  limit: Int,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(_jwt_data) -> {
          let counts =
            queries.get_event_counts(state.db)
            |> result.unwrap([])

          let playback =
            queries.export_playback_events(state.db, offset, limit)
            |> result.unwrap([])
          let queue =
            queries.export_queue_events(state.db, offset, limit)
            |> result.unwrap([])
          let search =
            queries.export_search_events(state.db, offset, limit)
            |> result.unwrap([])

          json.object([
            #(
              "counts",
              json.object(
                list.map(counts, fn(c) {
                  let #(tbl, cnt) = c
                  #(tbl, json.int(cnt))
                }),
              ),
            ),
            #("offset", json.int(offset)),
            #("limit", json.int(limit)),
            #("playback", json.array(playback, encode_playback_event)),
            #("queue", json.array(queue, encode_queue_event)),
            #("search", json.array(search, encode_search_event)),
          ])
          |> json.to_string
          |> respond_json(200)
        }
        Error(e) -> {
          error_response("Invalid or expired token: " <> string.inspect(e), 401)
        }
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

// ============================================================================
// ADMIN ANALYTICS ENDPOINTS (all users data)
// ============================================================================

/// Get comprehensive admin dashboard data
pub fn get_admin_dashboard(
  state: AppState,
  auth_header: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(_jwt_data) -> {
          // Get all dashboard data in parallel
          let user_activity = queries.get_user_activity_summary(state.db)
          let hourly_activity = queries.get_hourly_activity(state.db)
          let popular_tracks = queries.get_popular_tracks(state.db, 10)
          let popular_searches = queries.get_popular_searches(state.db, 10)
          let event_distribution = queries.get_event_type_distribution(state.db)
          let all_users = queries.get_all_users_with_activity(state.db)
          let total_counts = queries.get_event_counts(state.db)
          let now_playing =
            process.call(
              state.playback_state,
              2000,
              playback_state.GetState,
            )
          let state_timeline =
            queries.get_playback_state_history(state.db, 20)

          json.object([
            #(
              "user_activity",
              json.array(result.unwrap(user_activity, []), fn(summary) {
                let #(username, playback, queue, search, total) = summary
                json.object([
                  #("username", json.string(username)),
                  #("playback_events", json.int(playback)),
                  #("queue_events", json.int(queue)),
                  #("search_events", json.int(search)),
                  #("total_events", json.int(total)),
                ])
              }),
            ),
            #(
              "hourly_activity",
              json.array(result.unwrap(hourly_activity, []), fn(hourly) {
                let #(hour, events) = hourly
                json.object([
                  #("hour", json.int(hour)),
                  #("events", json.int(events)),
                ])
              }),
            ),
            #(
              "popular_tracks",
              json.array(result.unwrap(popular_tracks, []), fn(track) {
                let #(name, artist, score, users) = track
                json.object([
                  #("name", json.string(name)),
                  #("artist", json.string(artist)),
                  #("score", json.float(score)),
                  #("unique_users", json.int(users)),
                ])
              }),
            ),
            #(
              "popular_searches",
              json.array(result.unwrap(popular_searches, []), fn(search) {
                let #(query, searches, users) = search
                json.object([
                  #("query", json.string(query)),
                  #("search_count", json.int(searches)),
                  #("unique_users", json.int(users)),
                ])
              }),
            ),
            #(
              "event_distribution",
              json.array(result.unwrap(event_distribution, []), fn(dist) {
                let #(event_type, count) = dist
                json.object([
                  #("event_type", json.string(event_type)),
                  #("count", json.int(count)),
                ])
              }),
            ),
            #(
              "users",
              json.array(result.unwrap(all_users, []), fn(user) {
                let #(id, username, last_activity, total_events) = user
                json.object([
                  #("id", json.int(id)),
                  #("username", json.string(username)),
                  #("last_activity", json.nullable(last_activity, json.int)),
                  #("total_events", json.int(total_events)),
                ])
              }),
            ),
            #(
              "totals",
              json.object(
                list.map(result.unwrap(total_counts, []), fn(c) {
                  let #(tbl, cnt) = c
                  #(tbl, json.int(cnt))
                }),
              ),
            ),
            #("now_playing", encode_playback_snapshot(now_playing)),
            #(
              "state_timeline",
              json.array(
                result.unwrap(state_timeline, []),
                encode_state_log_entry,
              ),
            ),
          ])
          |> json.to_string
          |> respond_json(200)
        }
        Error(e) -> {
          error_response("Invalid or expired token: " <> string.inspect(e), 401)
        }
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

// ============================================================================
// PLAYBACK STATE ENDPOINTS
// ============================================================================

/// Get current playback state — no auth required (public "now playing")
pub fn get_playback_state(state: AppState) -> Response(ResponseData) {
  let snapshot =
    process.call(state.playback_state, 2000, playback_state.GetState)

  encode_playback_snapshot(snapshot)
  |> json.to_string
  |> respond_json(200)
}

/// Get playback state history — auth required
pub fn get_playback_history(
  state: AppState,
  auth_header: String,
  limit: Int,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(_jwt_data) -> {
          case queries.get_playback_state_history(state.db, limit) {
            Ok(entries) -> {
              json.object([
                #("entries", json.array(entries, encode_state_log_entry)),
                #("limit", json.int(limit)),
              ])
              |> json.to_string
              |> respond_json(200)
            }
            Error(_) -> error_response("Failed to get playback history", 500)
          }
        }
        Error(e) -> {
          error_response("Invalid or expired token: " <> string.inspect(e), 401)
        }
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

pub fn encode_playback_snapshot(snapshot: PlaybackStateSnapshot) -> json.Json {
  json.object([
    #("playback_state", json.nullable(snapshot.playback_state, json.string)),
    #("track_uri", json.nullable(snapshot.track_uri, json.string)),
    #("track_name", json.nullable(snapshot.track_name, json.string)),
    #("artist_name", json.nullable(snapshot.artist_name, json.string)),
    #("album_name", json.nullable(snapshot.album_name, json.string)),
    #("track_duration_ms", json.nullable(snapshot.track_duration_ms, json.int)),
    #("position_ms", json.nullable(snapshot.position_ms, json.int)),
    #("volume", json.nullable(snapshot.volume, json.int)),
    #("queue_length", json.int(snapshot.queue_length)),
  ])
}

fn encode_state_log_entry(entry: queries.PlaybackStateLogEntry) -> json.Json {
  json.object([
    #("id", json.int(entry.id)),
    #("timestamp_ms", json.int(entry.timestamp_ms)),
    #("event_type", json.string(entry.event_type)),
    #("track_uri", json.nullable(entry.track_uri, json.string)),
    #("track_name", json.nullable(entry.track_name, json.string)),
    #("artist_name", json.nullable(entry.artist_name, json.string)),
    #("album_name", json.nullable(entry.album_name, json.string)),
    #("track_duration_ms", json.nullable(entry.track_duration_ms, json.int)),
    #("position_ms", json.nullable(entry.position_ms, json.int)),
    #("volume_level", json.nullable(entry.volume_level, json.int)),
    #("queue_length", json.nullable(entry.queue_length, json.int)),
    #("user_id", json.nullable(entry.user_id, json.int)),
  ])
}

// ============================================================================
// AFFINITY ENDPOINT
// ============================================================================

/// Get user track and artist affinities sorted by score
pub fn get_user_affinities(
  state: AppState,
  auth_header: String,
  limit: Int,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              let track_affinities =
                queries.get_user_track_affinities(state.db, user_id, limit)
                |> result.unwrap([])
              let artist_affinities =
                queries.get_user_artist_affinities(state.db, user_id, limit)
                |> result.unwrap([])

              json.object([
                #(
                  "track_affinities",
                  json.array(track_affinities, encode_track_affinity),
                ),
                #(
                  "artist_affinities",
                  json.array(artist_affinities, encode_artist_affinity),
                ),
              ])
              |> json.to_string
              |> respond_json(200)
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) -> {
          error_response("Invalid or expired token: " <> string.inspect(e), 401)
        }
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

fn encode_track_affinity(a: queries.UserTrackAffinity) -> json.Json {
  json.object([
    #("track_uri", json.string(a.track_uri)),
    #("play_count", json.int(a.play_count)),
    #("total_listen_ms", json.int(a.total_listen_ms)),
    #("avg_listen_pct", json.float(a.avg_listen_pct)),
    #("queue_add_count", json.int(a.queue_add_count)),
    #("queue_move_closer", json.int(a.queue_move_closer)),
    #("skip_count", json.int(a.skip_count)),
    #("early_skip_count", json.int(a.early_skip_count)),
    #("queue_remove_count", json.int(a.queue_remove_count)),
    #("playlist_add_count", json.int(a.playlist_add_count)),
    #("affinity_score", json.float(a.affinity_score)),
    #("last_interaction_ms", json.int(a.last_interaction_ms)),
  ])
}

fn encode_artist_affinity(a: queries.UserArtistAffinity) -> json.Json {
  json.object([
    #("artist_name", json.string(a.artist_name)),
    #("play_count", json.int(a.play_count)),
    #("skip_count", json.int(a.skip_count)),
    #("total_listen_ms", json.int(a.total_listen_ms)),
    #("affinity_score", json.float(a.affinity_score)),
  ])
}

fn create_jwt_token(
  user: queries.User,
  secret: String,
) -> Result(String, String) {
  let exp =
    timestamp.system_time()
    |> timestamp.to_unix_seconds()
    |> float.add(86_400.0)
    |> float.round

  let token =
    jwt.new()
    |> jwt.set_subject(string.inspect(user.id))
    |> jwt.set_expiration(exp)
    |> jwt.set_issued_at(user.created_at)
    |> jwt.to_signed_string(jwt.HS256, secret)

  Ok(token)
}

pub fn extract_token(auth_header: String) -> Result(String, String) {
  case string.starts_with(auth_header, "Bearer ") {
    True -> {
      string.drop_start(auth_header, 7)
      |> Ok
    }
    False -> Error("Invalid authorization header format")
  }
}

pub fn verify_jwt_token(
  token: String,
  secret: String,
) -> Result(Jwt(Verified), jwt.JwtDecodeError) {
  jwt.from_signed_string(token, secret)
}

pub fn get_user_id_from_jwt(jwt_data: Jwt(Verified)) -> Result(Int, String) {
  use subject <- result.try(
    jwt.get_subject(jwt_data)
    |> result.replace_error("No subject in JWT"),
  )

  case int.parse(subject) {
    Ok(user_id) -> Ok(user_id)
    Error(_) -> Error("Invalid user ID in JWT subject")
  }
}

pub fn respond_json(body: String, status: Int) -> Response(ResponseData) {
  response.new(status)
  |> response.prepend_header("content-type", "application/json")
  |> response.set_body(mist.Bytes(bytes_tree.from_string(body)))
}

pub fn error_response(message: String, status: Int) -> Response(ResponseData) {
  json.object([#("error", json.string(message))])
  |> json.to_string
  |> respond_json(status)
}

// ============================================================================
// PLAYLIST ENDPOINTS
// ============================================================================

fn now_ms() -> Int {
  timestamp.system_time()
  |> timestamp.to_unix_seconds()
  |> float.multiply(1000.0)
  |> float.round
}

fn encode_playlist(p: queries.Playlist) -> json.Json {
  json.object([
    #("id", json.int(p.id)),
    #("user_id", json.int(p.user_id)),
    #("name", json.string(p.name)),
    #("description", json.nullable(p.description, json.string)),
    #("created_at", json.int(p.created_at)),
    #("updated_at", json.int(p.updated_at)),
  ])
}

fn encode_playlist_track(t: queries.PlaylistTrack) -> json.Json {
  json.object([
    #("playlist_id", json.int(t.playlist_id)),
    #("track_uri", json.string(t.track_uri)),
    #("position", json.int(t.position)),
  ])
}

/// Verify playlist ownership: returns Ok(playlist) if user_id matches
fn verify_ownership(
  db: sqlight.Connection,
  playlist_id: Int,
  user_id: Int,
) -> Result(queries.Playlist, Response(ResponseData)) {
  case queries.get_playlist_by_id(db, playlist_id) {
    Ok([playlist, ..]) -> {
      case playlist.user_id == user_id {
        True -> Ok(playlist)
        False -> Error(error_response("Forbidden", 403))
      }
    }
    Ok([]) -> Error(error_response("Playlist not found", 404))
    Error(_) -> Error(error_response("Database error", 500))
  }
}

/// List all playlists for the current user
pub fn list_playlists(
  state: AppState,
  auth_header: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case queries.get_user_playlists(state.db, user_id) {
                Ok(playlists) -> {
                  json.array(playlists, encode_playlist)
                  |> json.to_string
                  |> respond_json(200)
                }
                Error(_) -> error_response("Database error", 500)
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Get playlist IDs that contain a given track (for the current user)
pub fn get_playlists_containing_track(
  state: AppState,
  auth_header: String,
  track_uri: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case
                queries.get_playlists_containing_track(
                  state.db,
                  user_id,
                  track_uri,
                )
              {
                Ok(ids) -> {
                  json.array(ids, json.int)
                  |> json.to_string
                  |> respond_json(200)
                }
                Error(_) -> error_response("Database error", 500)
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Create a new playlist
pub fn create_playlist(
  state: AppState,
  auth_header: String,
  body: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case parse_playlist_body(body) {
                Ok(#(name, description)) -> {
                  case
                    queries.create_playlist(
                      state.db,
                      user_id,
                      name,
                      description,
                      now_ms(),
                    )
                  {
                    Ok([playlist, ..]) ->
                      encode_playlist(playlist)
                      |> json.to_string
                      |> respond_json(201)
                    _ -> error_response("Failed to create playlist", 500)
                  }
                }
                Error(e) -> error_response("Invalid request: " <> e, 400)
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Get a single playlist with its tracks
pub fn get_playlist(
  state: AppState,
  auth_header: String,
  playlist_id: Int,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case verify_ownership(state.db, playlist_id, user_id) {
                Ok(playlist) -> {
                  let tracks =
                    queries.get_playlist_tracks(state.db, playlist_id)
                    |> result.unwrap([])
                  json.object([
                    #("playlist", encode_playlist(playlist)),
                    #("tracks", json.array(tracks, encode_playlist_track)),
                  ])
                  |> json.to_string
                  |> respond_json(200)
                }
                Error(resp) -> resp
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Update a playlist's name and description
pub fn update_playlist(
  state: AppState,
  auth_header: String,
  playlist_id: Int,
  body: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case verify_ownership(state.db, playlist_id, user_id) {
                Ok(_) -> {
                  case parse_playlist_body(body) {
                    Ok(#(name, description)) -> {
                      case
                        queries.update_playlist(
                          state.db,
                          playlist_id,
                          name,
                          description,
                          now_ms(),
                        )
                      {
                        Ok([playlist, ..]) ->
                          encode_playlist(playlist)
                          |> json.to_string
                          |> respond_json(200)
                        _ -> error_response("Failed to update playlist", 500)
                      }
                    }
                    Error(e) -> error_response("Invalid request: " <> e, 400)
                  }
                }
                Error(resp) -> resp
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Delete a playlist
pub fn delete_playlist(
  state: AppState,
  auth_header: String,
  playlist_id: Int,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case verify_ownership(state.db, playlist_id, user_id) {
                Ok(_) -> {
                  case queries.delete_playlist(state.db, playlist_id) {
                    Ok(_) ->
                      json.object([#("ok", json.bool(True))])
                      |> json.to_string
                      |> respond_json(200)
                    Error(_) ->
                      error_response("Failed to delete playlist", 500)
                  }
                }
                Error(resp) -> resp
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Add a track to a playlist (also updates affinity)
pub fn add_playlist_track(
  state: AppState,
  auth_header: String,
  playlist_id: Int,
  body: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case verify_ownership(state.db, playlist_id, user_id) {
                Ok(_) -> {
                  case parse_track_uri_body(body) {
                    Ok(track_uri) -> {
                      case
                        queries.add_track_to_playlist(
                          state.db,
                          playlist_id,
                          track_uri,
                        )
                      {
                        Ok(_) -> {
                          // Update affinity score
                          let _ =
                            queries.update_track_affinity_playlist(
                              state.db,
                              user_id,
                              track_uri,
                              "add",
                              now_ms(),
                            )
                          // Touch updated_at on the playlist
                          let _ =
                            queries.update_playlist(
                              state.db,
                              playlist_id,
                              // Re-fetch to keep current name
                              {
                                case
                                  queries.get_playlist_by_id(
                                    state.db,
                                    playlist_id,
                                  )
                                {
                                  Ok([p, ..]) -> p.name
                                  _ -> ""
                                }
                              },
                              {
                                case
                                  queries.get_playlist_by_id(
                                    state.db,
                                    playlist_id,
                                  )
                                {
                                  Ok([p, ..]) -> p.description
                                  _ -> None
                                }
                              },
                              now_ms(),
                            )
                          json.object([#("ok", json.bool(True))])
                          |> json.to_string
                          |> respond_json(200)
                        }
                        Error(_) ->
                          error_response(
                            "Failed to add track (may already exist)",
                            409,
                          )
                      }
                    }
                    Error(e) -> error_response("Invalid request: " <> e, 400)
                  }
                }
                Error(resp) -> resp
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Remove a track from a playlist (also updates affinity)
pub fn remove_playlist_track(
  state: AppState,
  auth_header: String,
  playlist_id: Int,
  body: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case verify_ownership(state.db, playlist_id, user_id) {
                Ok(_) -> {
                  case parse_track_uri_body(body) {
                    Ok(track_uri) -> {
                      case
                        queries.remove_track_from_playlist(
                          state.db,
                          playlist_id,
                          track_uri,
                        )
                      {
                        Ok(_) -> {
                          json.object([#("ok", json.bool(True))])
                          |> json.to_string
                          |> respond_json(200)
                        }
                        Error(_) ->
                          error_response("Failed to remove track", 500)
                      }
                    }
                    Error(e) -> error_response("Invalid request: " <> e, 400)
                  }
                }
                Error(resp) -> resp
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

/// Reorder a track within a playlist
pub fn reorder_playlist_track(
  state: AppState,
  auth_header: String,
  playlist_id: Int,
  body: String,
) -> Response(ResponseData) {
  case extract_token(auth_header) {
    Ok(token) -> {
      case verify_jwt_token(token, state.jwt_secret) {
        Ok(jwt_data) -> {
          case get_user_id_from_jwt(jwt_data) {
            Ok(user_id) -> {
              case verify_ownership(state.db, playlist_id, user_id) {
                Ok(_) -> {
                  case parse_reorder_body(body) {
                    Ok(#(track_uri, new_position)) -> {
                      case
                        queries.reorder_playlist_track(
                          state.db,
                          playlist_id,
                          track_uri,
                          new_position,
                        )
                      {
                        Ok(_) ->
                          json.object([#("ok", json.bool(True))])
                          |> json.to_string
                          |> respond_json(200)
                        Error(_) ->
                          error_response("Failed to reorder track", 500)
                      }
                    }
                    Error(e) -> error_response("Invalid request: " <> e, 400)
                  }
                }
                Error(resp) -> resp
              }
            }
            Error(e) -> error_response("Invalid token: " <> e, 401)
          }
        }
        Error(e) ->
          error_response(
            "Invalid or expired token: " <> string.inspect(e),
            401,
          )
      }
    }
    Error(e) -> error_response(e, 401)
  }
}

// Playlist body parsers

fn parse_playlist_body(
  body: String,
) -> Result(#(String, Option(String)), String) {
  let decoder = {
    use name <- decode.field("name", decode.string)
    use description <- decode.optional_field(
      "description",
      None,
      decode.optional(decode.string),
    )
    decode.success(#(name, description))
  }

  json.parse(body, decoder)
  |> result.map_error(fn(_) { "Invalid JSON: expected {name, description?}" })
}

fn parse_track_uri_body(body: String) -> Result(String, String) {
  let decoder = {
    use track_uri <- decode.field("track_uri", decode.string)
    decode.success(track_uri)
  }

  json.parse(body, decoder)
  |> result.map_error(fn(_) { "Invalid JSON: expected {track_uri}" })
}

fn parse_reorder_body(body: String) -> Result(#(String, Int), String) {
  let decoder = {
    use track_uri <- decode.field("track_uri", decode.string)
    use new_position <- decode.field("new_position", decode.int)
    decode.success(#(track_uri, new_position))
  }

  json.parse(body, decoder)
  |> result.map_error(fn(_) {
    "Invalid JSON: expected {track_uri, new_position}"
  })
}
