/// Global playback state actor — the server-side ground truth of what
/// Mopidy is actually doing. Subscribes to the event bus, maintains a
/// PlaybackContext, and logs every state transition to the database.
///
/// Key differences from tracker.gleam:
///   - tracker.gleam logs USER COMMANDS (browser → Mopidy)
///   - This actor logs MOPIDY STATE CHANGES (events from Mopidy)
///   - This actor captures ALL changes including external sources
///   - Attribution links state changes to the user who caused them
///
import db/queries
import db/tracker
import event_bus.{type BusMessage, type MopidyEvent}
import gleam/dict.{type Dict}
import gleam/erlang/process.{type Subject}
import gleam/float
import gleam/int
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/otp/actor
import gleam/result
import gleam/string
import gleam/time/timestamp
import logging
import sqlight

// ─── Public types ──────────────────────────────────────────────────

/// Messages the playback state actor can receive
pub type PlaybackStateMessage {
  /// Mopidy event from the event bus (via bridge process)
  MopidyEventReceived(event: MopidyEvent)
  /// Attribution hint from a WebSocket handler: "user X just sent method Y"
  AttributeCommand(user_id: Int, method: String, timestamp_ms: Int)
  /// Synchronous state query (from HTTP handlers)
  GetState(reply_to: Subject(PlaybackStateSnapshot))
}

/// Snapshot of the current playback state, returned by GetState
pub type PlaybackStateSnapshot {
  PlaybackStateSnapshot(
    playback_state: Option(String),
    track_uri: Option(String),
    track_name: Option(String),
    artist_name: Option(String),
    album_name: Option(String),
    track_duration_ms: Option(Int),
    position_ms: Option(Int),
    volume: Option(Int),
    queue_length: Int,
  )
}

/// Pending attribution entry: a user command waiting to be matched
type PendingAttribution {
  PendingAttribution(user_id: Int, method: String, timestamp_ms: Int)
}

/// Per-user listening session state (in-memory)
type SessionState {
  SessionState(session_id: Int, started_ms: Int, last_activity_ms: Int, track_count: Int)
}

/// Internal actor state
type State {
  State(
    db: sqlight.Connection,
    event_bus: Subject(BusMessage),
    event_subject: Subject(MopidyEvent),
    context: tracker.PlaybackContext,
    pending_attributions: List(PendingAttribution),
    last_volume_log_ms: Int,
    /// Reserved request ID counter for our own Mopidy queries
    next_request_id: Int,
    /// Active listening sessions per user (user_id -> session)
    active_sessions: Dict(Int, SessionState),
  )
}

// ─── Constants ─────────────────────────────────────────────────────

/// Minimum time between volume log entries (ms) to avoid slider-drag floods
const volume_debounce_ms = 1000

/// TTL for pending attributions (ms) — commands older than this won't match
const attribution_ttl_ms = 2000

/// Base request ID for this actor's own Mopidy queries (avoid browser conflicts)
const base_request_id = 900_001

/// Session boundary: 5 minutes of inactivity starts a new session
const session_gap_ms = 300_000

// ─── Public API ────────────────────────────────────────────────────

/// Start the playback state actor.
/// Subscribes to the event bus and begins tracking Mopidy state.
pub fn start(
  db: sqlight.Connection,
  event_bus: Subject(BusMessage),
) -> Result(Subject(PlaybackStateMessage), actor.StartError) {
  // Create a placeholder subject — will be replaced by the bridge's real one
  let placeholder_subject: Subject(MopidyEvent) = process.new_subject()

  let initial_state =
    State(
      db: db,
      event_bus: event_bus,
      event_subject: placeholder_subject,
      context: tracker.empty_context(),
      pending_attributions: [],
      last_volume_log_ms: 0,
      next_request_id: base_request_id,
      active_sessions: dict.new(),
    )

  let actor_result =
    actor.new(initial_state)
    |> actor.on_message(handle_message)
    |> actor.start

  case actor_result {
    Ok(started) -> {
      let subject = started.data

      // Create bridge process that owns its own subject for receiving events
      let event_subject = create_event_bridge(subject)

      // Subscribe the bridge's subject to the event bus
      event_bus.subscribe(event_bus, event_subject)

      logging.log(logging.Info, "✓ Playback state actor started")
      Ok(subject)
    }
    Error(err) -> Error(err)
  }
}

