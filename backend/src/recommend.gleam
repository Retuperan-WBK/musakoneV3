/// Recommendation engine.
///
/// Turns listening history (per-user track/artist affinity, what the room as a
/// whole loves, what was played recently) into concrete tracks to play next.
/// Candidates come from Mopidy/Tidal – an artist's top tracks, Tidal's
/// "<Artist> (Artist Radio)" mixes and the account's daily discovery mix – as
/// well as from the room's own favourites, so the pool is "every song in the
/// world" and not only what has already been queued once.
import db/queries
import gleam/dict.{type Dict}
import gleam/dynamic/decode
import gleam/erlang/process.{type Subject}
import gleam/float
import gleam/json
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/set.{type Set}
import gleam/string
import gleam/time/timestamp
import logging
import mopidy_rpc.{type Ref, type RpcMessage}
import sqlight

pub type Scope {
  /// Taste of one person
  ForUser(user_id: Int)
  /// Blend of everyone who has been active lately
  ForRoom
}

pub type Options {
  Options(
    limit: Int,
    /// 0.0 = only safe favourites … 1.0 = mostly new music
    discovery: Float,
    /// Tracks that must not be suggested (e.g. the current queue)
    exclude_uris: List(String),
  )
}

pub type Recommendation {
  Recommendation(
    uri: String,
    name: String,
    artist: String,
    album: Option(String),
    duration_ms: Option(Int),
    score: Float,
    reasons: List(String),
  )
}

type Candidate {
  Candidate(
    uri: String,
    name: String,
    artist: Option(String),
    score: Float,
    reasons: List(String),
    /// Somebody in the seed set already has history with the track
    familiar: Bool,
  )
}

/// How many of the strongest artists seed catalogue/radio lookups
const seed_artist_count = 5

/// Tracks taken from each radio / discovery mix
const mix_track_count = 40

/// Tracks played inside this window are not suggested again (48 h)
const recent_window_ms = 172_800_000

/// Users active inside this window shape the room profile (7 d)
const active_window_ms = 604_800_000

/// Cache lifetime for an artist's catalogue and mix contents (6 h)
const browse_cache_ms = 21_600_000

/// Cache lifetime for the personalised Tidal listings (1 h)
const listing_cache_ms = 3_600_000

const for_you_radio_uri = "tidal:for_you:category:2"

const for_you_mixes_uri = "tidal:for_you:category:0"

/// Maximum tracks by the same artist in one batch
const per_artist_cap = 2

