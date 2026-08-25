/// Autoplay actor: keeps the queue from running dry.
///
/// Watches Mopidy events; whenever fewer than `min_ahead` tracks remain after
/// the one playing, it asks the recommender for a batch that fits the room and
/// appends it. Settings live in the `settings` table and are edited from the app.
import db/queries
import event_bus.{type BusMessage, type MopidyEvent}
import gleam/dict
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
import mopidy_rpc.{type RpcMessage}
import recommend
import sqlight

pub type Settings {
  Settings(
    enabled: Bool,
    /// Refill when fewer than this many tracks are queued after the current one
    min_ahead: Int,
    /// Tracks added per refill
    batch_size: Int,
    /// 0.0 = favourites only … 1.0 = mostly new music
    discovery: Float,
    /// "room" or "user:<id>"
    mode: String,
  )
}

pub type AutoplayMessage {
  BusEvent(event: MopidyEvent)
  /// Debounced: evaluate whether the queue needs topping up
  Check
  /// Top up right now regardless of the threshold; replies with tracks added
  Fill(reply_to: Subject(Result(Int, String)))
  ReloadSettings
  SetSelf(self: Subject(AutoplayMessage))
}

type State {
  State(
    db: sqlight.Connection,
    rpc: Subject(RpcMessage),
    bus: Subject(BusMessage),
    self: Option(Subject(AutoplayMessage)),
    settings: Settings,
    check_pending: Bool,
    last_fill_ms: Int,
  )
}

/// Events settle for this long before the queue is inspected
const check_delay_ms = 1500

/// Never refill more often than this (guards against event storms)
const fill_cooldown_ms = 15_000

pub fn start(
  db: sqlight.Connection,
  rpc: Subject(RpcMessage),
  bus: Subject(BusMessage),
) -> Result(Subject(AutoplayMessage), actor.StartError) {
  let initial =
    State(
      db: db,
      rpc: rpc,
      bus: bus,
      self: None,
      settings: load_settings(db),
      check_pending: False,
      last_fill_ms: 0,
    )

  actor.new(initial)
  |> actor.on_message(handle_message)
  |> actor.start
  |> result.map(fn(started) {
    let subject = started.data
    process.send(subject, SetSelf(subject))
    let bridge = create_event_bridge(subject)
    event_bus.subscribe(bus, bridge)
    logging.log(
      logging.Info,
      "✓ Autoplay actor started (" <> describe(initial.settings) <> ")",
    )
    subject
  })
}

fn create_event_bridge(actor_subject: Subject(AutoplayMessage)) -> Subject(MopidyEvent) {
  let parent_subject: Subject(Subject(MopidyEvent)) = process.new_subject()
  let _ =
    process.spawn(fn() {
      let event_subject = process.new_subject()
      process.send(parent_subject, event_subject)
      event_bridge_loop(event_subject, actor_subject)
    })
  let assert Ok(event_subject) = process.receive(parent_subject, 5000)
  event_subject
}

fn event_bridge_loop(
  event_subject: Subject(MopidyEvent),
  actor_subject: Subject(AutoplayMessage),
) -> Nil {
  case process.receive(event_subject, 60_000) {
    Ok(event) -> process.send(actor_subject, BusEvent(event))
    Error(_) -> Nil
  }
  event_bridge_loop(event_subject, actor_subject)
}

// ─── Settings ───────────────────────────────────────────────────────

pub fn load_settings(db: sqlight.Connection) -> Settings {
  let kv = queries.get_settings(db) |> result.unwrap([]) |> dict.from_list
  Settings(
    enabled: dict.get(kv, "autoplay.enabled") == Ok("1"),
    min_ahead: int_setting(kv, "autoplay.min_ahead", 2),
    batch_size: int_setting(kv, "autoplay.batch_size", 5),
    discovery: float_setting(kv, "autoplay.discovery", 0.5),
    mode: dict.get(kv, "autoplay.mode") |> result.unwrap("room"),
  )
}