// ─── Event bridge ──────────────────────────────────────────────────

/// Create a bridge process that receives MopidyEvents and forwards them
/// as PlaybackStateMessage to the actor. The subject is created IN the
/// bridge process so it can receive on it (Gleam/Erlang requirement).
fn create_event_bridge(
  actor_subject: Subject(PlaybackStateMessage),
) -> Subject(MopidyEvent) {
  // Parent subject to receive the bridge's event subject back
  let parent_subject: Subject(Subject(MopidyEvent)) = process.new_subject()

  let _ =
    process.spawn(fn() {
      // Create the subject IN THIS PROCESS so we can receive on it
      let event_subject = process.new_subject()
      // Send it back to the parent
      process.send(parent_subject, event_subject)
      // Run the receive loop
      event_bridge_loop(event_subject, actor_subject)
    })

  // Wait for the child to send us its subject
  let assert Ok(event_subject) = process.receive(parent_subject, 5000)
  event_subject
}

fn event_bridge_loop(
  event_subject: Subject(MopidyEvent),
  actor_subject: Subject(PlaybackStateMessage),
) -> Nil {
  case process.receive(event_subject, 60_000) {
    Ok(event) -> {
      process.send(actor_subject, MopidyEventReceived(event))
      event_bridge_loop(event_subject, actor_subject)
    }
    Error(_) -> {
      // Timeout, continue waiting
      event_bridge_loop(event_subject, actor_subject)
    }
  }
}

// ─── Message handler ───────────────────────────────────────────────

fn handle_message(
  state: State,
  message: PlaybackStateMessage,
) -> actor.Next(State, PlaybackStateMessage) {
  case message {
    MopidyEventReceived(event) -> handle_mopidy_event(state, event)

    AttributeCommand(user_id, method, ts) -> {
      let attr = PendingAttribution(user_id: user_id, method: method, timestamp_ms: ts)
      let new_pending = [attr, ..state.pending_attributions]
      actor.continue(State(..state, pending_attributions: new_pending))
    }

    GetState(reply_to) -> {
      let snapshot = build_snapshot(state.context)
      process.send(reply_to, snapshot)
      actor.continue(state)
    }
  }
}

// ─── Mopidy event handling ─────────────────────────────────────────

fn handle_mopidy_event(
  state: State,
  event: MopidyEvent,
) -> actor.Next(State, PlaybackStateMessage) {
  case event {
    event_bus.MessageReceived(data) -> {
      // Update context (reuse tracker's parsing logic)
      let new_ctx = tracker.update_context(state.context, data)

      // Check if this is a state-changing event and log it
      let new_state = process_state_change(
        State(..state, context: new_ctx),
        data,
      )

      actor.continue(new_state)
    }

    event_bus.Connected -> {
      logging.log(logging.Info, "Playback state actor: Mopidy connected, querying initial state")
      actor.continue(query_initial_state(state))
    }

    event_bus.Disconnected -> {
      logging.log(logging.Warning, "Playback state actor: Mopidy disconnected")
      actor.continue(state)
    }

    event_bus.Error(_) -> actor.continue(state)
  }
}

/// Send initial state queries to Mopidy. Every request id is recorded in the
/// context so the responses are correlated back into it (tracker.update_context
/// ignores responses to requests it never saw).
fn query_initial_state(state: State) -> State {
  let methods = [
    "core.playback.get_state",
    "core.mixer.get_volume",
    "core.playback.get_current_tl_track",
    "core.playback.get_time_position",
    "core.tracklist.get_tl_tracks",
  ]

  let #(ctx, next_id) =
    list.fold(methods, #(state.context, state.next_request_id), fn(acc, method) {
      let #(ctx, id) = acc
      let msg = rpc_message(id, method)
      event_bus.send_command(state.event_bus, event_bus.SendMessage(msg))
      #(tracker.record_request(ctx, msg), id + 1)
    })
  State(..state, context: ctx, next_request_id: next_id)
}

