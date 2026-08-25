/// Request/response JSON-RPC calls to Mopidy for backend code.
///
/// The event bus only offers fire-and-forget `SendMessage` and a firehose of
/// raw frames. This actor assigns request ids from its own range, remembers who
/// is waiting for each id, and hands the matching response frame back to the
/// caller – so a handler or actor can simply `call(rpc, "core.tracklist.get_tl_tracks", None, 5000)`.
///
/// Callers create the reply subject in their own process, so `call` may be used
/// from HTTP request processes and from other actors alike.
import event_bus.{type BusMessage, type MopidyEvent}
import gleam/dict.{type Dict}
import gleam/dynamic/decode
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

// ─── Mopidy model types ─────────────────────────────────────────────

pub type Ref {
  Ref(ref_type: String, uri: String, name: String)
}

pub type Artist {
  Artist(uri: String, name: String)
}

pub type Track {
  Track(
    uri: String,
    name: String,
    artists: List(Artist),
    album: Option(String),
    length_ms: Option(Int),
  )
}

pub type TlTrack {
  TlTrack(tlid: Int, track: Track)
}

// ─── Actor ──────────────────────────────────────────────────────────

pub type RpcMessage {
  /// A backend caller wants a response to `method`
  Call(
    reply_to: Subject(Result(String, String)),
    method: String,
    params: Option(String),
  )
  /// Bus traffic (via bridge process)
  Incoming(event: MopidyEvent)
}

type Pending {
  Pending(
    reply_to: Subject(Result(String, String)),
    method: String,
    created_ms: Int,
  )
}

type State {
  State(bus: Subject(BusMessage), next_id: Int, pending: Dict(Int, Pending))
}

/// Own id range: browsers count from 1, the playback-state actor from 900_001
const base_request_id = 700_001

/// Pending entries older than this are dropped (the caller gave up long ago)
const pending_ttl_ms = 60_000

pub fn start(
  bus: Subject(BusMessage),
) -> Result(Subject(RpcMessage), actor.StartError) {
  let initial = State(bus: bus, next_id: base_request_id, pending: dict.new())

  actor.new(initial)
  |> actor.on_message(handle_message)
  |> actor.start
  |> result.map(fn(started) {
    let subject = started.data
    let bridge = create_event_bridge(subject)
    event_bus.subscribe(bus, bridge)
    logging.log(logging.Info, "✓ Mopidy RPC actor started")
    subject
  })
}

fn create_event_bridge(actor_subject: Subject(RpcMessage)) -> Subject(MopidyEvent) {
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
  actor_subject: Subject(RpcMessage),
) -> Nil {
  case process.receive(event_subject, 60_000) {
    Ok(event) -> process.send(actor_subject, Incoming(event))
    Error(_) -> Nil
  }
  event_bridge_loop(event_subject, actor_subject)
}

fn handle_message(
  state: State,
  message: RpcMessage,
) -> actor.Next(State, RpcMessage) {
  case message {
    Call(reply_to, method, params) -> {
      let id = state.next_id
      let params_part = case params {
        Some(p) -> ",\"params\":" <> p
        None -> ""
      }
      let msg =
        "{\"jsonrpc\":\"2.0\",\"id\":"
        <> int.to_string(id)
        <> ",\"method\":\""
        <> method
        <> "\""
        <> params_part
        <> "}"
      event_bus.send_command(state.bus, event_bus.SendMessage(msg))

      let now = now_ms()
      let pending =
        state.pending
        |> prune(now)
        |> dict.insert(id, Pending(reply_to: reply_to, method: method, created_ms: now))
      actor.continue(State(..state, next_id: id + 1, pending: pending))
    }

    Incoming(event_bus.MessageReceived(data)) -> {
      case string.contains(data, "\"id\""), dict.size(state.pending) > 0 {
        True, True -> {
          let id_decoder = {
            use id <- decode.field("id", decode.int)
            decode.success(id)
          }
          case json.parse(data, id_decoder) {
            Ok(id) ->
              case dict.get(state.pending, id) {
                Ok(pending) -> {
                  process.send(pending.reply_to, Ok(data))
                  actor.continue(
                    State(..state, pending: dict.delete(state.pending, id)),
                  )
                }
                Error(_) -> actor.continue(state)
              }
            Error(_) -> actor.continue(state)
          }
        }
        _, _ -> actor.continue(state)
      }
    }

    Incoming(event_bus.Disconnected) -> {
      dict.each(state.pending, fn(_, pending) {
        process.send(pending.reply_to, Error("Mopidy disconnected"))
      })
      actor.continue(State(..state, pending: dict.new()))
    }

    Incoming(_) -> actor.continue(state)
  }
}

fn prune(pending: Dict(Int, Pending), now: Int) -> Dict(Int, Pending) {
  dict.filter(pending, fn(_, p) { now - p.created_ms < pending_ttl_ms })
}

fn now_ms() -> Int {
  timestamp.system_time()
  |> timestamp.to_unix_seconds()
  |> float.multiply(1000.0)
  |> float.round
}

// ─── Calling ────────────────────────────────────────────────────────

