import db/connection
import envoy
import event_bus
import gleam/bit_array
import gleam/bytes_tree
import gleam/erlang/process
import gleam/http
import gleam/http/request.{type Request}
import gleam/http/response.{type Response}
import gleam/int
import gleam/list
import gleam/option
import gleam/result
import gleam/string
import gleam/uri
import handlers/http as http_handlers
import logging
import mist
import playback_state
import websocket/handler as ws_handler
import websocket/mopidy_client

pub fn main() {
  logging.configure()
  logging.set_level(logging.Debug)

  let port = 3001

  let db_path =
    envoy.get("DB_PATH")
    |> result.unwrap("./data/musakone.db")

  let jwt_secret =
    envoy.get("JWT_SECRET")
    |> result.unwrap("change-this-secret-in-production")

  let mopidy_url =
    envoy.get("MOPIDY_URL")
    |> result.unwrap("ws://localhost:6680/mopidy/ws")

  logging.log(logging.Info, "╔══════════════════════════════════════╗")
  logging.log(logging.Info, "║   MusakoneV3 Backend Starting...     ║")
  logging.log(logging.Info, "╚══════════════════════════════════════╝")
  logging.log(logging.Info, "")
  logging.log(logging.Info, "Configuration:")
  logging.log(logging.Info, "  • Port: " <> int.to_string(port))
  logging.log(logging.Info, "  • Database: " <> db_path)
  logging.log(logging.Info, "  • Mopidy URL: " <> mopidy_url)
  logging.log(logging.Info, "")

  use db <- result.try(connection.initialize(db_path))

  logging.log(logging.Info, "✓ Database connected and initialized")

  // Start the event bus (pub/sub for decoupling components)
  use bus <- result.try(
    event_bus.start()
    |> result.map_error(string.inspect),
  )

  logging.log(logging.Info, "✓ Event bus started")

  // Initialize Mopidy WebSocket connection (connects to bus automatically)
  use _mopidy <- result.try(
    mopidy_client.start(mopidy_url, bus)
    |> result.map_error(string.inspect),
  )

  // Start playback state actor (global state tracking with attribution)
  use ps_actor <- result.try(
    playback_state.start(db, bus)
    |> result.map_error(string.inspect),
  )

  logging.log(logging.Info, "")

  // Create app state
  let state =
    http_handlers.AppState(
      db: db,
      jwt_secret: jwt_secret,
      event_bus: bus,
      playback_state: ps_actor,
    )

  // Start Mist server
  let assert Ok(_) =
    fn(req: Request(mist.Connection)) -> Response(mist.ResponseData) {
      handle_request(req, state)
    }
    |> mist.new
    |> mist.bind("0.0.0.0")
    |> mist.port(port)
    |> mist.start

  logging.log(logging.Info, "✓ Server started successfully!")

  Ok(process.sleep_forever())
}

