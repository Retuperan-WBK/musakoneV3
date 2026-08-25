# MusakoneV3 Backend API

Backend server for MusakoneV3 - runs on port **3001**

## Health Check

### `GET /api/health`
Check server status.

**Response:**
```json
{
  "status": "ok",
  "service": "musakone-backend",
  "timestamp": 1705708800
}
```

---

## Authentication

### `POST /api/auth/register`
Create a new user account.

**Body:**
```json
{
  "username": "string",
  "password": "string"
}
```

**Response (201):**
```json
{
  "id": 1,
  "username": "string",
  "created_at": 1705708800
}
```

### `POST /api/auth/login`
Authenticate and receive JWT token.

**Body:**
```json
{
  "username": "string",
  "password": "string"
}
```

**Response (200):**
```json
{
  "token": "eyJhbGc...",
  "user": {
    "id": 1,
    "username": "string",
    "created_at": 1705708800
  }
}
```

### `GET /api/auth/me`
Get current authenticated user info.

**Headers:**
```
Authorization: Bearer <token>
```

**Response (200):**
```json
{
  "id": 1,
  "authenticated": true
}
```

---

## Analytics

All analytics endpoints require `Authorization: Bearer <token>`.

### `GET /api/analytics/stats`
Per-user event counts: `{ "playback": int, "queue": int, "search": int }`.

### `GET /api/analytics/events`
Latest 50 of the caller's playback / queue / search events plus global counts.

### `GET /api/analytics/affinity?limit=20`
The caller's learned taste: `track_affinities[]` and `artist_affinities[]` ordered by score.

### `GET /api/analytics/admin`
Room-wide dashboard payload (user activity, hourly activity, popular tracks/searches, event distribution, now playing, state timeline).

### `GET /api/analytics/export?offset=0&limit=1000`
Raw event export across all users (ML training data).

### `GET /api/playback/state`
Public "now playing" snapshot (no auth): state, track, position, volume, `queue_length`.

### `GET /api/playback/history?limit=50`
Recent Mopidy state transitions with attribution (`user_id` when a person caused it).

## Mix: recommendations, autoplay, insights

### `GET /api/recommendations?scope=me|room&limit=20`
Tracks to play next, built from listening history and Mopidy/Tidal discovery
(artist top tracks, "<Artist> (Artist Radio)" mixes, the account's Daily Discovery).
`scope=me` serves the caller's taste, `scope=room` blends everyone active in the last 7 days.
Tracks currently queued, played in the last 12 h, or disliked are never suggested.

```json
{
  "scope": "room",
  "generated_at": 1787688541358,
  "items": [
    {
      "uri": "tidal:track:…",
      "name": "One of These Nights",
      "artist": "Eagles",
      "album": "One of These Nights",
      "duration_ms": 291000,
      "score": 1.12,
      "reasons": ["Sounds like Korelon", "Daily discovery"]
    }
  ]
}
```

First call for a new set of seed artists can take several seconds (Tidal browsing);
results are cached in `rpc_cache` for 6 h.

### `GET /api/autoplay`
```json
{ "settings": { "enabled": false, "min_ahead": 2, "batch_size": 5, "discovery": 0.5, "mode": "room" }, "added_24h": 0 }
```

### `PUT /api/autoplay`
Partial update of the same settings object. `mode` is `"room"` or `"user:<id>"`,
`discovery` is 0.0–1.0 (favourites → new music), `min_ahead` 0–20, `batch_size` 1–20.
When enabled, the autoplay actor appends `batch_size` recommended tracks whenever fewer
than `min_ahead` tracks remain after the current one, and restarts playback if the queue
had run dry. Additions are logged as `queue_events` under the `autoplay` system user and
announced to browsers as a `{"event":"autoplay_added","count":n,"tracks":[…]}` frame.

### `POST /api/autoplay/fill`
Add a batch right now regardless of the threshold. Returns `{ "added": n }`, or 409 with
an `error` when nothing could be recommended (e.g. no listening history yet).

### `GET /api/insights`
Everything the Mix → Stats page shows: `now_playing`, `autoplay`, `room` (totals, top
tracks/artists, per-user taste map, plays by UTC hour for 7 d, plays per day for 14 d,
active users, recent queue adds with usernames, recent plays) and `me` (top artists/tracks,
counts, minutes listened).

## WebSocket

### `WS /ws`
Real-time connection for Mopidy communication.

**Protocol:** WebSocket  
**Messages:** JSON-RPC 2.0 (Mopidy format)

Connect to forward commands to Mopidy server and receive real-time updates.

**Example (send):**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "core.playback.play"
}
```

---

## Error Responses

All endpoints return errors in this format:

```json
{
  "error": "Error message description"
}
```

**Common status codes:**
- `400` - Bad Request (invalid JSON or missing fields)
- `401` - Unauthorized (missing or invalid token)
- `404` - Not Found
- `409` - Conflict (e.g., username already exists)
- `500` - Internal Server Error

---

## Environment Variables

```bash
JWT_SECRET=your-secret-key-change-in-production
MOPIDY_URL=http://mopidy:6680
```

## CORS

All endpoints support CORS with:
- Origin: `*`
- Methods: `GET, POST, PUT, DELETE, OPTIONS`
- Headers: `content-type, authorization`