/// Call a Mopidy method and wait for the raw response frame.
pub fn call(
  rpc: Subject(RpcMessage),
  method: String,
  params: Option(json.Json),
  timeout_ms: Int,
) -> Result(String, String) {
  let reply: Subject(Result(String, String)) = process.new_subject()
  process.send(
    rpc,
    Call(reply, method, option.map(params, json.to_string)),
  )
  case process.receive(reply, timeout_ms) {
    Ok(result) -> result
    Error(Nil) -> Error("Mopidy call timed out: " <> method)
  }
}

/// Call a Mopidy method and decode its `result` field.
pub fn call_decode(
  rpc: Subject(RpcMessage),
  method: String,
  params: Option(json.Json),
  decoder: decode.Decoder(a),
  timeout_ms: Int,
) -> Result(a, String) {
  use raw <- result.try(call(rpc, method, params, timeout_ms))

  let error_decoder = decode.at(["error", "message"], decode.string)
  case json.parse(raw, error_decoder) {
    Ok(message) -> Error("Mopidy error in " <> method <> ": " <> message)
    Error(_) ->
      json.parse(raw, decode.at(["result"], decoder))
      |> result.map_error(fn(e) {
        "Could not decode " <> method <> " response: " <> string.inspect(e)
      })
  }
}

// ─── Decoders ───────────────────────────────────────────────────────

pub fn ref_decoder() -> decode.Decoder(Ref) {
  use ref_type <- decode.optional_field("type", "", decode.string)
  use uri <- decode.field("uri", decode.string)
  use name <- decode.optional_field("name", "", decode.string)
  decode.success(Ref(ref_type: ref_type, uri: uri, name: name))
}

pub fn artist_decoder() -> decode.Decoder(Artist) {
  use uri <- decode.optional_field("uri", "", decode.string)
  use name <- decode.optional_field("name", "", decode.string)
  decode.success(Artist(uri: uri, name: name))
}

pub fn track_decoder() -> decode.Decoder(Track) {
  use uri <- decode.field("uri", decode.string)
  use name <- decode.optional_field("name", "", decode.string)
  use artists <- decode.optional_field(
    "artists",
    [],
    decode.list(artist_decoder()),
  )
  use album <- decode.optional_field(
    "album",
    None,
    decode.optional(decode.at(["name"], decode.string)),
  )
  use length_ms <- decode.optional_field(
    "length",
    None,
    decode.optional(decode.int),
  )
  decode.success(Track(
    uri: uri,
    name: name,
    artists: artists,
    album: album,
    length_ms: length_ms,
  ))
}

pub fn tl_track_decoder() -> decode.Decoder(TlTrack) {
  use tlid <- decode.field("tlid", decode.int)
  use track <- decode.field("track", track_decoder())
  decode.success(TlTrack(tlid: tlid, track: track))
}

// ─── Typed helpers for the calls the backend needs ──────────────────

const default_timeout_ms = 8000

/// Tidal browse calls can be slow; they get a longer budget
const browse_timeout_ms = 15_000

pub fn browse(rpc: Subject(RpcMessage), uri: String) -> Result(List(Ref), String) {
  call_decode(
    rpc,
    "core.library.browse",
    Some(json.object([#("uri", json.string(uri))])),
    decode.list(ref_decoder()),
    browse_timeout_ms,
  )
}

pub fn lookup(
  rpc: Subject(RpcMessage),
  uris: List(String),
) -> Result(Dict(String, List(Track)), String) {
  call_decode(
    rpc,
    "core.library.lookup",
    Some(json.object([#("uris", json.array(uris, json.string))])),
    decode.dict(decode.string, decode.list(track_decoder())),
    browse_timeout_ms,
  )
}

pub fn get_tl_tracks(rpc: Subject(RpcMessage)) -> Result(List(TlTrack), String) {
  call_decode(
    rpc,
    "core.tracklist.get_tl_tracks",
    None,
    decode.list(tl_track_decoder()),
    default_timeout_ms,
  )
}

pub fn get_current_tl_track(
  rpc: Subject(RpcMessage),
) -> Result(Option(TlTrack), String) {
  call_decode(
    rpc,
    "core.playback.get_current_tl_track",
    None,
    decode.optional(tl_track_decoder()),
    default_timeout_ms,
  )
}

pub fn get_playback_state(rpc: Subject(RpcMessage)) -> Result(String, String) {
  call_decode(rpc, "core.playback.get_state", None, decode.string, default_timeout_ms)
}

pub fn tracklist_add(
  rpc: Subject(RpcMessage),
  uris: List(String),
  at_position: Option(Int),
) -> Result(List(TlTrack), String) {
  let params = [#("uris", json.array(uris, json.string))]
  let params = case at_position {
    Some(pos) -> [#("at_position", json.int(pos)), ..params]
    None -> params
  }
  call_decode(
    rpc,
    "core.tracklist.add",
    Some(json.object(params)),
    decode.list(tl_track_decoder()),
    browse_timeout_ms,
  )
}

pub fn play(rpc: Subject(RpcMessage)) -> Result(Nil, String) {
  call(rpc, "core.playback.play", None, default_timeout_ms)
  |> result.map(fn(_) { Nil })
}

/// Names of the artists on a track, joined the way the tracker does it
pub fn artist_names(track: Track) -> String {
  track.artists
  |> list.map(fn(a) { a.name })
  |> string.join(", ")
}