fn handle_request(
  req: Request(mist.Connection),
  state: http_handlers.AppState,
) -> Response(mist.ResponseData) {
  let path = request.path_segments(req)
  let method = req.method

  logging.log(
    logging.Debug,
    string.inspect(method) <> " " <> string.join(path, "/"),
  )

  // Add CORS headers
  let with_cors = fn(resp: Response(mist.ResponseData)) {
    resp
    |> response.set_header("access-control-allow-origin", "*")
    |> response.set_header(
      "access-control-allow-methods",
      "GET, POST, PUT, DELETE, OPTIONS",
    )
    |> response.set_header(
      "access-control-allow-headers",
      "content-type, authorization",
    )
  }

  case method, path {
    // OPTIONS requests (CORS preflight)
    http.Options, _ -> {
      response.new(204)
      |> response.set_body(mist.Bytes(bytes_tree.new()))
      |> with_cors
    }

    // Health check
    http.Get, ["api", "health"] ->
      http_handlers.health_check()
      |> with_cors

    // Auth endpoints
    http.Post, ["api", "auth", "login"] -> {
      case mist.read_body(req, max_body_limit: 1024 * 1024) {
        Ok(body_req) -> {
          case bit_array.to_string(body_req.body) {
            Ok(body) -> http_handlers.login(state, body) |> with_cors
            Error(_) ->
              error_response("Invalid UTF-8 in request body", 400) |> with_cors
          }
        }
        Error(_) ->
          error_response("Failed to read request body", 400) |> with_cors
      }
    }

    http.Post, ["api", "auth", "register"] -> {
      case mist.read_body(req, max_body_limit: 1024 * 1024) {
        Ok(body_req) -> {
          case bit_array.to_string(body_req.body) {
            Ok(body) -> http_handlers.register(state, body) |> with_cors
            Error(_) ->
              error_response("Invalid UTF-8 in request body", 400) |> with_cors
          }
        }
        Error(_) ->
          error_response("Failed to read request body", 400) |> with_cors
      }
    }

    http.Get, ["api", "auth", "me"] -> {
      case get_auth_header(req) {
        Ok(auth_header) -> http_handlers.me(state, auth_header) |> with_cors
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    // Analytics endpoints
    http.Get, ["api", "analytics", "events"] -> {
      case get_auth_header(req) {
        Ok(auth_header) ->
          http_handlers.get_events(state, auth_header) |> with_cors
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    http.Get, ["api", "analytics", "stats"] -> {
      case get_auth_header(req) {
        Ok(auth_header) ->
          http_handlers.get_stats(state, auth_header) |> with_cors
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    // Admin dashboard (all users data)
    http.Get, ["api", "analytics", "admin"] -> {
      case get_auth_header(req) {
        Ok(auth_header) ->
          http_handlers.get_admin_dashboard(state, auth_header) |> with_cors
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    // User affinity scores (track + artist)
    http.Get, ["api", "analytics", "affinity"] -> {
      case get_auth_header(req) {
        Ok(auth_header) -> {
          let query_params =
            req.query
            |> option.unwrap("")
            |> uri.parse_query
            |> result.unwrap([])

          let limit =
            list.key_find(query_params, "limit")
            |> result.try(int.parse)
            |> result.unwrap(20)

          http_handlers.get_user_affinities(state, auth_header, limit)
          |> with_cors
        }
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    // Playback state: current state (no auth — public "now playing")
    http.Get, ["api", "playback", "state"] -> {
      http_handlers.get_playback_state(state)
      |> with_cors
    }

    // Playback state: history (auth required)
    http.Get, ["api", "playback", "history"] -> {
      case get_auth_header(req) {
        Ok(auth_header) -> {
          let query_params =
            req.query
            |> option.unwrap("")
            |> uri.parse_query
            |> result.unwrap([])

          let limit =
            list.key_find(query_params, "limit")
            |> result.try(int.parse)
            |> result.unwrap(50)

          http_handlers.get_playback_history(state, auth_header, limit)
          |> with_cors
        }
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    // ML data export (paginated)
    http.Get, ["api", "analytics", "export"] -> {
      case get_auth_header(req) {
        Ok(auth_header) -> {
          let query_params =
            req.query
            |> option.unwrap("")
            |> uri.parse_query
            |> result.unwrap([])

          let offset =
            list.key_find(query_params, "offset")
            |> result.try(int.parse)
            |> result.unwrap(0)

          let limit =
            list.key_find(query_params, "limit")
            |> result.try(int.parse)
            |> result.unwrap(1000)

          http_handlers.export_ml_data(state, auth_header, offset, limit)
          |> with_cors
        }
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    // Playlist endpoints
    http.Get, ["api", "playlists"] -> {
      case get_auth_header(req) {
        Ok(auth_header) ->
          http_handlers.list_playlists(state, auth_header) |> with_cors
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    http.Post, ["api", "playlists"] -> {
      case get_auth_header(req) {
        Ok(auth_header) -> {
          case mist.read_body(req, max_body_limit: 1024 * 1024) {
            Ok(body_req) -> {
              case bit_array.to_string(body_req.body) {
                Ok(body) ->
                  http_handlers.create_playlist(state, auth_header, body)
                  |> with_cors
                Error(_) ->
                  error_response("Invalid UTF-8 in request body", 400)
                  |> with_cors
              }
            }
            Error(_) ->
              error_response("Failed to read request body", 400) |> with_cors
          }
        }
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    http.Get, ["api", "playlists", "containing"] -> {
      case get_auth_header(req) {
        Ok(auth_header) -> {
          let query_params =
            req.query
            |> option.unwrap("")
            |> uri.parse_query
            |> result.unwrap([])

          case list.key_find(query_params, "track_uri") {
            Ok(track_uri) ->
              http_handlers.get_playlists_containing_track(
                state,
                auth_header,
                track_uri,
              )
              |> with_cors
            Error(_) ->
              error_response("Missing track_uri query parameter", 400)
              |> with_cors
          }
        }
        Error(e) -> error_response(e, 401) |> with_cors
      }
    }

    http.Get, ["api", "playlists", id_str] -> {
      case int.parse(id_str) {
        Ok(playlist_id) -> {
          case get_auth_header(req) {
            Ok(auth_header) ->
              http_handlers.get_playlist(state, auth_header, playlist_id)
              |> with_cors
            Error(e) -> error_response(e, 401) |> with_cors
          }
        }
        Error(_) -> error_response("Invalid playlist ID", 400) |> with_cors
      }
    }

    http.Put, ["api", "playlists", id_str] -> {
      case int.parse(id_str) {
        Ok(playlist_id) -> {
          case get_auth_header(req) {
            Ok(auth_header) -> {
              case mist.read_body(req, max_body_limit: 1024 * 1024) {
                Ok(body_req) -> {
                  case bit_array.to_string(body_req.body) {
                    Ok(body) ->
                      http_handlers.update_playlist(
                        state,
                        auth_header,
                        playlist_id,
                        body,
                      )
                      |> with_cors
                    Error(_) ->
                      error_response("Invalid UTF-8 in request body", 400)
                      |> with_cors
                  }
                }
                Error(_) ->
                  error_response("Failed to read request body", 400)
                  |> with_cors
              }
            }
            Error(e) -> error_response(e, 401) |> with_cors
          }
        }
        Error(_) -> error_response("Invalid playlist ID", 400) |> with_cors
      }
    }

    http.Delete, ["api", "playlists", id_str] -> {
      case int.parse(id_str) {
        Ok(playlist_id) -> {
          case get_auth_header(req) {
            Ok(auth_header) ->
              http_handlers.delete_playlist(state, auth_header, playlist_id)
              |> with_cors
            Error(e) -> error_response(e, 401) |> with_cors
          }
        }
        Error(_) -> error_response("Invalid playlist ID", 400) |> with_cors
      }
    }

    http.Post, ["api", "playlists", id_str, "tracks"] -> {
      case int.parse(id_str) {
        Ok(playlist_id) -> {
          case get_auth_header(req) {
            Ok(auth_header) -> {
              case mist.read_body(req, max_body_limit: 1024 * 1024) {
                Ok(body_req) -> {
                  case bit_array.to_string(body_req.body) {
                    Ok(body) ->
                      http_handlers.add_playlist_track(
                        state,
                        auth_header,
                        playlist_id,
                        body,
                      )
                      |> with_cors
                    Error(_) ->
                      error_response("Invalid UTF-8 in request body", 400)
                      |> with_cors
                  }
                }
                Error(_) ->
                  error_response("Failed to read request body", 400)
                  |> with_cors
              }
            }
            Error(e) -> error_response(e, 401) |> with_cors
          }
        }
        Error(_) -> error_response("Invalid playlist ID", 400) |> with_cors
      }
    }

    http.Post, ["api", "playlists", id_str, "tracks", "remove"] -> {
      case int.parse(id_str) {
        Ok(playlist_id) -> {
          case get_auth_header(req) {
            Ok(auth_header) -> {
              case mist.read_body(req, max_body_limit: 1024 * 1024) {
                Ok(body_req) -> {
                  case bit_array.to_string(body_req.body) {
                    Ok(body) ->
                      http_handlers.remove_playlist_track(
                        state,
                        auth_header,
                        playlist_id,
                        body,
                      )
                      |> with_cors
                    Error(_) ->
                      error_response("Invalid UTF-8 in request body", 400)
                      |> with_cors
                  }
                }
                Error(_) ->
                  error_response("Failed to read request body", 400)
                  |> with_cors
              }
            }
            Error(e) -> error_response(e, 401) |> with_cors
          }
        }
        Error(_) -> error_response("Invalid playlist ID", 400) |> with_cors
      }
    }

    http.Put, ["api", "playlists", id_str, "tracks", "reorder"] -> {
      case int.parse(id_str) {
        Ok(playlist_id) -> {
          case get_auth_header(req) {
            Ok(auth_header) -> {
              case mist.read_body(req, max_body_limit: 1024 * 1024) {
                Ok(body_req) -> {
                  case bit_array.to_string(body_req.body) {
                    Ok(body) ->
                      http_handlers.reorder_playlist_track(
                        state,
                        auth_header,
                        playlist_id,
                        body,
                      )
                      |> with_cors
                    Error(_) ->
                      error_response("Invalid UTF-8 in request body", 400)
                      |> with_cors
                  }
                }
                Error(_) ->
                  error_response("Failed to read request body", 400)
                  |> with_cors
              }
            }
            Error(e) -> error_response(e, 401) |> with_cors
          }
        }
        Error(_) -> error_response("Invalid playlist ID", 400) |> with_cors
      }
    }

    // WebSocket endpoint - now uses event bus
    http.Get, ["ws"] -> {
      ws_handler.handle_websocket(
        req,
        state.event_bus,
        state.db,
        state.jwt_secret,
        state.playback_state,
      )
      |> with_cors
    }

    // 404 Not Found
    _, _ -> {
      response.new(404)
      |> response.set_body(
        mist.Bytes(bytes_tree.from_string("{\"error\":\"Not found\"}")),
      )
      |> response.set_header("content-type", "application/json")
      |> with_cors
    }
  }
}

fn get_auth_header(req: Request(mist.Connection)) -> Result(String, String) {
  request.get_header(req, "authorization")
  |> result.replace_error("Missing authorization header")
}

fn error_response(message: String, status: Int) -> Response(mist.ResponseData) {
  response.new(status)
  |> response.set_header("content-type", "application/json")
  |> response.set_body(
    mist.Bytes(bytes_tree.from_string("{\"error\":\"" <> message <> "\"}")),
  )
}