pub fn save_settings(db: sqlight.Connection, s: Settings) -> Result(Nil, String) {
  let now = now_ms()
  let enabled = case s.enabled {
    True -> "1"
    False -> "0"
  }
  [
    #("autoplay.enabled", enabled),
    #("autoplay.min_ahead", int.to_string(s.min_ahead)),
    #("autoplay.batch_size", int.to_string(s.batch_size)),
    #("autoplay.discovery", float.to_string(s.discovery)),
    #("autoplay.mode", s.mode),
  ]
  |> list.try_each(fn(kv) {
    queries.set_setting(db, kv.0, kv.1, now)
    |> result.map_error(fn(e) { "Failed to save " <> kv.0 <> ": " <> e.message })
  })
}

pub fn settings_to_json(s: Settings) -> json.Json {
  json.object([
    #("enabled", json.bool(s.enabled)),
    #("min_ahead", json.int(s.min_ahead)),
    #("batch_size", json.int(s.batch_size)),
    #("discovery", json.float(s.discovery)),
    #("mode", json.string(s.mode)),
  ])
}

fn int_setting(kv: dict.Dict(String, String), key: String, default: Int) -> Int {
  dict.get(kv, key)
  |> result.try(int.parse)
  |> result.unwrap(default)
}

fn float_setting(kv: dict.Dict(String, String), key: String, default: Float) -> Float {
  case dict.get(kv, key) {
    Ok(raw) ->
      case float.parse(raw) {
        Ok(f) -> f
        Error(_) ->
          int.parse(raw) |> result.map(int.to_float) |> result.unwrap(default)
      }
    Error(_) -> default
  }
}

fn describe(s: Settings) -> String {
  case s.enabled {
    True ->
      "on, mode "
      <> s.mode
      <> ", keep "
      <> int.to_string(s.min_ahead)
      <> " ahead, batches of "
      <> int.to_string(s.batch_size)
    False -> "off"
  }
}

// ─── Message handling ───────────────────────────────────────────────

fn handle_message(
  state: State,
  message: AutoplayMessage,
) -> actor.Next(State, AutoplayMessage) {
  case message {
    SetSelf(self) -> actor.continue(State(..state, self: Some(self)))

    ReloadSettings -> {
      let settings = load_settings(state.db)
      logging.log(logging.Info, "Autoplay settings: " <> describe(settings))
      // Turning it on should act immediately if the queue is already short
      let state = State(..state, settings: settings)
      actor.continue(schedule_check(state))
    }

    BusEvent(event_bus.MessageReceived(data)) -> {
      case state.settings.enabled && is_queue_related(data) {
        True -> actor.continue(schedule_check(state))
        False -> actor.continue(state)
      }
    }

    BusEvent(_) -> actor.continue(state)

    Check -> {
      let state = State(..state, check_pending: False)
      case state.settings.enabled {
        False -> actor.continue(state)
        True -> {
          let #(state, outcome) = fill(state, False)
          case outcome {
            Error(e) -> logging.log(logging.Warning, "Autoplay: " <> e)
            Ok(_) -> Nil
          }
          actor.continue(state)
        }
      }
    }

    Fill(reply_to) -> {
      let #(state, outcome) = fill(state, True)
      process.send(reply_to, outcome)
      actor.continue(state)
    }
  }
}

fn is_queue_related(data: String) -> Bool {
  string.contains(data, "\"event\"")
  && {
    string.contains(data, "track_playback_ended")
    || string.contains(data, "track_playback_started")
    || string.contains(data, "tracklist_changed")
    || string.contains(data, "playback_state_changed")
  }
}

fn schedule_check(state: State) -> State {
  case state.check_pending, state.self {
    False, Some(self) -> {
      let _ = process.send_after(self, check_delay_ms, Check)
      State(..state, check_pending: True)
    }
    _, _ -> state
  }
}

// ─── Refilling ──────────────────────────────────────────────────────

/// Inspect the queue and top it up when needed (`force` ignores threshold and cooldown).
/// Returns the number of tracks added.
fn fill(state: State, force: Bool) -> #(State, Result(Int, String)) {
  let now = now_ms()
  case !force && now - state.last_fill_ms < fill_cooldown_ms {
    True -> #(state, Ok(0))
    False ->
      case mopidy_rpc.get_tl_tracks(state.rpc), mopidy_rpc.get_current_tl_track(state.rpc) {
        Error(e), _ | _, Error(e) -> #(state, Error(e))
        Ok(tracklist), Ok(current) -> {
          let total = list.length(tracklist)
          let current_index = case current {
            Some(cur) -> index_of_tlid(tracklist, cur.tlid, 0)
            None -> None
          }
          let remaining = case current_index {
            Some(i) -> total - i - 1
            None -> total
          }
          case force || remaining < state.settings.min_ahead {
            False -> #(state, Ok(0))
            True -> add_batch(state, tracklist, current, total, now)
          }
        }
      }
  }
}

