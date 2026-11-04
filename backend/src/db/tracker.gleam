/// Action tracker – intercepts JSON-RPC commands flowing through the
/// WebSocket proxy and logs user actions to the database.
///
/// Maintains an in-memory PlaybackContext per WebSocket connection,
/// updated from Mopidy events as they pass through. This context
/// enriches every logged user action with "what was playing", "how
/// far into the track", etc. — without storing Mopidy events themselves.
///
/// Only user-initiated commands are tracked. Unauthenticated sessions
/// are silently dropped.
///
import db/queries
import gleam/dict.{type Dict}
import gleam/dynamic/decode
import gleam/float
import gleam/int
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/string
import gleam/time/timestamp
import logging
import sqlight

// ─── Playback Context (in-memory, per connection) ──────────────────

/// A single entry in the tracklist (queue), ordered by position.
pub type TlTrackEntry {
  TlTrackEntry(tlid: Int, uri: String, name: String, artist: String)
}

/// Snapshot of what's currently playing, maintained from Mopidy events
/// flowing through the WebSocket. Never written to DB directly.
pub type PlaybackContext {
  PlaybackContext(
    /// Current track info (from track_playback_started)
    track_uri: Option(String),
    track_name: Option(String),
    artist_name: Option(String),
    album_name: Option(String),
    track_duration_ms: Option(Int),
    /// Extended metadata for ML track features
    genre: Option(String),
    release_date: Option(String),
    musicbrainz_id: Option(String),
    track_no: Option(Int),
    disc_no: Option(Int),
    /// Last known playback position in ms
    position_ms: Option(Int),
    /// Current volume 0-100
    volume: Option(Int),
    /// Playback state: "playing", "paused", "stopped"
    playback_state: Option(String),
    /// Maps JSON-RPC request id → method name for response correlation
    pending_requests: Dict(Int, String),
    /// Full tracklist (queue) ordered by position, for enriching queue events
    tracklist: List(TlTrackEntry),
    /// tlid of the currently playing track (for relative position context)
    current_tlid: Option(Int),
    /// URI → human-readable name map from browse/search/lookup responses.
    /// Used to resolve opaque Mopidy URIs to names for browse/lookup events.
    uri_names: Dict(String, String),
    /// Last search query text (for search conversion tracking)
    last_search_query: Option(String),
    /// Timestamp of last search (for 30s conversion window)
    last_search_timestamp_ms: Option(Int),
    /// Track uris of the last search reply, in result order (for result_position)
    last_search_results: List(String),
    /// Last logged tracklist.add (uris json, time) to drop double submits
    last_add_uris: Option(String),
    last_add_ms: Int,
  )
}

/// Create an empty context for a new connection
pub fn empty_context() -> PlaybackContext {
  PlaybackContext(
    track_uri: None,
    track_name: None,
    artist_name: None,
    album_name: None,
    track_duration_ms: None,
    genre: None,
    release_date: None,
    musicbrainz_id: None,
    track_no: None,
    disc_no: None,
    position_ms: None,
    volume: None,
    playback_state: None,
    pending_requests: dict.new(),
    tracklist: [],
    current_tlid: None,
    uri_names: dict.new(),
    last_search_query: None,
    last_search_timestamp_ms: None,
    last_search_results: [],
    last_add_uris: None,
    last_add_ms: 0,
  )
}

