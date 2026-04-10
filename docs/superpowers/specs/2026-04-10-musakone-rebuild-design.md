# MusakoneV3 Rebuild: FastAPI + SvelteKit

**Date:** 2026-04-10
**Status:** Approved
**Motivation:** Improve developer experience and iteration speed. Replace Gleam/OTP backend and Preact frontend with Python (FastAPI) and SvelteKit. Add smart caching layer to compensate for Mopidy's slow search and queue operations.

---

## 1. Architecture Overview

```
┌──────────────────────────────┐
│  SvelteKit SPA (Port 5173)   │  Mobile-first, terminal aesthetic
│  Svelte 5 + TypeScript       │  Runes for reactivity
└──────────┬───────────────────┘
           │ HTTP REST + WebSocket
           │
┌──────────▼───────────────────────────────────────┐
│  FastAPI Backend (Port 8000)                      │
│                                                   │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────┐│
│  │ REST API    │  │ WS Proxy     │  │ Cache    ││
│  │ - Auth      │  │ Browser ↔    │  │ Manager  ││
│  │ - Search    │  │ Mopidy       │  │          ││
│  │ - Library   │  │ + broadcast  │  │ - FTS5   ││
│  │ - Playlists │  │              │  │ - Tidal  ││
│  │ - Queue ops │  │              │  │   TTL    ││
│  └─────────────┘  └──────────────┘  └──────────┘│
│                                                   │
│  SQLite (WAL mode)                                │
│  - users, playlists, playlist_tracks              │
│  - library_cache (FTS5), tidal_cache              │
│  - basic event_log                                │
└──────────┬───────────────────────────────────────┘
           │ WebSocket JSON-RPC
           │
┌──────────▼───────────────────┐
│  Mopidy (Port 6680)          │
│  Local files + Tidal         │
└──────────────────────────────┘
```

### Key Architectural Decisions

- **Backend is a smart cache layer**, not just a proxy. Search hits local FTS5 index first, only falls through to Mopidy on cache miss. Queue operations are batched.
- **Single Mopidy WebSocket connection** shared by all browser clients. Backend fans out events to all connected browsers.
- **REST for request-response** (search, library, playlists, auth). WebSocket reserved for real-time events (playback state, queue changes).
- **Frontend is a static SPA** — SvelteKit with `adapter-static`, served by Bun. No SSR.

### What's Dropped (vs current)

- ML-ready tables (track_features, user_track_affinity, user_artist_affinity, listening_sessions, search_conversions)
- Playback attribution system
- Analytics dashboard
- Affinity scoring
- Session tracking

### What's Added

- FTS5 full-text search index for local library (sub-millisecond search)
- Tidal result caching with TTL
- Queue operation batching
- Optimistic UI updates
- Skeleton loaders, haptic feedback, pull-to-refresh

---

## 2. Backend (FastAPI + Python 3.13)

### 2.1 Mopidy Connection

Single persistent WebSocket connection to Mopidy, established on startup as a background task. Auto-reconnect with exponential backoff (1s → 30s, max 10 retries).

All browser clients share this connection:
```
Browser A ──┐
Browser B ──┼── FastAPI ──── single WS ──── Mopidy
Browser C ──┘
```

**Command routing:** Each browser command gets a unique JSON-RPC request ID. Backend tracks pending requests and routes Mopidy responses back to the originating browser WebSocket.

**Event broadcast:** Mopidy events (track_playback_started, tracklist_changed, etc.) are broadcast to all connected browser WebSockets.

### 2.2 Library Cache

**Local files:**
- Full library scan on startup via recursive `library.browse` + `library.lookup`
- Every track indexed into SQLite FTS5 table (name, artist, album, genre, URI, duration)
- Search becomes a local FTS5 query — sub-millisecond response
- Background rescan every 30 minutes (configurable)
- Manual rescan endpoint: `POST /api/library/rescan`

**Tidal:**
- Search results cached with 1-hour TTL
- Track metadata (from `library.lookup`) cached with 24-hour TTL (rarely changes)
- Cache key: normalized query string + source
- On cache miss: query Mopidy → store result → return to client

**Queue batching:**
- `POST /api/queue/add` accepts an array of URIs
- Backend sends a single `tracklist.add` call to Mopidy with all URIs
- Frontend shows optimistic result immediately

### 2.3 REST API

