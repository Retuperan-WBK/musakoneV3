/// WebSocket handler for browser client connections
/// Subscribes to event bus for Mopidy events, sends commands through event bus
/// Tracks all user actions to the database for ML training
import auth/jwt
import db/queries
import db/tracker
import event_bus.{type BusMessage, type MopidyEvent}
import gleam/erlang/process.{type Subject}
import gleam/float
import gleam/http/request.{type Request}
import gleam/http/response
import gleam/int
import gleam/option.{type Option, None, Some}
import gleam/string
import gleam/time/timestamp
import gleam/uri
import logging
import mist.{type ResponseData, type WebsocketConnection, type WebsocketMessage}
import playback_state.{type PlaybackStateMessage}
import sqlight

/// State for each WebSocket connection
pub type WsState {
  WsState(
    event_bus: Subject(BusMessage),
    event_subject: Subject(MopidyEvent),
    db: sqlight.Connection,
    user_id: Option(Int),
    context: tracker.PlaybackContext,
    playback_state: Subject(PlaybackStateMessage),
  )
}

/// WebSocket handler for client connections
pub fn handle_websocket(
  req: Request(mist.Connection),
  event_bus: Subject(BusMessage),
  db: sqlight.Connection,
  jwt_secret: String,
  ps_actor: Subject(PlaybackStateMessage),
) -> response.Response(ResponseData) {
  let user_id = extract_user_id_from_request(req, jwt_secret)

  mist.websocket(
    request: req,
    on_init: fn(_conn) { init_websocket(event_bus, db, user_id, ps_actor) },
    on_close: fn(state) {
      logging.log(
        logging.Info,
        "WebSocket connection closed, unsubscribing from event bus",
      )
      // Unsubscribe from the event bus
      event_bus.unsubscribe(state.event_bus, state.event_subject)
      Nil
    },
    handler: handle_message,
  )
}

/// Extract user_id from the JWT token passed as ?token= query parameter
fn extract_user_id_from_request(
  req: Request(mist.Connection),
  jwt_secret: String,
) -> Option(Int) {
  let query_string = req.query |> option.unwrap("")

  case uri.parse_query(query_string) {
    Ok(params) -> {
      case find_param(params, "token") {
        Some(token) -> {
          case jwt.from_signed_string(token, jwt_secret) {
            Ok(jwt_data) -> {
              case jwt.get_subject(jwt_data) {
                Ok(subject) -> {
                  case int.parse(subject) {
                    Ok(uid) -> {
                      logging.log(
                        logging.Debug,
                        "WS authenticated: user_id=" <> int.to_string(uid),
                      )
                      Some(uid)
                    }
                    Error(_) -> None
                  }
                }
                Error(_) -> None
              }
            }
            Error(_) -> None
          }
        }
        None -> None
      }
    }
    Error(_) -> None
  }
}