fn rpc_message(id: Int, method: String) -> String {
  json.object([
    #("jsonrpc", json.string("2.0")),
    #("id", json.int(id)),
    #("method", json.string(method)),
  ])
  |> json.to_string
}

/// Remember artist name -> URI pairs from a track_playback_started event, so the
/// recommender can browse an artist's catalogue by name later.
fn index_artists(db: sqlight.Connection, raw: String) -> Nil {
  let artists_decoder =
    decode.at(
      ["tl_track", "track", "artists"],
      decode.list({
        use uri <- decode.optional_field("uri", "", decode.string)
        use name <- decode.optional_field("name", "", decode.string)
        decode.success(#(name, uri))
      }),
    )
  case json.parse(raw, artists_decoder) {
    Ok(artists) ->
      list.each(artists, fn(artist) {
        case artist.0 != "" && artist.1 != "" {
          True -> {
            let _ =
              queries.upsert_artist_index(db, artist.0, artist.1, now_ms())
            Nil
          }
          False -> Nil
        }
      })
    Error(_) -> Nil
  }
}

// ─── State change detection and logging ────────────────────────────

/// Check if a Mopidy message represents a loggable state change.
/// If so, log it to the playback_state_log table with attribution.
fn process_state_change(state: State, raw: String) -> State {
  let event_type = detect_event_type(raw)

  case event_type {
    None -> {
      // Not a state-changing event, just clean expired attributions
      prune_attributions(state)
    }
    Some(etype) -> {
      case etype == "track_started" {
        True -> index_artists(state.db, raw)
        False -> Nil
      }
      // Volume debouncing: skip if too recent
      case etype == "volume" {
        True -> {
          let now = now_ms()
          case now - state.last_volume_log_ms < volume_debounce_ms {
            True -> prune_attributions(state)
            False -> {
              let state = log_state_change(state, etype)
              State(..state, last_volume_log_ms: now)
            }
          }
        }
        False -> {
          // For tracklist_changed, re-query to capture updated queue
          case etype == "tracklist_changed" {
            True -> {
              let id = state.next_request_id
              let msg = rpc_message(id, "core.tracklist.get_tl_tracks")
              event_bus.send_command(state.event_bus, event_bus.SendMessage(msg))
              let state =
                State(
                  ..state,
                  context: tracker.record_request(state.context, msg),
                  next_request_id: id + 1,
                )
              log_state_change(state, etype)
            }
            False -> log_state_change(state, etype)
          }
        }
      }
    }
  }
}

/// Detect the event type from a raw Mopidy message.
/// Returns None for non-state-changing messages (responses, etc.)
fn detect_event_type(raw: String) -> Option(String) {
  // Check for Mopidy event field
  case extract_event_field(raw) {
    Ok("track_playback_started") -> Some("track_started")
    Ok("track_playback_ended") -> Some("track_ended")
    Ok("track_playback_paused") -> Some("paused")
    Ok("track_playback_resumed") -> Some("resumed")
    Ok("seeked") -> Some("seeked")
    Ok("volume_changed") -> Some("volume")
    Ok("tracklist_changed") -> Some("tracklist_changed")
    _ -> None
  }
}

fn extract_event_field(raw: String) -> Result(String, Nil) {
  // Quick check to avoid parsing non-events
  case string.contains(raw, "\"event\"") {
    False -> Error(Nil)
    True -> {
      case
        json.parse(raw, {
          use event <- decode.field("event", decode.string)
          decode.success(event)
        })
      {
        Ok(event) -> Ok(event)
        Error(_) -> Error(Nil)
      }
    }
  }
}

import gleam/dynamic/decode

/// Log a state change to the database with attribution.
/// Also handles: track features upsert, affinity updates on track_ended,
/// and listening session management.
fn log_state_change(state: State, event_type: String) -> State {
  let now = now_ms()
  let ctx = state.context
  let is_track_end = event_type == "track_ended"
  let is_track_start = event_type == "track_started"

  // Find attribution: match pending commands to this event. A track that starts
  // or ends on its own (natural playback) has no command behind it – credit the
  // person who put it in the queue, otherwise no listening data ever accrues.
  let #(explicit_user, remaining_attrs) =
    find_attribution(state.pending_attributions, event_type, now)
  let user_id = case explicit_user, is_track_start || is_track_end, ctx.track_uri {
    None, True, Some(uri) ->
      queries.find_queued_by(state.db, uri) |> result.unwrap(None)
    _, _, _ -> explicit_user
  }

  // How much of the track was heard (only meaningful when it just ended)
  let #(listen_ms, listen_pct) = case ctx.position_ms, ctx.track_duration_ms {
    Some(pos), Some(dur) ->
      case dur > 0 {
        True -> #(pos, int.to_float(pos) /. int.to_float(dur))
        False -> #(pos, 0.0)
      }
    Some(pos), None -> #(pos, 0.0)
    None, _ -> #(0, 0.0)
  }
  let is_skip = listen_pct <. 0.8
  let is_early_skip = listen_pct <. 0.25

  let queue_length = list.length(ctx.tracklist)

  case
    queries.log_playback_state_change(
      state.db,
      now,
      event_type,
      ctx.track_uri,
      ctx.track_name,
      ctx.artist_name,
      ctx.album_name,
      ctx.track_duration_ms,
      ctx.position_ms,
      ctx.volume,
      case queue_length > 0 {
        True -> Some(queue_length)
        False -> None
      },
      user_id,
    )
  {
    Ok(_) -> Nil
    Error(e) ->
      logging.log(
        logging.Error,
        "Failed to log playback state change: " <> string.inspect(e),
      )
  }

  let state = State(..state, pending_attributions: remaining_attrs)

  // Upsert track features when we have track context. play = heard to (nearly)
  // the end, skip = ended early – regardless of who, if anyone, gets the credit.
  case ctx.track_uri {
    Some(uri) -> {
      let _ =
        queries.upsert_track_features(
          state.db,
          uri,
          ctx.track_name,
          ctx.artist_name,
          ctx.album_name,
          ctx.track_duration_ms,
          ctx.genre,
          ctx.release_date,
          ctx.musicbrainz_id,
          ctx.track_no,
          ctx.disc_no,
          now,
          is_track_end && !is_skip,
          is_track_end && is_skip,
        )
      Nil
    }
    None -> Nil
  }

  // Affinity updates on track_ended (before context gets wiped)
  let state = case is_track_end, user_id {
    True, Some(uid) -> {
      // Update track affinity
      case ctx.track_uri {
        Some(uri) -> {
          let _ =
            queries.update_track_affinity_listen(
              state.db,
              uid,
              uri,
              listen_ms,
              listen_pct,
              is_skip,
              is_early_skip,
              now,
            )
          Nil
        }
        None -> Nil
      }

      // Update artist affinity
      case ctx.artist_name {
        Some(artist) -> {
          let _ =
            queries.update_artist_affinity_listen(
              state.db,
              uid,
              artist,
              listen_ms,
              is_skip,
            )
          Nil
        }
        None -> Nil
      }

      // Update session track count
      increment_session_track_count(state, uid)
    }
    _, _ -> state
  }

  // Session management for attributed events
  case user_id {
    Some(uid) -> manage_session(state, uid, now)
    None -> state
  }
}