```
# Auth
POST /api/auth/register         # Create account → { token, user }
POST /api/auth/login            # Login → { token, user }
GET  /api/auth/me               # Current user (requires auth)

# Search (cache-first)
GET  /api/search?q=radiohead    # Searches FTS5 first, Tidal cache, then Mopidy

# Library (cached)
GET  /api/library/browse?uri=   # Browse with cached results
GET  /api/library/lookup?uri=   # Track metadata (cached)
POST /api/library/rescan        # Trigger library rescan (requires auth)

# Playlists (CRUD)
GET    /api/playlists                      # User's playlists
POST   /api/playlists                      # Create playlist
GET    /api/playlists/:id                  # Playlist with tracks
PUT    /api/playlists/:id                  # Update playlist
DELETE /api/playlists/:id                  # Delete playlist
POST   /api/playlists/:id/tracks           # Add track(s)
DELETE /api/playlists/:id/tracks/:uri      # Remove track

# Queue (batched operations)
GET  /api/queue                 # Current tracklist (from memory)
POST /api/queue/add             # Add URIs { uris: string[], position?: number }
POST /api/queue/clear           # Clear tracklist
POST /api/queue/shuffle         # Shuffle

# Playback control (convenience — can also go through WebSocket)
POST /api/playback/play         # { tlid?: number }
POST /api/playback/pause
POST /api/playback/next
POST /api/playback/previous
POST /api/playback/volume       # { volume: number }
POST /api/playback/seek         # { position: number }

# WebSocket
WS   /ws?token=<jwt>            # Real-time Mopidy events + command forwarding
```

### 2.4 WebSocket Protocol (Browser ↔ Backend)

**Client → Server (commands):**
```json
{ "jsonrpc": "2.0", "id": 1, "method": "core.playback.play", "params": { "tlid": 5 } }
```
Standard Mopidy JSON-RPC forwarded through to Mopidy.

**Server → Client (events):**
```json
{ "event": "track_playback_started", "tl_track": { ... } }
```
Mopidy events broadcast as-is.

**Server → Client (state sync on connect):**
```json
{ "type": "state_sync", "data": { "playback_state": "playing", "current_track": {...}, "volume": 80, "queue": [...] } }
```
Full state snapshot sent on WebSocket connect so client doesn't need multiple REST calls to hydrate.

### 2.5 SQLite Schema

```sql
-- Auth
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Playlists
CREATE TABLE playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_uri TEXT NOT NULL,
    track_name TEXT,
    artist_name TEXT,
    position INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, track_uri)
);

-- Library cache (FTS5 for instant full-text search)
CREATE VIRTUAL TABLE library_fts USING fts5(
    uri UNINDEXED,
    name,
    artist,
    album,
    genre,
    duration UNINDEXED,
    source UNINDEXED
);

-- Tidal search/metadata cache (TTL-based)
CREATE TABLE tidal_cache (
    cache_key TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    cached_at INTEGER NOT NULL DEFAULT (unixepoch()),
    ttl_seconds INTEGER NOT NULL
);

-- Simple event log
CREATE TABLE event_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER REFERENCES users(id),
    event_type TEXT NOT NULL,
    detail TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
```

WAL mode enabled on connection for concurrent read/write safety.

### 2.6 Auth

- **Password hashing:** bcrypt (via `passlib` or `bcrypt` package)
- **JWT:** HS256 via `python-jose` or `PyJWT`, configurable secret via `JWT_SECRET` env var
- **Token delivery:** Authorization header for REST, query parameter for WebSocket
- **Token payload:** `{ "sub": "<user_id>", "exp": <expiry> }`

### 2.7 Python Project Structure

```
backend/
├── pyproject.toml          # uv project config
├── uv.lock
├── Dockerfile
├── app/
│   ├── main.py             # FastAPI app, lifespan, CORS
│   ├── config.py           # Settings from env vars
│   ├── db.py               # SQLite connection, migrations
│   ├── auth/
│   │   ├── router.py       # Auth endpoints
│   │   ├── service.py      # Register, login, verify
│   │   └── deps.py         # get_current_user dependency
│   ├── mopidy/
│   │   ├── client.py       # Persistent WS connection to Mopidy
│   │   ├── cache.py        # FTS5 indexing, Tidal TTL cache
│   │   └── models.py       # Track, Album, Artist types
│   ├── search/
│   │   └── router.py       # GET /api/search (cache-first)
│   ├── library/
│   │   └── router.py       # Browse + lookup endpoints
│   ├── playlists/
│   │   ├── router.py       # Playlist CRUD
│   │   └── service.py      # Playlist business logic
│   ├── queue/
│   │   └── router.py       # Queue operations (batched)
│   ├── playback/
│   │   └── router.py       # Playback control endpoints
│   ├── ws/
│   │   └── handler.py      # Browser WebSocket handler
│   └── migrations/
│       └── 001_initial.sql # Schema creation
```