/// Record an outgoing JSON-RPC request so we can correlate the response.
/// Called on every message from browser → Mopidy.
pub fn record_request(ctx: PlaybackContext, raw: String) -> PlaybackContext {
  let id_method_decoder = {
    use id <- decode.field("id", decode.int)
    use method <- decode.field("method", decode.string)
    decode.success(#(id, method))
  }

  case json.parse(raw, id_method_decoder) {
    Ok(#(id, method)) ->
      PlaybackContext(
        ..ctx,
        pending_requests: dict.insert(ctx.pending_requests, id, method),
      )
    Error(_) -> ctx
  }
}

/// True for events (no "id") and for replies to a request this connection sent.
pub fn owns_response(ctx: PlaybackContext, raw: String) -> Bool {
  let id_decoder = {
    use id <- decode.field("id", decode.int)
    decode.success(id)
  }
  case json.parse(raw, id_decoder) {
    Ok(id) -> dict.has_key(ctx.pending_requests, id)
    Error(_) -> True
  }
}

/// Number of tracks in a core.library.search reply this connection is waiting for.
pub fn search_result_count(ctx: PlaybackContext, raw: String) -> Option(Int) {
  let id_decoder = {
    use id <- decode.field("id", decode.int)
    decode.success(id)
  }
  case json.parse(raw, id_decoder) {
    Ok(id) ->
      case dict.get(ctx.pending_requests, id) {
        Ok("core.library.search") -> {
          let tracks_decoder =
            decode.at(
              ["result"],
              decode.list(decode.optionally_at(
                ["tracks"],
                [],
                decode.list(decode.dynamic),
              )),
            )
          case json.parse(raw, tracks_decoder) {
            Ok(per_backend) ->
              Some(list.fold(per_backend, 0, fn(n, t) { n + list.length(t) }))
            Error(_) -> Some(0)
          }
        }
        _ -> None
      }
    Error(_) -> None
  }
}

/// Update context from a Mopidy message flowing through.
/// Handles both events (has "event" field) and JSON-RPC responses
/// (has "id" + "result" — correlated to the original request method).
pub fn update_context(ctx: PlaybackContext, raw: String) -> PlaybackContext {
  // Try as Mopidy event first
  let event_decoder = {
    use event <- decode.field("event", decode.string)
    decode.success(event)
  }

  case json.parse(raw, event_decoder) {
    Ok(event_name) -> update_from_event(ctx, event_name, raw)
    Error(_) -> {
      // Try as JSON-RPC response (correlate via request id)
      let id_decoder = {
        use id <- decode.field("id", decode.int)
        decode.success(id)
      }
      case json.parse(raw, id_decoder) {
        Ok(id) -> update_from_response(ctx, id, raw)
        Error(_) -> ctx
      }
    }
  }
}

fn update_from_event(
  ctx: PlaybackContext,
  event: String,
  raw: String,
) -> PlaybackContext {
  case event {
    "track_playback_started" -> {
      let info = extract_tl_track_info(raw)
      let tlid_decoder = decode.at(["tl_track", "tlid"], decode.int)
      let tlid = case json.parse(raw, tlid_decoder) {
        Ok(id) -> Some(id)
        Error(_) -> ctx.current_tlid
      }
      PlaybackContext(
        ..ctx,
        track_uri: info.uri,
        track_name: info.name,
        artist_name: info.artist,
        album_name: info.album,
        track_duration_ms: info.duration,
        genre: info.genre,
        release_date: info.release_date,
        musicbrainz_id: info.musicbrainz_id,
        track_no: info.track_no,
        disc_no: info.disc_no,
        position_ms: Some(0),
        playback_state: Some("playing"),
        current_tlid: tlid,
      )
    }

    "track_playback_ended" -> {
      let pos = extract_int_field(raw, "time_position")
      PlaybackContext(..ctx, position_ms: pos, playback_state: Some("stopped"))
    }

    "track_playback_paused" -> {
      let pos = extract_int_field(raw, "time_position")
      PlaybackContext(..ctx, position_ms: pos, playback_state: Some("paused"))
    }

    "track_playback_resumed" -> {
      let pos = extract_int_field(raw, "time_position")
      PlaybackContext(..ctx, position_ms: pos, playback_state: Some("playing"))
    }

    "volume_changed" -> {
      let vol = extract_int_field(raw, "volume")
      PlaybackContext(..ctx, volume: vol)
    }

    "seeked" -> {
      let pos = extract_int_field(raw, "time_position")
      PlaybackContext(..ctx, position_ms: pos)
    }

    _ -> ctx
  }
}

/// Update context from a JSON-RPC response by correlating the response id
/// to the original request method. This catches initial state queries like
/// get_current_tl_track, get_volume, get_state, get_time_position.
fn update_from_response(
  ctx: PlaybackContext,
  id: Int,
  raw: String,
) -> PlaybackContext {
  case dict.get(ctx.pending_requests, id) {
    Error(_) -> ctx
    Ok(method) -> {
      // Remove from pending
      let new_pending = dict.delete(ctx.pending_requests, id)
      let ctx = PlaybackContext(..ctx, pending_requests: new_pending)

      case method {
        "core.playback.get_current_tl_track" -> {
          // result: {tlid, track: {uri, name, artists, album, length}} or null
          let info = extract_tl_track_info_from_result(raw)
          let tlid_decoder = decode.at(["result", "tlid"], decode.int)
          let tlid = case json.parse(raw, tlid_decoder) {
            Ok(id) -> Some(id)
            Error(_) -> ctx.current_tlid
          }
          PlaybackContext(
            ..ctx,
            track_uri: info.uri,
            track_name: info.name,
            artist_name: info.artist,
            album_name: info.album,
            track_duration_ms: info.duration,
            genre: info.genre,
            release_date: info.release_date,
            musicbrainz_id: info.musicbrainz_id,
            track_no: info.track_no,
            disc_no: info.disc_no,
            current_tlid: tlid,
          )
        }

        "core.playback.get_current_track" -> {
          // result: {uri, name, artists, album, length} or null
          let info = extract_track_info_from_result(raw)
          PlaybackContext(
            ..ctx,
            track_uri: info.uri,
            track_name: info.name,
            artist_name: info.artist,
            album_name: info.album,
            track_duration_ms: info.duration,
            genre: info.genre,
            release_date: info.release_date,
            musicbrainz_id: info.musicbrainz_id,
            track_no: info.track_no,
            disc_no: info.disc_no,
          )
        }

        "core.mixer.get_volume" -> {
          // result: 75 (int or null)
          let vol = extract_int_field(raw, "result")
          PlaybackContext(..ctx, volume: vol)
        }

        "core.playback.get_state" -> {
          // result: "playing" | "paused" | "stopped"
          let state_decoder = decode.at(["result"], decode.string)
          case json.parse(raw, state_decoder) {
            Ok(state) -> PlaybackContext(..ctx, playback_state: Some(state))
            Error(_) -> ctx
          }
        }

        "core.playback.get_time_position" -> {
          // result: 12345 (int or null)
          let pos = extract_int_field(raw, "result")
          PlaybackContext(..ctx, position_ms: pos)
        }

        "core.tracklist.get_tl_tracks" -> {
          // result: [{tlid, track: {uri, name, artists, album, length}}, ...]
          let tl = parse_tracklist_result(raw)
          PlaybackContext(..ctx, tracklist: tl)
        }

        "core.library.browse" -> {
          // result: [{__model__: "Ref", uri, name, type}, ...]
          // Populate uri→name map from browse results
          let names = extract_ref_names(raw)
          let new_map =
            list.fold(names, ctx.uri_names, fn(acc, pair) {
              dict.insert(acc, pair.0, pair.1)
            })
          PlaybackContext(..ctx, uri_names: new_map)
        }

        "core.library.search" -> {
          // result: [{uri, tracks: [...], artists: [...], albums: [...]}, ...]
          // Populate uri→name map from all result items
          let names = extract_search_result_names(raw)
          let new_map =
            list.fold(names, ctx.uri_names, fn(acc, pair) {
              dict.insert(acc, pair.0, pair.1)
            })
          PlaybackContext(
            ..ctx,
            uri_names: new_map,
            last_search_results: extract_search_track_uris(raw),
          )
        }

        "core.library.lookup" -> {
          // result: {"uri": [{track}, ...], ...}
          let names = extract_lookup_result_names(raw)
          let new_map =
            list.fold(names, ctx.uri_names, fn(acc, pair) {
              dict.insert(acc, pair.0, pair.1)
            })
          PlaybackContext(..ctx, uri_names: new_map)
        }

        _ -> ctx
      }
    }
  }
}

// ─── Command tracking ──────────────────────────────────────────────

/// Track a user command (browser → Mopidy).
/// Enriches the logged event with the current playback context.
/// Silently drops events if user_id is None (unauthenticated).
pub fn track_command(
  db: sqlight.Connection,
  user_id: Option(Int),
  ctx: PlaybackContext,
  raw: String,
) -> PlaybackContext {
  case user_id {
    None -> ctx
    Some(uid) -> {
      case parse_jsonrpc_request(raw) {
        Ok(req) -> handle_command(db, uid, ctx, req)
        Error(_) -> ctx
      }
    }
  }
}

/// Extract the JSON-RPC method name from a raw request string.
/// Used by the playback state actor for attribution matching.
pub fn extract_method(raw: String) -> Result(String, Nil) {
  let method_decoder = {
    use method <- decode.field("method", decode.string)
    decode.success(method)
  }

  case json.parse(raw, method_decoder) {
    Ok(method) -> Ok(method)
    Error(_) -> Error(Nil)
  }
}

// ─── Internal types ────────────────────────────────────────────────

type JsonRpcRequest {
  JsonRpcRequest(method: String, params: Option(String))
}

// ─── JSON parsing ──────────────────────────────────────────────────

fn parse_jsonrpc_request(raw: String) -> Result(JsonRpcRequest, Nil) {
  case extract_method(raw) {
    Ok(method) -> Ok(JsonRpcRequest(method: method, params: Some(raw)))
    Error(_) -> Error(Nil)
  }
}

// ─── Command handling (user actions) ───────────────────────────────

fn handle_command(
  db: sqlight.Connection,
  user_id: Int,
  ctx: PlaybackContext,
  req: JsonRpcRequest,
) -> PlaybackContext {
  let now = now_ms()

  case req.method {
    // ── Playback commands ────────────────────────────────
    // Every playback action gets the full current track context so we
    // know WHAT the user was doing. E.g. "skipped track X at 30s" is a
    // KEY signal: skip at 10s = dislike, full listen + next = like.
    "core.playback.play" -> {
      log_playback_with_context(db, user_id, now, "play", ctx)
      ctx
    }

    "core.playback.pause" -> {
      log_playback_with_context(db, user_id, now, "pause", ctx)
      ctx
    }

    "core.playback.resume" -> {
      log_playback_with_context(db, user_id, now, "resume", ctx)
      ctx
    }

    "core.playback.stop" -> {
      log_playback_with_context(db, user_id, now, "stop", ctx)
      ctx
    }

    "core.playback.next" -> {
      log_playback_with_context(db, user_id, now, "next", ctx)
      ctx
    }

    "core.playback.previous" -> {
      log_playback_with_context(db, user_id, now, "previous", ctx)
      ctx
    }

    "core.playback.seek" -> {
      let seek_to = extract_int_param(req.params, "time_position")
      log_playback(
        db,
        user_id,
        now,
        "seek",
        ctx.track_uri,
        ctx.track_name,
        ctx.artist_name,
        ctx.album_name,
        ctx.track_duration_ms,
        ctx.position_ms,
        seek_to,
        ctx.volume,
      )
      ctx
    }

    "core.mixer.set_volume" -> {
      let vol = extract_int_param(req.params, "volume")
      log_playback(
        db,
        user_id,
        now,
        "volume",
        ctx.track_uri,
        ctx.track_name,
        ctx.artist_name,
        ctx.album_name,
        ctx.track_duration_ms,
        ctx.position_ms,
        None,
        vol,
      )
      ctx
    }

    // ── Queue commands ───────────────────────────────────
    // Every queue event stores track names + queue length + position
    // relative to now-playing. This gives ML: "user moved X closer
    // to now-playing" = positive signal, "user removed Y" = negative.
    "core.tracklist.add" -> {
      let uris = extract_string_array_param(req.params, "uris")
      let uri_list = extract_raw_string_array_param(req.params, "uris")
      let at_pos = extract_int_param(req.params, "at_position")
      let current_index = find_current_index(ctx)
      let queue_len = list.length(ctx.tracklist)
      let event_type = case at_pos {
        Some(_) -> "add_at_position"
        None -> "add"
      }
      // Store how far from now-playing the insert is
      let relative_pos = case at_pos, current_index {
        Some(ap), Some(ci) -> Some(ap - ci)
        _, _ -> None
      }
      // Resolve URIs to human-readable names from our context
      let track_names = case uri_list {
        [] -> None
        _ -> {
          let resolved_names =
            list.filter_map(uri_list, fn(uri) {
              case dict.get(ctx.uri_names, uri) {
                Ok(name) -> Ok(name)
                Error(_) -> Error(Nil)
              }
            })
          case resolved_names {
            [] -> None
            names -> Some(json.to_string(json.array(names, json.string)))
          }
        }
      }
      // The same uris again within 1.5 s is a double submit, not a second add
      let duplicate =
        uris != None
        && uris == ctx.last_add_uris
        && now - ctx.last_add_ms < 1500
      case duplicate {
        True -> Nil
        False ->
          log_queue(
            db,
            user_id,
            now,
            event_type,
            uris,
            track_names,
            at_pos,
            relative_pos,
            None,
            Some(queue_len),
          )
      }
      let ctx = PlaybackContext(..ctx, last_add_uris: uris, last_add_ms: now)

      // Queue affinity: update for each added URI
      list.each(uri_list, fn(uri) {
        let _ =
          queries.update_track_affinity_queue(db, user_id, uri, "add", now)
        Nil
      })

      // Search conversion: check if within 30s of a search
      case ctx.last_search_query, ctx.last_search_timestamp_ms {
        Some(query), Some(search_ts) -> {
          let elapsed = now - search_ts
          case elapsed <= 30_000 {
            True -> {
              // One conversion row per add (not per uri): the first uri's rank
              // in the last result list says what the person picked.
              let first = list.first(uri_list) |> option.from_result
              let position =
                option.then(first, fn(u) {
                  position_in(ctx.last_search_results, u) |> option.from_result
                })
              case option.map(first, dict.has_key(ctx.uri_names, _)) {
                Some(True) -> {
                  let _ =
                    queries.log_search_conversion(
                      db,
                      user_id,
                      now,
                      Some(query),
                      first,
                      position,
                      list.length(uri_list),
                      "add",
                      Some(elapsed),
                    )
                  Nil
                }
                _ -> Nil
              }
              ctx
            }
            False -> ctx
          }
        }
        _, _ -> ctx
      }
    }

    "core.tracklist.remove" -> {
      // Look up track names from our tracklist by tlid
      let #(track_uris_json, track_names_json) =
        extract_remove_track_info(req.params, ctx)
      let queue_len = list.length(ctx.tracklist)
      log_queue(
        db,
        user_id,
        now,
        "remove",
        track_uris_json,
        track_names_json,
        None,
        None,
        None,
        Some(queue_len),
      )

      // Queue affinity: update for each removed URI
      case track_uris_json {
        Some(uris_json) -> {
          case json.parse(uris_json, decode.list(decode.string)) {
            Ok(uri_list) -> {
              list.each(uri_list, fn(uri) {
                let _ =
                  queries.update_track_affinity_queue(
                    db,
                    user_id,
                    uri,
                    "remove",
                    now,
                  )
                Nil
              })
            }
            Error(_) -> Nil
          }
        }
        None -> Nil
      }
      ctx
    }

    "core.tracklist.clear" -> {
      let queue_len = list.length(ctx.tracklist)
      log_queue(
        db,
        user_id,
        now,
        "clear",
        None,
        None,
        None,
        None,
        None,
        Some(queue_len),
      )
      ctx
    }

    "core.tracklist.shuffle" -> {
      let queue_len = list.length(ctx.tracklist)
      log_queue(
        db,
        user_id,
        now,
        "shuffle",
        None,
        None,
        None,
        None,
        None,
        Some(queue_len),
      )
      ctx
    }

    "core.tracklist.move" -> {
      // Move uses indices into the tracklist
      let from_start = extract_int_param(req.params, "start")
      let _from_end = extract_int_param(req.params, "end")
      let to = extract_int_param(req.params, "to_position")
      let current_index = find_current_index(ctx)
      let queue_len = list.length(ctx.tracklist)

      // Look up the track being moved by its index
      let #(track_uris, track_names) = case from_start {
        Some(idx) -> lookup_track_at_index(ctx.tracklist, idx)
        None -> #(None, None)
      }

      // Compute distance-from-now-playing for both source and destination
      // from_position = distance of source from now-playing
      // to_position = distance of destination from now-playing
      // Negative = moved closer (positive signal), positive = moved away
      let #(from_rel, to_rel) = case current_index {
        Some(ci) -> #(
          option.map(from_start, fn(f) { f - ci }),
          option.map(to, fn(t) { t - ci }),
        )
        None -> #(from_start, to)
      }

      log_queue(
        db,
        user_id,
        now,
        "move",
        track_uris,
        track_names,
        None,
        from_rel,
        to_rel,
        Some(queue_len),
      )

      // Queue affinity: if moved closer to now-playing, it's a positive signal
      case track_uris, from_rel, to_rel {
        Some(uri), Some(from_d), Some(to_d) -> {
          // Moved closer if absolute distance decreased
          let moved_closer =
            int.absolute_value(to_d) < int.absolute_value(from_d)
          case moved_closer {
            True -> {
              let _ =
                queries.update_track_affinity_queue(
                  db,
                  user_id,
                  uri,
                  "move_closer",
                  now,
                )
              Nil
            }
            False -> Nil
          }
        }
        _, _, _ -> Nil
      }
      ctx
    }

    // ── Playback option toggles ──────────────────────────
    "core.tracklist.set_repeat" -> {
      let val = extract_bool_as_string(req.params, "value")
      log_playback_toggle(db, user_id, now, "set_repeat", val, ctx)
      ctx
    }

    "core.tracklist.set_random" -> {
      let val = extract_bool_as_string(req.params, "value")
      log_playback_toggle(db, user_id, now, "set_random", val, ctx)
      ctx
    }

    "core.tracklist.set_single" -> {
      let val = extract_bool_as_string(req.params, "value")
      log_playback_toggle(db, user_id, now, "set_single", val, ctx)
      ctx
    }

    "core.tracklist.set_consume" -> {
      let val = extract_bool_as_string(req.params, "value")
      log_playback_toggle(db, user_id, now, "set_consume", val, ctx)
      ctx
    }

    // ── Search / browse commands ─────────────────────────
    "core.library.search" -> {
      let query = extract_search_query(req.params)
      log_search(db, user_id, now, "search", query, None, None)
      // Store search query + timestamp for conversion tracking
      PlaybackContext(
        ..ctx,
        last_search_query: query,
        last_search_timestamp_ms: Some(now),
      )
    }

    "core.library.browse" -> {
      let uri = extract_string_param(req.params, "uri")
      // Resolve the URI to a human-readable name from our map
      let name = case uri {
        Some(u) -> dict.get(ctx.uri_names, u) |> option.from_result
        None -> None
      }
      log_search(db, user_id, now, "browse", name, uri, None)
      ctx
    }

    "core.library.lookup" -> {
      let uri_json = extract_string_array_param(req.params, "uris")
      // Resolve URIs to names
      let names = case extract_raw_string_array_param(req.params, "uris") {
        [] -> None
        uris -> {
          let resolved =
            list.filter_map(uris, fn(u) {
              case dict.get(ctx.uri_names, u) {
                Ok(n) -> Ok(n)
                Error(_) -> Error(Nil)
              }
            })
          case resolved {
            [] -> None
            names -> Some(string.join(names, ", "))
          }
        }
      }
      log_search(db, user_id, now, "lookup", names, uri_json, None)
      ctx
    }

    // ── Read-only / state queries → not tracked ──────────
    _ -> ctx
  }
}