// ─── Attribution matching ──────────────────────────────────────────

/// Command-to-event mapping for attribution.
/// Returns the list of Mopidy methods that could cause a given event.
fn matching_methods(event_type: String) -> List(String) {
  case event_type {
    "track_started" -> [
      "core.playback.play", "core.playback.next", "core.playback.previous",
    ]
    "track_ended" -> [
      "core.playback.stop", "core.playback.next", "core.playback.previous",
    ]
    "paused" -> ["core.playback.pause"]
    "resumed" -> ["core.playback.resume"]
    "seeked" -> ["core.playback.seek"]
    "volume" -> ["core.mixer.set_volume"]
    "tracklist_changed" -> [
      "core.tracklist.add", "core.tracklist.remove",
      "core.tracklist.clear", "core.tracklist.shuffle",
      "core.tracklist.move",
    ]
    _ -> []
  }
}

/// Find a matching attribution for an event type.
/// Returns (user_id, remaining_attributions).
/// Consumes the matched attribution and prunes expired ones.
fn find_attribution(
  attrs: List(PendingAttribution),
  event_type: String,
  now: Int,
) -> #(Option(Int), List(PendingAttribution)) {
  let methods = matching_methods(event_type)

  // Find first matching attribution that hasn't expired
  let result =
    list.fold(attrs, #(None, []), fn(acc, attr) {
      let #(found, remaining) = acc
      let expired = now - attr.timestamp_ms > attribution_ttl_ms

      case expired {
        True -> #(found, remaining)
        False -> {
          case found {
            Some(_) -> #(found, [attr, ..remaining])
            None -> {
              case list.contains(methods, attr.method) {
                True -> #(Some(attr.user_id), remaining)
                False -> #(found, [attr, ..remaining])
              }
            }
          }
        }
      }
    })

  #(result.0, list.reverse(result.1))
}