### 2.8 Key Dependencies

```toml
[project]
dependencies = [
    "fastapi>=0.115",
    "uvicorn[standard]>=0.34",
    "websockets>=14.0",
    "aiosqlite>=0.21",
    "bcrypt>=4.2",
    "pyjwt>=2.10",
    "pydantic>=2.10",
]
```

Minimal. No ORM — raw SQL with aiosqlite. Pydantic for request/response models (comes with FastAPI anyway).

---

## 3. Frontend (SvelteKit + Svelte 5)

### 3.1 Stack

- **Svelte 5** with runes (`$state`, `$derived`, `$effect`)
- **SvelteKit** with `adapter-static` (SPA mode, no SSR)
- **TypeScript** strict mode
- **UnoCSS** — same utility-first approach, same terminal theme
- **Lucide Svelte** — tree-shakeable icons
- **Bun** — dev tooling and static file serving in production

Bundle target: < 50KB gzipped.

### 3.2 Routes

```
/                → QueueView (home — what's playing, up next)
/library         → LibraryView (browse artists → albums → tracks)
/search          → SearchView (instant search, grouped results)
/playlists       → PlaylistsView (list + create)
/playlists/[id]  → PlaylistDetail (tracks + manage)
/login           → Login
/register        → Register
```

### 3.3 State Management

Svelte 5 runes in `.svelte.ts` modules. No external state library.

```
lib/state/
├── player.svelte.ts      # playbackState, currentTrack, volume, position, isPlaying
├── queue.svelte.ts        # queue items, queueVersion
├── connection.svelte.ts   # WebSocket connection status
├── auth.svelte.ts         # currentUser, token, isAuthenticated
├── library.svelte.ts      # libraryItems, libraryPath, loading state
└── search.svelte.ts       # query, results (tracks/albums/artists), loading
```

### 3.4 Services

```
lib/services/
├── ws.svelte.ts           # WebSocket connection, auto-reconnect, state sync
├── api.ts                 # Typed fetch wrapper for REST endpoints
└── auth.ts                # Login, register, token management (localStorage)
```

**WebSocket service:**
- Connects to `ws://backend:8000/ws?token=<jwt>`
- Receives initial state sync on connect (no need for multiple REST calls)
- Incoming Mopidy events update reactive state directly
- Outgoing commands forwarded as JSON-RPC
- Auto-reconnect with backoff
- Exposes `connectionStatus` as reactive state

**API service:**
- Thin typed wrapper around `fetch`
- Adds Authorization header automatically
- Returns typed responses
- Used for search, library browse, playlists, queue batch operations

### 3.5 Components

```
lib/components/
├── Layout.svelte              # Shell: BottomNav + MiniPlayer slot
├── BottomNav.svelte           # Fixed bottom tabs (Queue, Library, Search, Playlists)
├── MiniPlayer.svelte          # Current track + play/pause/next, swipe up to expand
├── TrackItem.svelte           # Track row: title, artist, duration, swipe actions
├── SwipeAction.svelte         # Reusable swipe gesture handler
├── ConfirmDialog.svelte       # Reusable confirmation modal
├── Skeleton.svelte            # Skeleton loader for lists
└── VirtualList.svelte         # Virtual scrolling for long lists (500+ items)
```

### 3.6 Search UX

- **Instant results as you type** for local library (backend FTS5 responds in <5ms)
- **Debounced Tidal search** (300ms delay after typing stops)
- **Grouped results** in a single scrollable view — tracks section, albums section, artists section — no tabs
- **"Add all" button** on album/artist results — sends batch request to backend
- **Recent searches** stored in localStorage

### 3.7 Styling

Same terminal/ncmpcpp aesthetic from current app:

```
Background:  #000000 / #0a0a0a / #141414
Foreground:  #ffffff / #b0b0b0 / #707070
Accent:      #cc0000
Borders:     #333333 / #1a1a1a
Font:        Monospace
```

UnoCSS config carries over with same theme tokens and shortcuts (touch-target, btn, btn-icon, track-item).

Touch targets: 56px for primary actions, 48px minimum for all interactive elements.

### 3.8 Mobile UX

- **Haptic feedback** via `navigator.vibrate()` on destructive/confirmatory actions
- **Pull-to-refresh** on queue and library views
- **Skeleton loaders** instead of spinners during loading
- **Optimistic updates** — add to queue shows immediately, reconciled on Mopidy response
- **Virtual scrolling** for lists with 500+ items
- **Swipe gestures** — left to remove, right to add to queue/playlist
- **Bottom sheet** for track context actions (add to playlist, play next, etc.)