// ─── Convenience wrappers ──────────────────────────────────────────

/// Log playback event enriched with full context (track, position, volume)
fn log_playback_with_context(
  db: sqlight.Connection,
  user_id: Int,
  now: Int,
  event_type: String,
  ctx: PlaybackContext,
) -> Nil {
  log_playback(
    db,
    user_id,
    now,
    event_type,
    ctx.track_uri,
    ctx.track_name,
    ctx.artist_name,
    ctx.album_name,
    ctx.track_duration_ms,
    ctx.position_ms,
    None,
    ctx.volume,
  )
}

/// Log playback toggle with the new value stored in playback_flags
fn log_playback_toggle(
  db: sqlight.Connection,
  user_id: Int,
  now: Int,
  event_type: String,
  value: Option(String),
  ctx: PlaybackContext,
) -> Nil {
  case
    queries.log_playback_event(
      db,
      user_id,
      now,
      event_type,
      ctx.track_uri,
      ctx.track_name,
      ctx.artist_name,
      ctx.album_name,
      ctx.track_duration_ms,
      ctx.position_ms,
      None,
      ctx.volume,
      value,
    )
  {
    Ok(_) -> Nil
    Error(e) ->
      logging.log(
        logging.Error,
        "Failed to log playback event: " <> string.inspect(e),
      )
  }
}