/// Remove expired attributions
fn prune_attributions(state: State) -> State {
  let now = now_ms()
  let pruned =
    list.filter(state.pending_attributions, fn(attr) {
      now - attr.timestamp_ms <= attribution_ttl_ms
    })
  State(..state, pending_attributions: pruned)
}

// ─── Session management ─────────────────────────────────────────

/// Manage session boundaries for a user. If the gap since last activity
/// exceeds 5 minutes, close the old session and start a new one.
fn manage_session(state: State, user_id: Int, now: Int) -> State {
  case dict.get(state.active_sessions, user_id) {
    Ok(session) -> {
      let gap = now - session.last_activity_ms
      case gap > session_gap_ms {
        True -> {
          // Close old session and start new
          let state = close_user_session(state, user_id, session, now)
          start_user_session(state, user_id, now)
        }
        False -> {
          // Update last activity
          let updated =
            SessionState(..session, last_activity_ms: now)
          State(
            ..state,
            active_sessions: dict.insert(state.active_sessions, user_id, updated),
          )
        }
      }
    }
    Error(_) -> {
      // No active session, start one
      start_user_session(state, user_id, now)
    }
  }
}

fn start_user_session(state: State, user_id: Int, now: Int) -> State {
  let hour = { now / 1000 / 3600 } % 24
  let day = { { now / 1000 / 86_400 } + 4 } % 7

  case queries.create_session(state.db, user_id, now, hour, day) {
    Ok(session_id) -> {
      let session =
        SessionState(
          session_id: session_id,
          started_ms: now,
          last_activity_ms: now,
          track_count: 0,
        )
      State(
        ..state,
        active_sessions: dict.insert(state.active_sessions, user_id, session),
      )
    }
    Error(e) -> {
      logging.log(
        logging.Error,
        "Failed to create session: " <> string.inspect(e),
      )
      state
    }
  }
}

fn close_user_session(
  state: State,
  user_id: Int,
  session: SessionState,
  now: Int,
) -> State {
  let _ =
    queries.close_session(
      state.db,
      session.session_id,
      now,
      session.track_count,
      None,
    )
  State(
    ..state,
    active_sessions: dict.delete(state.active_sessions, user_id),
  )
}

fn increment_session_track_count(state: State, user_id: Int) -> State {
  case dict.get(state.active_sessions, user_id) {
    Ok(session) -> {
      let updated =
        SessionState(..session, track_count: session.track_count + 1)
      State(
        ..state,
        active_sessions: dict.insert(state.active_sessions, user_id, updated),
      )
    }
    Error(_) -> state
  }
}

// ─── Snapshot builder ──────────────────────────────────────────────

fn build_snapshot(ctx: tracker.PlaybackContext) -> PlaybackStateSnapshot {
  PlaybackStateSnapshot(
    playback_state: ctx.playback_state,
    track_uri: ctx.track_uri,
    track_name: ctx.track_name,
    artist_name: ctx.artist_name,
    album_name: ctx.album_name,
    track_duration_ms: ctx.track_duration_ms,
    position_ms: ctx.position_ms,
    volume: ctx.volume,
    queue_length: list.length(ctx.tracklist),
  )
}

// ─── Helpers ───────────────────────────────────────────────────────

fn now_ms() -> Int {
  timestamp.system_time()
  |> timestamp.to_unix_seconds()
  |> float.multiply(1000.0)
  |> float.round
}