pub fn recommend(
  db: sqlight.Connection,
  rpc: Subject(RpcMessage),
  scope: Scope,
  opts: Options,
) -> Result(List(Recommendation), String) {
  let now = now_ms()
  let discovery = float.clamp(opts.discovery, 0.0, 1.0)
  let seeds = seed_users(db, scope, now)
  let #(artist_scores, track_scores, disliked) = build_profile(db, seeds)

  let recent =
    queries.get_recently_played_uris(db, now - recent_window_ms)
    |> result.unwrap([])
  let excluded: Set(String) =
    set.from_list(list.flatten([opts.exclude_uris, recent, disliked]))
  let artist_index: Dict(String, String) =
    queries.get_artist_index(db) |> result.unwrap([]) |> dict.from_list

  let top_artists =
    artist_scores
    |> dict.to_list
    |> list.sort(fn(a, b) { float.compare(b.1, a.1) })
    |> list.take(seed_artist_count * 2)
    |> weighted_sample(seed_artist_count, fn(a) { a.1 })
  let artist_max =
    top_artists
    |> list.fold(0.0, fn(m, a) { float.max(m, a.1) })
    |> fn(m) {
      case m >. 0.0 {
        True -> m
        False -> 1.0
      }
    }

  let candidates: Dict(String, Candidate) = dict.new()

  // a. Top tracks from the artists the seeds love (Tidal artist page)
  let candidates =
    list.fold(top_artists, candidates, fn(acc, seed) {
      let #(artist, score) = seed
      let weight = score /. artist_max
      case resolve_artist_uri(db, rpc, artist, artist_index, now) {
        None -> acc
        Some(uri) ->
          cached_browse(db, rpc, "browse:" <> uri, uri, browse_cache_ms, now)
          |> list.filter(fn(r) { r.ref_type == "track" })
          |> list.take(12)
          |> list.fold(acc, fn(acc, ref) {
            add_candidate(
              acc,
              ref,
              Some(artist),
              weight,
              "Top track by " <> artist,
            )
          })
      }
    })

  // b. Tidal "Artist Radio" mixes for those artists: similar music beyond their own catalogue
  let radio_refs =
    cached_browse(
      db,
      rpc,
      "browse:" <> for_you_radio_uri,
      for_you_radio_uri,
      listing_cache_ms,
      now,
    )
  let candidates =
    list.fold(top_artists, candidates, fn(acc, seed) {
      let #(artist, score) = seed
      let weight = { score /. artist_max } *. { 0.45 +. 0.45 *. discovery }
      case list.find(radio_refs, fn(r) { r.name == artist <> " (Artist Radio)" }) {
        Error(_) -> acc
        Ok(mix) ->
          cached_browse(db, rpc, "browse:" <> mix.uri, mix.uri, browse_cache_ms, now)
          |> list.filter(fn(r) { r.ref_type == "track" })
          |> list.take(mix_track_count)
          |> list.fold(acc, fn(acc, ref) {
            add_candidate(acc, ref, None, weight, "Sounds like " <> artist)
          })
      }
    })

  // c. Tidal's daily discovery mix for the account – fresh music even with little history
  let candidates = case discovery >. 0.15 {
    False -> candidates
    True -> {
      let mixes =
        cached_browse(
          db,
          rpc,
          "browse:" <> for_you_mixes_uri,
          for_you_mixes_uri,
          listing_cache_ms,
          now,
        )
      case list.find(mixes, fn(r) { string.starts_with(r.name, "My Daily Discovery") }) {
        Error(_) -> candidates
        Ok(mix) ->
          cached_browse(db, rpc, "browse:" <> mix.uri, mix.uri, listing_cache_ms, now)
          |> list.filter(fn(r) { r.ref_type == "track" })
          |> list.take(mix_track_count)
          |> list.fold(candidates, fn(acc, ref) {
            add_candidate(acc, ref, None, 0.15 +. 0.5 *. discovery, "Daily discovery")
          })
      }
    }
  }

  // d. Familiar favourites of the seeds themselves
  let track_max =
    track_scores
    |> dict.values
    |> list.fold(0.0, float.max)
  let favourite_reason = case scope {
    ForUser(_) -> "One of your favourites"
    ForRoom -> "Clubroom favourite"
  }
  let candidates =
    dict.fold(track_scores, candidates, fn(acc, uri, score) {
      case score >. 0.0 && track_max >. 0.0 {
        False -> acc
        True ->
          add_candidate(
            acc,
            mopidy_rpc.Ref(ref_type: "track", uri: uri, name: ""),
            None,
            { score /. track_max } *. { 1.0 -. 0.6 *. discovery },
            favourite_reason,
          )
      }
    })

  // e. For one person, also lean on what the whole room likes
  let candidates = case scope {
    ForRoom -> candidates
    ForUser(_) -> {
      let room = queries.get_room_top_tracks(db, 20) |> result.unwrap([])
      let room_max = case room {
        [#(_, s, _), ..] if s >. 0.0 -> s
        _ -> 1.0
      }
      list.fold(room, candidates, fn(acc, entry) {
        let #(uri, score, _users) = entry
        add_candidate(
          acc,
          mopidy_rpc.Ref(ref_type: "track", uri: uri, name: ""),
          None,
          { score /. room_max } *. 0.4,
          "Clubroom favourite",
        )
      })
    }
  }

  // Score, exclude, and add a novelty/familiarity bias according to the discovery setting
  let ranked =
    candidates
    |> dict.values
    |> list.filter(fn(c) { !set.contains(excluded, c.uri) })
    |> list.map(fn(c) {
      let familiar = c.familiar || dict.has_key(track_scores, c.uri)
      let bias = case familiar {
        True -> 1.0 +. 0.5 *. { 1.0 -. discovery }
        False -> 1.0 +. 0.5 *. discovery
      }
      Candidate(..c, score: c.score *. bias, familiar: familiar)
    })
    |> weighted_sample(opts.limit * 3, fn(c) { c.score })

  case ranked {
    [] -> Ok([])
    _ -> {
      let meta = fetch_metadata(db, rpc, list.map(ranked, fn(c) { c.uri }))
      ranked
      |> list.map(fn(c) { to_recommendation(c, meta) })
      |> diversify(opts.limit)
      |> Ok
    }
  }
}

/// Pick up to `count` items without replacement, each draw proportional to
/// `weight`. Keeps strong items likely but lets the long tail through, so two
/// runs with the same history give different music.
pub fn weighted_sample(items: List(a), count: Int, weight: fn(a) -> Float) -> List(a) {
  items
  |> list.map(fn(item) {
    // Efraimidis–Spirakis: key = u^(1/w), larger key wins
    let w = float.max(weight(item), 0.000001)
    let u = float.max(float.random(), 0.000001)
    let log_u = float.logarithm(u) |> result.unwrap(-20.0)
    let key = float.exponential(log_u /. w)
    #(key, item)
  })
  |> list.sort(fn(a, b) { float.compare(b.0, a.0) })
  |> list.take(count)
  |> list.map(fn(p) { p.1 })
}

// ─── Seeds and taste profile ────────────────────────────────────────

/// (user_id, weight) pairs that define whose taste we are serving
fn seed_users(db: sqlight.Connection, scope: Scope, now: Int) -> List(#(Int, Float)) {
  case scope {
    ForUser(user_id) -> [#(user_id, 1.0)]
    ForRoom -> {
      let active =
        queries.get_active_users(db, now - active_window_ms)
        |> result.unwrap([])
        |> list.map(fn(u) {
          let #(id, _name, last) = u
          let age = now - last
          let weight = case age < 21_600_000, age < 86_400_000 {
            True, _ -> 1.0
            False, True -> 0.7
            False, False -> 0.4
          }
          #(id, weight)
        })
      case active {
        [] ->
          queries.get_users_with_affinity(db)
          |> result.unwrap([])
          |> list.map(fn(id) { #(id, 0.5) })
        _ -> active
      }
    }
  }
}

/// Weighted artist scores, weighted track scores and the union of disliked tracks
fn build_profile(
  db: sqlight.Connection,
  seeds: List(#(Int, Float)),
) -> #(Dict(String, Float), Dict(String, Float), List(String)) {
  list.fold(seeds, #(dict.new(), dict.new(), []), fn(acc, seed) {
    let #(user_id, weight) = seed
    let #(artists, tracks, disliked) = acc

    let artists =
      queries.get_user_artist_affinities(db, user_id, 12)
      |> result.unwrap([])
      |> list.filter(fn(a) { a.affinity_score >. 0.0 })
      |> list.fold(artists, fn(d, a) {
        dict.upsert(d, a.artist_name, fn(existing) {
          option.unwrap(existing, 0.0) +. a.affinity_score *. weight
        })
      })

    let tracks =
      queries.get_user_track_affinities(db, user_id, 30)
      |> result.unwrap([])
      |> list.filter(fn(t) { t.affinity_score >. 0.0 })
      |> list.fold(tracks, fn(d, t) {
        dict.upsert(d, t.track_uri, fn(existing) {
          option.unwrap(existing, 0.0) +. t.affinity_score *. weight
        })
      })

    let disliked =
      list.append(
        disliked,
        queries.get_user_disliked_uris(db, user_id) |> result.unwrap([]),
      )

    #(artists, tracks, disliked)
  })
}

// ─── Candidate bookkeeping ──────────────────────────────────────────

fn add_candidate(
  candidates: Dict(String, Candidate),
  ref: Ref,
  artist: Option(String),
  weight: Float,
  reason: String,
) -> Dict(String, Candidate) {
  dict.upsert(candidates, ref.uri, fn(existing) {
    case existing {
      None ->
        Candidate(
          uri: ref.uri,
          name: ref.name,
          artist: artist,
          score: weight,
          reasons: [reason],
          familiar: False,
        )
      Some(c) ->
        Candidate(
          ..c,
          name: case c.name {
            "" -> ref.name
            _ -> c.name
          },
          artist: option.or(c.artist, artist),
          score: c.score +. weight,
          reasons: case list.contains(c.reasons, reason) {
            True -> c.reasons
            False -> list.append(c.reasons, [reason])
          },
        )
    }
  })
}

/// Find the Mopidy URI for an artist name: from the learned index, else by
/// looking up one of their tracks.
fn resolve_artist_uri(
  db: sqlight.Connection,
  rpc: Subject(RpcMessage),
  artist: String,
  index: Dict(String, String),
  now: Int,
) -> Option(String) {
  case dict.get(index, artist) {
    Ok(uri) -> Some(uri)
    Error(_) -> {
      let track_uri =
        queries.get_track_uri_for_artist(db, artist) |> result.unwrap(None)
      case track_uri {
        None -> None
        Some(track_uri) ->
          case mopidy_rpc.lookup(rpc, [track_uri]) {
            Error(_) -> None
            Ok(found) -> {
              let uri =
                dict.get(found, track_uri)
                |> result.unwrap([])
                |> list.flat_map(fn(t) { t.artists })
                |> list.find(fn(a) { a.name == artist && a.uri != "" })
                |> result.map(fn(a) { a.uri })
              case uri {
                Ok(uri) -> {
                  let _ = queries.upsert_artist_index(db, artist, uri, now)
                  Some(uri)
                }
                Error(_) -> None
              }
            }
          }
      }
    }
  }
}

/// Browse through the SQLite cache; failures degrade to an empty list
fn cached_browse(
  db: sqlight.Connection,
  rpc: Subject(RpcMessage),
  key: String,
  uri: String,
  max_age_ms: Int,
  now: Int,
) -> List(Ref) {
  let cached = case queries.cache_get(db, key, max_age_ms, now) {
    Ok(Some(value)) ->
      json.parse(value, decode.list(cached_ref_decoder()))
      |> option.from_result
    _ -> None
  }
  case cached {
    Some(refs) -> refs
    None ->
      case mopidy_rpc.browse(rpc, uri) {
        Ok(refs) -> {
          let encoded =
            json.array(refs, fn(r) {
              json.object([
                #("type", json.string(r.ref_type)),
                #("uri", json.string(r.uri)),
                #("name", json.string(r.name)),
              ])
            })
            |> json.to_string
          let _ = queries.cache_put(db, key, encoded, now)
          refs
        }
        Error(e) -> {
          logging.log(logging.Warning, "Recommender browse failed for " <> uri <> ": " <> e)
          []
        }
      }
  }
}

fn cached_ref_decoder() -> decode.Decoder(Ref) {
  use ref_type <- decode.field("type", decode.string)
  use uri <- decode.field("uri", decode.string)
  use name <- decode.field("name", decode.string)
  decode.success(mopidy_rpc.Ref(ref_type: ref_type, uri: uri, name: name))
}

// ─── Metadata and final selection ──────────────────────────────────

type Meta {
  Meta(name: String, artist: String, album: Option(String), duration_ms: Option(Int))
}

/// Names/artists/durations for the shortlisted tracks: Mopidy lookup first,
/// what we have stored locally as a fallback.
fn fetch_metadata(
  db: sqlight.Connection,
  rpc: Subject(RpcMessage),
  uris: List(String),
) -> Dict(String, Meta) {
  let local =
    queries.get_track_meta_many(db, uris)
    |> result.unwrap([])
    |> list.map(fn(t) {
      #(
        t.uri,
        Meta(
          name: option.unwrap(t.name, ""),
          artist: option.unwrap(t.artist_name, ""),
          album: t.album_name,
          duration_ms: t.duration_ms,
        ),
      )
    })
    |> dict.from_list

  case mopidy_rpc.lookup(rpc, uris) {
    Error(e) -> {
      logging.log(logging.Warning, "Recommender lookup failed: " <> e)
      local
    }
    Ok(found) ->
      dict.fold(found, local, fn(acc, uri, tracks) {
        case tracks {
          [track, ..] ->
            dict.insert(
              acc,
              uri,
              Meta(
                name: track.name,
                artist: mopidy_rpc.artist_names(track),
                album: track.album,
                duration_ms: track.length_ms,
              ),
            )
          [] -> acc
        }
      })
  }
}

fn to_recommendation(c: Candidate, meta: Dict(String, Meta)) -> Recommendation {
  case dict.get(meta, c.uri) {
    Ok(m) ->
      Recommendation(
        uri: c.uri,
        name: case m.name {
          "" -> c.name
          _ -> m.name
        },
        artist: case m.artist {
          "" -> option.unwrap(c.artist, "")
          _ -> m.artist
        },
        album: m.album,
        duration_ms: m.duration_ms,
        score: c.score,
        reasons: c.reasons,
      )
    Error(_) ->
      Recommendation(
        uri: c.uri,
        name: c.name,
        artist: option.unwrap(c.artist, ""),
        album: None,
        duration_ms: None,
        score: c.score,
        reasons: c.reasons,
      )
  }
}

/// Keep the ranking but never more than `per_artist_cap` tracks by one artist,
/// and drop anything we could not even name.
fn diversify(ranked: List(Recommendation), limit: Int) -> List(Recommendation) {
  let #(picked, _) =
    list.fold(ranked, #([], dict.new()), fn(acc, rec) {
      let #(picked, per_artist) = acc
      let key = string.lowercase(rec.artist)
      let count = dict.get(per_artist, key) |> result.unwrap(0)
      case list.length(picked) >= limit, rec.name == "", count >= per_artist_cap {
        False, False, False -> #(
          [rec, ..picked],
          dict.insert(per_artist, key, count + 1),
        )
        _, _, _ -> acc
      }
    })
  list.reverse(picked)
}

pub fn to_json(rec: Recommendation) -> json.Json {
  json.object([
    #("uri", json.string(rec.uri)),
    #("name", json.string(rec.name)),
    #("artist", json.string(rec.artist)),
    #("album", json.nullable(rec.album, json.string)),
    #("duration_ms", json.nullable(rec.duration_ms, json.int)),
    #("score", json.float(rec.score)),
    #("reasons", json.array(rec.reasons, json.string)),
  ])
}

fn now_ms() -> Int {
  timestamp.system_time()
  |> timestamp.to_unix_seconds()
  |> float.multiply(1000.0)
  |> float.round
}