fn log_playback(
  db: sqlight.Connection,
  user_id: Int,
  now: Int,
  event_type: String,
  track_uri: Option(String),
  track_name: Option(String),
  artist_name: Option(String),
  album_name: Option(String),
  track_duration_ms: Option(Int),
  position_ms: Option(Int),
  seek_to_ms: Option(Int),
  volume_level: Option(Int),
) -> Nil {
  case
    queries.log_playback_event(
      db,
      user_id,
      now,
      event_type,
      track_uri,
      track_name,
      artist_name,
      album_name,
      track_duration_ms,
      position_ms,
      seek_to_ms,
      volume_level,
      None,
    )
  {
    Ok(_) -> Nil
    Error(e) ->
      logging.log(
        logging.Error,
        "Failed to log playback event: " <> string.inspect(e),
      )
  }
}

fn log_queue(
  db: sqlight.Connection,
  user_id: Int,
  now: Int,
  event_type: String,
  track_uris: Option(String),
  track_names: Option(String),
  at_position: Option(Int),
  from_position: Option(Int),
  to_position: Option(Int),
  queue_length: Option(Int),
) -> Nil {
  case
    queries.log_queue_event(
      db,
      user_id,
      now,
      event_type,
      track_uris,
      track_names,
      at_position,
      from_position,
      to_position,
      queue_length,
    )
  {
    Ok(_) -> Nil
    Error(e) ->
      logging.log(
        logging.Error,
        "Failed to log queue event: " <> string.inspect(e),
      )
  }
}