fn add_batch(
  state: State,
  tracklist: List(mopidy_rpc.TlTrack),
  current: Option(mopidy_rpc.TlTrack),
  total: Int,
  now: Int,
) -> #(State, Result(Int, String)) {
  let scope = case state.settings.mode {
    "user:" <> id ->
      case int.parse(id) {
        Ok(user_id) -> recommend.ForUser(user_id)
        Error(_) -> recommend.ForRoom
      }
    _ -> recommend.ForRoom
  }
  let opts =
    recommend.Options(
      limit: int.max(1, state.settings.batch_size),
      discovery: state.settings.discovery,
      exclude_uris: list.map(tracklist, fn(t) { t.track.uri }),
    )

  case recommend.recommend(state.db, state.rpc, scope, opts) {
    Error(e) -> #(state, Error("recommendation failed: " <> e))
    Ok([]) -> #(
      state,
      Error("nothing to recommend yet – play and queue some music first"),
    )
    Ok(recs) -> {
      let uris = list.map(recs, fn(r) { r.uri })
      case mopidy_rpc.tracklist_add(state.rpc, uris, None) {
        Error(e) -> #(state, Error("could not add to tracklist: " <> e))
        Ok(added) -> {
          let count = list.length(added)
          log_queue_event(state.db, recs, total, now)
          // The queue had run dry and nothing was playing: get it going again
          case current {
            None -> {
              let _ = mopidy_rpc.play(state.rpc)
              Nil
            }
            Some(_) -> Nil
          }
          announce(state.bus, count, recs)
          logging.log(
            logging.Info,
            "Autoplay added "
              <> int.to_string(count)
              <> " tracks: "
              <> string.join(list.map(recs, fn(r) { r.artist <> " – " <> r.name }), ", "),
          )
          #(State(..state, last_fill_ms: now), Ok(count))
        }
      }
    }
  }
}

fn index_of_tlid(tracklist: List(mopidy_rpc.TlTrack), tlid: Int, index: Int) -> Option(Int) {
  case tracklist {
    [] -> None
    [first, ..rest] ->
      case first.tlid == tlid {
        True -> Some(index)
        False -> index_of_tlid(rest, tlid, index + 1)
      }
  }
}

/// Record the addition under the `autoplay` system user so it shows in the activity log
fn log_queue_event(
  db: sqlight.Connection,
  recs: List(recommend.Recommendation),
  queue_length: Int,
  now: Int,
) -> Nil {
  let system_user =
    queries.get_user_by_username(db, "autoplay")
    |> result.unwrap([])
    |> list.first
  case system_user {
    Error(_) -> logging.log(logging.Warning, "Autoplay: system user missing, not logging")
    Ok(user) -> {
      let uris = json.array(recs, fn(r) { json.string(r.uri) }) |> json.to_string
      let names =
        json.array(recs, fn(r) { json.string(r.name <> " - " <> r.artist) })
        |> json.to_string
      let _ =
        queries.log_queue_event(
          db,
          user.id,
          now,
          "autoplay",
          Some(uris),
          Some(names),
          None,
          None,
          None,
          Some(queue_length),
        )
      Nil
    }
  }
}

/// Tell connected browsers what just happened (they receive bus messages verbatim)
fn announce(bus: Subject(BusMessage), count: Int, recs: List(recommend.Recommendation)) -> Nil {
  json.object([
    #("event", json.string("autoplay_added")),
    #("count", json.int(count)),
    #("tracks", json.array(recs, recommend.to_json)),
  ])
  |> json.to_string
  |> event_bus.MessageReceived
  |> event_bus.publish(bus, _)
}

fn now_ms() -> Int {
  timestamp.system_time()
  |> timestamp.to_unix_seconds()
  |> float.multiply(1000.0)
  |> float.round
}