### 3.9 Frontend Project Structure

```
frontend/
├── package.json
├── svelte.config.js
├── vite.config.ts
├── uno.config.ts              # Theme, shortcuts (carried from current)
├── tsconfig.json
├── Dockerfile
├── src/
│   ├── app.html
│   ├── app.css                # UnoCSS entry + base styles
│   ├── lib/
│   │   ├── state/             # Svelte 5 rune-based state modules
│   │   ├── services/          # API client, WebSocket, auth
│   │   ├── components/        # Shared components
│   │   └── types.ts           # Shared TypeScript types
│   └── routes/
│       ├── +layout.svelte     # App shell with Layout component
│       ├── +page.svelte       # Queue (home)
│       ├── library/
│       │   └── +page.svelte
│       ├── search/
│       │   └── +page.svelte
│       ├── playlists/
│       │   ├── +page.svelte
│       │   └── [id]/
│       │       └── +page.svelte
│       ├── login/
│       │   └── +page.svelte
│       └── register/
│           └── +page.svelte
```

---

## 4. Infrastructure & Docker

### 4.1 Docker Compose

```yaml
services:
  mopidy:
    # Same as current — local files + Tidal
    build: ./mopidy
    ports: ["6680:6680"]
    volumes:
      - ./mopidy/mopidy.conf:/config/mopidy.conf:ro
      - ${MUSIC_LIBRARY_PATH:-./data/music}:/media/music:ro
      - mopidy-local:/var/lib/mopidy/local
      - mopidy-cache:/var/cache/mopidy

  backend:
    build: ./backend
    ports: ["${BACKEND_PORT:-8000}:8000"]
    volumes:
      - backend-data:/app/data
    environment:
      - JWT_SECRET=${JWT_SECRET}
      - MOPIDY_WS_URL=${MOPIDY_WS_URL:-ws://mopidy:6680/mopidy/ws}
      - LIBRARY_RESCAN_INTERVAL=${LIBRARY_RESCAN_INTERVAL:-1800}
    depends_on:
      mopidy:
        condition: service_healthy

  frontend:
    build: ./frontend
    ports: ["${FRONTEND_PORT:-3000}:3000"]
    depends_on:
      - backend

volumes:
  mopidy-local:
  mopidy-cache:
  backend-data:
```

### 4.2 Backend Dockerfile

```dockerfile
FROM python:3.13-slim
COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv
WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev
COPY app/ app/
CMD ["uv", "run", "uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
```

### 4.3 Frontend Dockerfile

```dockerfile
FROM oven/bun:latest AS build
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY . .
RUN bun run build

FROM oven/bun:latest
WORKDIR /app
COPY --from=build /app/build ./build
COPY --from=build /app/package.json .
CMD ["bun", "run", "--bun", "build/index.js"]
```

SvelteKit builds to static files via adapter-static. Bun serves the static output directory.

### 4.4 Environment Variables

```bash
# Required
JWT_SECRET=your-secret-here

# Backend
MOPIDY_WS_URL=ws://mopidy:6680/mopidy/ws   # default
BACKEND_PORT=8000                             # default
LIBRARY_RESCAN_INTERVAL=1800                  # seconds, default 30min

# Frontend (build-time)
VITE_BACKEND_HTTP_URL=http://localhost:8000
VITE_BACKEND_WS_URL=ws://localhost:8000/ws

# Mopidy
MUSIC_LIBRARY_PATH=./data/music

# Ports
FRONTEND_PORT=3000
```

### 4.5 Dev Workflow

```bash
# Backend: hot reload
cd backend && uv run uvicorn app.main:app --reload

# Frontend: Vite HMR
cd frontend && bun run dev

# Full stack
docker compose up

# Rebuild single service
docker compose up -d --build backend
```

---

## 5. Migration Path

This is a full rewrite, not a migration. The old Gleam backend and Preact frontend are replaced entirely.

**What carries over:**
- Mopidy Docker config (unchanged)
- UnoCSS theme config (colors, shortcuts, touch targets)
- The terminal aesthetic and mobile-first UX patterns
- docker-compose structure (3 services)
- `.env` structure (mostly same variables)

**What's deleted:**
- `backend/` (entire Gleam codebase)
- `frontend/` (entire Preact codebase)
- Both replaced with new directories

**Data migration:**
- User accounts: Can export/import from old SQLite if needed, but for a clubroom app it's simpler to start fresh
- Playlists: Same — start fresh or write a one-time migration script
- No analytics data to preserve (we're dropping ML tables)