fn log_search(
  db: sqlight.Connection,
  user_id: Int,
  now: Int,
  event_type: String,
  query_text: Option(String),
  browse_uri: Option(String),
  result_count: Option(Int),
) -> Nil {
  case
    queries.log_search_event(
      db,
      user_id,
      now,
      event_type,
      query_text,
      browse_uri,
      result_count,
    )
  {
    Ok(_) -> Nil
    Error(e) ->
      logging.log(
        logging.Error,
        "Failed to log search event: " <> string.inspect(e),
      )
  }
}

// ─── Parameter extraction helpers ──────────────────────────────────

fn now_ms() -> Int {
  timestamp.system_time()
  |> timestamp.to_unix_seconds()
  |> float.multiply(1000.0)
  |> float.round
}

fn extract_int_param(raw_params: Option(String), key: String) -> Option(Int) {
  case raw_params {
    None -> None
    Some(raw) -> {
      let decoder = decode.at(["params", key], decode.int)
      case json.parse(raw, decoder) {
        Ok(val) -> Some(val)
        Error(_) -> None
      }
    }
  }
}

fn extract_string_param(
  raw_params: Option(String),
  key: String,
) -> Option(String) {
  case raw_params {
    None -> None
    Some(raw) -> {
      let decoder = decode.at(["params", key], decode.string)
      case json.parse(raw, decoder) {
        Ok(val) -> Some(val)
        Error(_) -> None
      }
    }
  }
}

fn extract_bool_as_string(
  raw_params: Option(String),
  key: String,
) -> Option(String) {
  case raw_params {
    None -> None
    Some(raw) -> {
      let decoder = decode.at(["params", key], decode.bool)
      case json.parse(raw, decoder) {
        Ok(True) -> Some("true")
        Ok(False) -> Some("false")
        Error(_) -> None
      }
    }
  }
}

fn extract_string_array_param(
  raw_params: Option(String),
  key: String,
) -> Option(String) {
  case raw_params {
    None -> None
    Some(raw) -> {
      let decoder = decode.at(["params", key], decode.list(decode.string))
      case json.parse(raw, decoder) {
        Ok(vals) -> Some(json.to_string(json.array(vals, json.string)))
        Error(_) -> None
      }
    }
  }
}

