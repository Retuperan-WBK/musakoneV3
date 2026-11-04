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

## Playlists

All playlists are shared with the whole room: everyone can list and read them, only the
owner can change them. Every playlist object carries `user_id` and `username` (the owner).
Endpoints require `Authorization: Bearer <token>`.

- `GET /api/playlists` — every playlist, newest activity first
- `POST /api/playlists` — `{ "name", "description" }` creates one owned by the caller (201)
- `GET /api/playlists/:id` — `{ "playlist", "tracks": [{ "track_uri", "position" }] }`
- `PUT /api/playlists/:id`, `DELETE /api/playlists/:id` — owner only (403 otherwise)
- `POST /api/playlists/:id/tracks` `{ "track_uri" }`, `POST /api/playlists/:id/tracks/remove`
  `{ "track_uri" }`, `PUT /api/playlists/:id/tracks/reorder` `{ "track_uri", "new_position" }` — owner only
- `GET /api/playlists/containing?track_uri=…` — ids of the caller's playlists holding the track

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