fn find_param(params: List(#(String, String)), key: String) -> Option(String) {
  case params {
    [] -> None
    [#(k, v), ..rest] ->
      case k == key {
        True -> Some(v)
        False -> find_param(rest, key)
      }
  }
}

/// Initialize WebSocket connection and subscribe to event bus
fn init_websocket(
  bus: Subject(BusMessage),
  db: sqlight.Connection,
  user_id: Option(Int),
  ps_actor: Subject(PlaybackStateMessage),
) -> #(WsState, option.Option(process.Selector(MopidyEvent))) {
  logging.log(logging.Info, "New WebSocket connection established")

  // Create a subject to receive events from the bus
  let event_subject = process.new_subject()

  // Subscribe to Mopidy events
  event_bus.subscribe(bus, event_subject)

  let state =
    WsState(
      event_bus: bus,
      event_subject: event_subject,
      db: db,
      user_id: user_id,
      context: tracker.empty_context(),
      playback_state: ps_actor,
    )

  // Set up selector to receive events
  let selector =
    process.new_selector()
    |> process.select(event_subject)

  #(state, Some(selector))
}

/// Handle incoming WebSocket messages
fn handle_message(
  state: WsState,
  message: WebsocketMessage(MopidyEvent),
  conn: WebsocketConnection,
) -> mist.Next(WsState, MopidyEvent) {
  case message {
    // Text message from browser - send command to Mopidy via event bus
    mist.Text(text) -> {
      logging.log(logging.Debug, "Browser -> Backend: " <> text)
      // Track the user command with playback context (returns updated ctx
      // with search query state for conversion tracking)
      let ctx_after_track =
        tracker.track_command(state.db, state.user_id, state.context, text)
      // Send attribution hint to playback state actor (authenticated users only)
      case state.user_id {
        Some(uid) -> {
          case tracker.extract_method(text) {
            Ok(method) -> {
              let now =
                timestamp.system_time()
                |> timestamp.to_unix_seconds()
                |> float.multiply(1000.0)
                |> float.round
              process.send(
                state.playback_state,
                playback_state.AttributeCommand(uid, method, now),
              )
            }
            Error(_) -> Nil
          }
        }
        None -> Nil
      }
      // Record outgoing request id→method for response correlation
      let new_ctx = tracker.record_request(ctx_after_track, text)
      event_bus.send_command(state.event_bus, event_bus.SendMessage(text))
      mist.continue(WsState(..state, context: new_ctx))
    }

    // Binary message from browser - log and ignore
    mist.Binary(data) -> {
      logging.log(
        logging.Debug,
        "Received binary message from browser: " <> string.inspect(data),
      )
      mist.continue(state)
    }

    // Event from event bus - forward to browser, update playback context
    mist.Custom(event) -> {
      let new_state = case event {
        event_bus.MessageReceived(data) -> {
          // Every client shares one Mopidy connection, so replies reach every handler.
          // Forward events to everyone, but a reply only to the connection that asked.
          case tracker.owns_response(state.context, data) {
            True -> {
              logging.log(logging.Debug, "Backend -> Browser: " <> data)
              case mist.send_text_frame(conn, data) {
                Ok(_) -> Nil
                Error(err) -> {
                  logging.log(
                    logging.Error,
                    "Failed to send to browser: " <> string.inspect(err),
                  )
                }
              }
            }
            False -> Nil
          }
          case state.user_id, tracker.search_result_count(state.context, data) {
            Some(uid), Some(n) -> {
              let _ = queries.set_last_search_result_count(state.db, uid, n)
              Nil
            }
            _, _ -> Nil
          }
          // Update playback context from Mopidy events
          let new_ctx = tracker.update_context(state.context, data)
          WsState(..state, context: new_ctx)
        }

        event_bus.Error(error) -> {
          logging.log(logging.Error, "Mopidy error: " <> error)
          let error_json =
            "{\"error\": \"" <> escape_json_string(error) <> "\"}"
          let _ = mist.send_text_frame(conn, error_json)
          state
        }

        event_bus.Connected -> {
          logging.log(logging.Info, "Mopidy connection established")
          let _ =
            mist.send_text_frame(conn, "{\"event\": \"mopidy_connected\"}")
          state
        }

        event_bus.Disconnected -> {
          logging.log(logging.Warning, "Mopidy connection lost")
          let _ =
            mist.send_text_frame(conn, "{\"event\": \"mopidy_disconnected\"}")
          state
        }
      }

      mist.continue(new_state)
    }

    // Connection closed
    mist.Closed | mist.Shutdown -> {
      logging.log(logging.Info, "WebSocket closed")
      mist.stop()
    }
  }
}

/// Escape special characters for JSON string
fn escape_json_string(s: String) -> String {
  s
  |> string.replace("\\", "\\\\")
  |> string.replace("\"", "\\\"")
  |> string.replace("\n", "\\n")
  |> string.replace("\r", "\\r")
  |> string.replace("\t", "\\t")
}