/// Like extract_string_array_param but returns the raw list for iteration
fn extract_raw_string_array_param(
  raw_params: Option(String),
  key: String,
) -> List(String) {
  case raw_params {
    None -> []
    Some(raw) -> {
      let decoder = decode.at(["params", key], decode.list(decode.string))
      case json.parse(raw, decoder) {
        Ok(vals) -> vals
        Error(_) -> []
      }
    }
  }
}

fn extract_search_query(raw_params: Option(String)) -> Option(String) {
  case raw_params {
    None -> None
    Some(raw) -> {
      // Mopidy search params: {"query": {"any": ["search text"]}}
      let decoder =
        decode.at(["params", "query", "any"], decode.list(decode.string))
      case json.parse(raw, decoder) {
        Ok(terms) -> Some(string.join(terms, " "))
        Error(_) -> None
      }
    }
  }
}

// ─── Response name extractors (for uri→name map) ────────────────────────

/// Extract (uri, name) pairs from a core.library.browse response.
/// Response: {"result": [{"uri": "...", "name": "...", "type": "..."}, ...]}
fn extract_ref_names(raw: String) -> List(#(String, String)) {
  let ref_decoder = {
    use uri <- decode.field("uri", decode.string)
    use name <- decode.field("name", decode.string)
    decode.success(#(uri, name))
  }
  let list_decoder = decode.at(["result"], decode.list(ref_decoder))
  case json.parse(raw, list_decoder) {
    Ok(pairs) -> pairs
    Error(_) -> []
  }
}

/// Extract (uri, name) pairs from a core.library.search response.
/// Response: {"result": [{"tracks": [...], "artists": [...], "albums": [...]}, ...]}
/// 1-based index of `uri` in `items`
fn position_in(items: List(String), uri: String) -> Result(Int, Nil) {
  list.index_map(items, fn(u, i) { #(u, i + 1) })
  |> list.key_find(uri)
}

/// Track uris of a core.library.search reply, in the order Mopidy returned them
fn extract_search_track_uris(raw: String) -> List(String) {
  let decoder =
    decode.at(
      ["result"],
      decode.list(decode.optionally_at(
        ["tracks"],
        [],
        decode.list(decode.at(["uri"], decode.string)),
      )),
    )
  case json.parse(raw, decoder) {
    Ok(per_backend) -> list.flatten(per_backend)
    Error(_) -> []
  }
}

fn extract_search_result_names(raw: String) -> List(#(String, String)) {
  // Tracks: [{uri, name, artists: [{name}]}]
  let track_decoder = {
    use uri <- decode.field("uri", decode.string)
    use name <- decode.field("name", decode.string)
    use artists <- decode.optional_field(
      "artists",
      [],
      decode.list({
        use aname <- decode.field("name", decode.string)
        decode.success(aname)
      }),
    )
    let display = case artists {
      [] -> name
      _ -> name <> " - " <> string.join(artists, ", ")
    }
    decode.success(#(uri, display))
  }

  // Artists: [{uri, name}]
  let simple_decoder = {
    use uri <- decode.field("uri", decode.string)
    use name <- decode.field("name", decode.string)
    decode.success(#(uri, name))
  }

  // Albums: [{uri, name, artists: [{name}]}]
  let album_decoder = {
    use uri <- decode.field("uri", decode.string)
    use name <- decode.field("name", decode.string)
    use artists <- decode.optional_field(
      "artists",
      [],
      decode.list({
        use aname <- decode.field("name", decode.string)
        decode.success(aname)
      }),
    )
    let display = case artists {
      [] -> name
      _ -> name <> " - " <> string.join(artists, ", ")
    }
    decode.success(#(uri, display))
  }

  // Search returns a list of result sets (one per backend)
  let result_set_decoder = {
    use tracks <- decode.optional_field(
      "tracks",
      [],
      decode.list(track_decoder),
    )
    use artists <- decode.optional_field(
      "artists",
      [],
      decode.list(simple_decoder),
    )
    use albums <- decode.optional_field(
      "albums",
      [],
      decode.list(album_decoder),
    )
    decode.success(list.flatten([tracks, artists, albums]))
  }

  let list_decoder = decode.at(["result"], decode.list(result_set_decoder))
  case json.parse(raw, list_decoder) {
    Ok(result_sets) -> list.flatten(result_sets)
    Error(_) -> []
  }
}

/// Extract (uri, name) pairs from a core.library.lookup response.
/// Response: {"result": {"uri1": [{track}, ...], "uri2": [...]}}
fn extract_lookup_result_names(raw: String) -> List(#(String, String)) {
  let track_decoder = {
    use uri <- decode.field("uri", decode.string)
    use name <- decode.field("name", decode.string)
    use artists <- decode.optional_field(
      "artists",
      [],
      decode.list({
        use aname <- decode.field("name", decode.string)
        decode.success(aname)
      }),
    )
    let display = case artists {
      [] -> name
      _ -> name <> " - " <> string.join(artists, ", ")
    }
    decode.success(#(uri, display))
  }

  // The result is a dict of uri -> list of tracks
  // We decode as key-value pairs
  let dict_decoder =
    decode.at(
      ["result"],
      decode.dict(decode.string, decode.list(track_decoder)),
    )
  case json.parse(raw, dict_decoder) {
    Ok(result_dict) -> dict.values(result_dict) |> list.flatten
    Error(_) -> []
  }
}

// ─── Mopidy event field extractors (for context updates) ───────────

fn extract_int_field(raw: String, key: String) -> Option(Int) {
  let decoder = decode.at([key], decode.int)
  case json.parse(raw, decoder) {
    Ok(val) -> Some(val)
    Error(_) -> None
  }
}

type TrackInfo {
  TrackInfo(
    uri: Option(String),
    name: Option(String),
    artist: Option(String),
    album: Option(String),
    duration: Option(Int),
    genre: Option(String),
    release_date: Option(String),
    musicbrainz_id: Option(String),
    track_no: Option(Int),
    disc_no: Option(Int),
  )
}

fn extract_tl_track_info(raw: String) -> TrackInfo {
  let uri_decoder = decode.at(["tl_track", "track", "uri"], decode.string)
  let name_decoder = decode.at(["tl_track", "track", "name"], decode.string)
  let length_decoder = decode.at(["tl_track", "track", "length"], decode.int)

  let artist_decoder =
    decode.at(
      ["tl_track", "track", "artists"],
      decode.list({
        use name <- decode.field("name", decode.string)
        decode.success(name)
      }),
    )

  let album_decoder =
    decode.at(["tl_track", "track", "album", "name"], decode.string)

  let uri =
    json.parse(raw, uri_decoder) |> result.map(Some) |> result.unwrap(None)
  let name =
    json.parse(raw, name_decoder) |> result.map(Some) |> result.unwrap(None)
  let duration =
    json.parse(raw, length_decoder) |> result.map(Some) |> result.unwrap(None)
  let artist = case json.parse(raw, artist_decoder) {
    Ok(artists) -> Some(string.join(artists, ", "))
    Error(_) -> None
  }
  let album =
    json.parse(raw, album_decoder) |> result.map(Some) |> result.unwrap(None)

  // Extended metadata
  let genre =
    json.parse(raw, decode.at(["tl_track", "track", "genre"], decode.string))
    |> result.map(Some)
    |> result.unwrap(None)
  let release_date =
    json.parse(raw, decode.at(["tl_track", "track", "date"], decode.string))
    |> result.map(Some)
    |> result.unwrap(None)
  let musicbrainz_id =
    json.parse(
      raw,
      decode.at(["tl_track", "track", "musicbrainz_id"], decode.string),
    )
    |> result.map(Some)
    |> result.unwrap(None)
  let track_no =
    json.parse(raw, decode.at(["tl_track", "track", "track_no"], decode.int))
    |> result.map(Some)
    |> result.unwrap(None)
  let disc_no =
    json.parse(raw, decode.at(["tl_track", "track", "disc_no"], decode.int))
    |> result.map(Some)
    |> result.unwrap(None)

  TrackInfo(
    uri: uri,
    name: name,
    artist: artist,
    album: album,
    duration: duration,
    genre: genre,
    release_date: release_date,
    musicbrainz_id: musicbrainz_id,
    track_no: track_no,
    disc_no: disc_no,
  )
}

/// Extract track info from a JSON-RPC response to get_current_tl_track
/// Path: result.track.{uri,name,artists,album,length}
fn extract_tl_track_info_from_result(raw: String) -> TrackInfo {
  let uri_decoder = decode.at(["result", "track", "uri"], decode.string)
  let name_decoder = decode.at(["result", "track", "name"], decode.string)
  let length_decoder = decode.at(["result", "track", "length"], decode.int)

  let artist_decoder =
    decode.at(
      ["result", "track", "artists"],
      decode.list({
        use name <- decode.field("name", decode.string)
        decode.success(name)
      }),
    )

  let album_decoder =
    decode.at(["result", "track", "album", "name"], decode.string)

  let uri =
    json.parse(raw, uri_decoder) |> result.map(Some) |> result.unwrap(None)
  let name =
    json.parse(raw, name_decoder) |> result.map(Some) |> result.unwrap(None)
  let duration =
    json.parse(raw, length_decoder) |> result.map(Some) |> result.unwrap(None)
  let artist = case json.parse(raw, artist_decoder) {
    Ok(artists) -> Some(string.join(artists, ", "))
    Error(_) -> None
  }
  let album =
    json.parse(raw, album_decoder) |> result.map(Some) |> result.unwrap(None)

  // Extended metadata
  let genre =
    json.parse(raw, decode.at(["result", "track", "genre"], decode.string))
    |> result.map(Some)
    |> result.unwrap(None)
  let release_date =
    json.parse(raw, decode.at(["result", "track", "date"], decode.string))
    |> result.map(Some)
    |> result.unwrap(None)
  let musicbrainz_id =
    json.parse(
      raw,
      decode.at(["result", "track", "musicbrainz_id"], decode.string),
    )
    |> result.map(Some)
    |> result.unwrap(None)
  let track_no =
    json.parse(raw, decode.at(["result", "track", "track_no"], decode.int))
    |> result.map(Some)
    |> result.unwrap(None)
  let disc_no =
    json.parse(raw, decode.at(["result", "track", "disc_no"], decode.int))
    |> result.map(Some)
    |> result.unwrap(None)

  TrackInfo(
    uri: uri,
    name: name,
    artist: artist,
    album: album,
    duration: duration,
    genre: genre,
    release_date: release_date,
    musicbrainz_id: musicbrainz_id,
    track_no: track_no,
    disc_no: disc_no,
  )
}

/// Extract track info from a JSON-RPC response to get_current_track
/// Path: result.{uri,name,artists,album,length}
fn extract_track_info_from_result(raw: String) -> TrackInfo {
  let uri_decoder = decode.at(["result", "uri"], decode.string)
  let name_decoder = decode.at(["result", "name"], decode.string)
  let length_decoder = decode.at(["result", "length"], decode.int)

  let artist_decoder =
    decode.at(
      ["result", "artists"],
      decode.list({
        use name <- decode.field("name", decode.string)
        decode.success(name)
      }),
    )

  let album_decoder = decode.at(["result", "album", "name"], decode.string)

  let uri =
    json.parse(raw, uri_decoder) |> result.map(Some) |> result.unwrap(None)
  let name =
    json.parse(raw, name_decoder) |> result.map(Some) |> result.unwrap(None)
  let duration =
    json.parse(raw, length_decoder) |> result.map(Some) |> result.unwrap(None)
  let artist = case json.parse(raw, artist_decoder) {
    Ok(artists) -> Some(string.join(artists, ", "))
    Error(_) -> None
  }
  let album =
    json.parse(raw, album_decoder) |> result.map(Some) |> result.unwrap(None)

  // Extended metadata
  let genre =
    json.parse(raw, decode.at(["result", "genre"], decode.string))
    |> result.map(Some)
    |> result.unwrap(None)
  let release_date =
    json.parse(raw, decode.at(["result", "date"], decode.string))
    |> result.map(Some)
    |> result.unwrap(None)
  let musicbrainz_id =
    json.parse(raw, decode.at(["result", "musicbrainz_id"], decode.string))
    |> result.map(Some)
    |> result.unwrap(None)
  let track_no =
    json.parse(raw, decode.at(["result", "track_no"], decode.int))
    |> result.map(Some)
    |> result.unwrap(None)
  let disc_no =
    json.parse(raw, decode.at(["result", "disc_no"], decode.int))
    |> result.map(Some)
    |> result.unwrap(None)

  TrackInfo(
    uri: uri,
    name: name,
    artist: artist,
    album: album,
    duration: duration,
    genre: genre,
    release_date: release_date,
    musicbrainz_id: musicbrainz_id,
    track_no: track_no,
    disc_no: disc_no,
  )
}

// ─── Tracklist helpers (for queue event enrichment) ─────────────────

/// Parse the result of core.tracklist.get_tl_tracks into our compact format.
/// result: [{__model__: "TlTrack", tlid: 1, track: {uri, name, artists, ...}}, ...]
fn parse_tracklist_result(raw: String) -> List(TlTrackEntry) {
  let entry_decoder = {
    use tlid <- decode.field("tlid", decode.int)
    use track <- decode.field("track", {
      use uri <- decode.field("uri", decode.string)
      use name <- decode.field("name", decode.string)
      use artists <- decode.optional_field(
        "artists",
        [],
        decode.list({
          use aname <- decode.field("name", decode.string)
          decode.success(aname)
        }),
      )
      decode.success(#(uri, name, artists))
    })
    decode.success(TlTrackEntry(
      tlid: tlid,
      uri: track.0,
      name: track.1,
      artist: string.join(track.2, ", "),
    ))
  }

  let list_decoder = decode.at(["result"], decode.list(entry_decoder))
  case json.parse(raw, list_decoder) {
    Ok(entries) -> entries
    Error(_) -> []
  }
}

/// Find the 0-based index of the currently playing track in the tracklist.
fn find_current_index(ctx: PlaybackContext) -> Option(Int) {
  case ctx.current_tlid {
    None -> None
    Some(tlid) -> find_index_by_tlid(ctx.tracklist, tlid, 0)
  }
}

fn find_index_by_tlid(
  tl: List(TlTrackEntry),
  tlid: Int,
  idx: Int,
) -> Option(Int) {
  case tl {
    [] -> None
    [entry, ..rest] ->
      case entry.tlid == tlid {
        True -> Some(idx)
        False -> find_index_by_tlid(rest, tlid, idx + 1)
      }
  }
}

/// Look up a track in the tracklist by its 0-based index.
/// Returns (track_uri, "name - artist").
fn lookup_track_at_index(
  tl: List(TlTrackEntry),
  idx: Int,
) -> #(Option(String), Option(String)) {
  case list.drop(tl, idx) |> list.first {
    Ok(entry) -> #(Some(entry.uri), Some(entry.name <> " - " <> entry.artist))
    Error(_) -> #(None, None)
  }
}

/// Extract track info for remove by looking up tlids in the tracklist.
/// Returns (track_uris_json, track_names_json).
fn extract_remove_track_info(
  raw_params: Option(String),
  ctx: PlaybackContext,
) -> #(Option(String), Option(String)) {
  case raw_params {
    None -> #(None, None)
    Some(raw) -> {
      // Try tlid criteria first
      let tlid_decoder =
        decode.at(["params", "criteria", "tlid"], decode.list(decode.int))
      case json.parse(raw, tlid_decoder) {
        Ok(tlids) -> {
          let entries =
            list.filter(ctx.tracklist, fn(e) { list.contains(tlids, e.tlid) })
          case entries {
            [] -> #(
              Some(
                json.to_string(
                  json.array(tlids, fn(t) { json.string(int.to_string(t)) }),
                ),
              ),
              None,
            )
            _ -> #(
              Some(
                json.to_string(
                  json.array(entries, fn(e) { json.string(e.uri) }),
                ),
              ),
              Some(
                json.to_string(
                  json.array(entries, fn(e) {
                    json.string(e.name <> " - " <> e.artist)
                  }),
                ),
              ),
            )
          }
        }
        Error(_) -> {
          // Try uri criteria
          let uri_decoder =
            decode.at(["params", "criteria", "uri"], decode.list(decode.string))
          case json.parse(raw, uri_decoder) {
            Ok(uris) -> {
              let entries =
                list.filter(ctx.tracklist, fn(e) { list.contains(uris, e.uri) })
              let names = case entries {
                [] -> None
                _ ->
                  Some(
                    json.to_string(
                      json.array(entries, fn(e) {
                        json.string(e.name <> " - " <> e.artist)
                      }),
                    ),
                  )
              }
              #(Some(json.to_string(json.array(uris, json.string))), names)
            }
            Error(_) -> #(None, None)
          }
        }
      }
    }
  }
}
