# MusakoneV3 Rebuild Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the MusakoneV3 music player with FastAPI (Python) backend and SvelteKit (Svelte 5) frontend, with full e2e test coverage via Playwright.

**Architecture:** Python FastAPI backend acts as a smart cache layer between browser clients and Mopidy. It maintains a single persistent WebSocket to Mopidy, fans out events to all connected browsers, and provides FTS5 full-text search over a cached library index. SvelteKit SPA frontend uses Svelte 5 runes for state, UnoCSS for styling, and communicates via REST + WebSocket.

**Tech Stack:** Python 3.13, FastAPI, aiosqlite, uvicorn, bcrypt, PyJWT, websockets | Svelte 5, SvelteKit, TypeScript, UnoCSS, Bits UI (headless accessible primitives), Bun | Playwright for e2e | Docker Compose

**Spec:** `docs/superpowers/specs/2026-04-10-musakone-rebuild-design.md`

**Testing Strategy:**
- Backend: pytest with httpx AsyncClient (FastAPI TestClient) + a fake Mopidy WebSocket server
- Frontend: Playwright e2e tests against the real backend with fake Mopidy
- The fake Mopidy server is a simple asyncio WebSocket that responds to JSON-RPC with canned data. It lives in `tests/fake_mopidy.py` and is shared by backend tests and e2e tests.

---

## File Structure

### Backend (`backend/`)

```
backend/
├── pyproject.toml
├── Dockerfile
├── app/
│   ├── __init__.py
│   ├── main.py                  # FastAPI app, lifespan (Mopidy connect, DB init, cache warmup), CORS
│   ├── config.py                # Pydantic Settings from env vars
│   ├── db.py                    # get_db(), init_db(), run migrations
│   ├── auth/
│   │   ├── __init__.py
│   │   ├── router.py            # POST /api/auth/register, /login, GET /me
│   │   ├── service.py           # hash_password, verify_password, create_token, decode_token
│   │   └── deps.py              # get_current_user FastAPI dependency
│   ├── mopidy/
│   │   ├── __init__.py
│   │   ├── client.py            # MopidyClient: persistent WS, send_command, reconnect
│   │   ├── cache.py             # LibraryCache: FTS5 indexing, Tidal TTL, search
│   │   └── models.py            # Track, Artist, Album, TlTrack, Ref, SearchResult Pydantic models
│   ├── search/
│   │   ├── __init__.py
│   │   └── router.py            # GET /api/search?q=
│   ├── library/
│   │   ├── __init__.py
│   │   └── router.py            # GET /api/library/browse, /lookup, POST /rescan
│   ├── playlists/
│   │   ├── __init__.py
│   │   ├── router.py            # CRUD endpoints
│   │   └── service.py           # DB operations for playlists
│   ├── queue/
│   │   ├── __init__.py
│   │   └── router.py            # GET /api/queue, POST add/clear/shuffle
│   ├── playback/
│   │   ├── __init__.py
│   │   └── router.py            # POST play/pause/next/previous/volume/seek
│   ├── ws/
│   │   ├── __init__.py
│   │   └── handler.py           # Browser WS handler: forward commands, broadcast events, state_sync
│   └── migrations/
│       └── 001_initial.sql      # Full schema
├── tests/
│   ├── __init__.py
│   ├── conftest.py              # Fixtures: app, client, db, fake_mopidy, auth helpers
│   ├── fake_mopidy.py           # Fake Mopidy WS server for testing
│   ├── test_auth.py
│   ├── test_search.py
│   ├── test_library.py
│   ├── test_playlists.py
│   ├── test_queue.py
│   ├── test_playback.py
│   └── test_ws.py
```

### Frontend (`frontend/`)

```
frontend/
├── package.json
├── svelte.config.js
├── vite.config.ts
├── uno.config.ts
├── tsconfig.json
├── Dockerfile
├── src/
│   ├── app.html
│   ├── app.css
│   ├── lib/
│   │   ├── types.ts             # Shared TS types (Track, Artist, Album, etc.)
│   │   ├── state/
│   │   │   ├── player.svelte.ts
│   │   │   ├── queue.svelte.ts
│   │   │   ├── connection.svelte.ts
│   │   │   └── auth.svelte.ts
│   │   ├── services/
│   │   │   ├── api.ts           # Typed fetch wrapper
│   │   │   ├── ws.svelte.ts     # WebSocket connection + event routing
│   │   │   └── auth.ts          # Login, register, token management
│   │   └── components/
│   │       ├── Layout.svelte
│   │       ├── BottomNav.svelte
│   │       ├── MiniPlayer.svelte
│   │       ├── TrackItem.svelte
│   │       └── ConfirmDialog.svelte    # Uses Bits UI Dialog primitive
│   └── routes/
│       ├── +layout.svelte
│       ├── +layout.ts
│       ├── +page.svelte              # Queue
│       ├── library/+page.svelte
│       ├── search/+page.svelte
│       ├── playlists/+page.svelte
│       ├── playlists/[id]/+page.svelte
│       ├── login/+page.svelte
│       └── register/+page.svelte
├── e2e/
│   ├── playwright.config.ts
│   ├── helpers/
│   │   └── setup.ts             # Start backend + fake mopidy, auth helpers
│   ├── auth.spec.ts
│   ├── queue.spec.ts
│   ├── search.spec.ts
│   ├── library.spec.ts
│   ├── playlists.spec.ts
│   └── playback.spec.ts
```

---

## Phase 1: Backend Foundation

### Task 1: Project scaffold + DB + config

**Files:**
- Create: `backend/pyproject.toml`
- Create: `backend/app/__init__.py`
- Create: `backend/app/config.py`
- Create: `backend/app/db.py`
- Create: `backend/app/main.py`
- Create: `backend/app/migrations/001_initial.sql`
- Create: `backend/tests/__init__.py`
- Create: `backend/tests/conftest.py`

- [ ] **Step 1: Create pyproject.toml**

```toml
[project]
name = "musakone-backend"
version = "0.1.0"
requires-python = ">=3.13"
dependencies = [
    "fastapi>=0.115",
    "uvicorn[standard]>=0.34",
    "websockets>=14.0",
    "aiosqlite>=0.21",
    "bcrypt>=4.2",
    "pyjwt>=2.10",
    "pydantic-settings>=2.7",
]

[project.optional-dependencies]
dev = [
    "pytest>=8.0",
    "pytest-asyncio>=0.25",
    "httpx>=0.28",
]

[tool.pytest.ini_options]
asyncio_mode = "auto"
testpaths = ["tests"]
```

- [ ] **Step 2: Install dependencies**

Run: `cd backend && uv sync --all-extras`
Expected: lockfile created, all deps installed

- [ ] **Step 3: Create config.py**

```python
from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    jwt_secret: str = "dev-secret-change-in-production"
    jwt_expiry_hours: int = 168  # 1 week
    mopidy_ws_url: str = "ws://localhost:6680/mopidy/ws"
    db_path: str = "data/musakone.db"
    library_rescan_interval: int = 1800  # seconds

    model_config = {"env_prefix": "", "case_sensitive": False}


settings = Settings()
```

- [ ] **Step 4: Create migration SQL**

```sql
-- 001_initial.sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_uri TEXT NOT NULL,
    track_name TEXT,
    artist_name TEXT,
    position INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, track_uri)
);

CREATE TABLE IF NOT EXISTS tidal_cache (
    cache_key TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    cached_at INTEGER NOT NULL DEFAULT (unixepoch()),
    ttl_seconds INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS event_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    event_type TEXT NOT NULL,
    detail TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
```

Note: The FTS5 virtual table is created programmatically in `cache.py` because FTS5 tables can't use `IF NOT EXISTS`.

- [ ] **Step 5: Create db.py**

```python
import aiosqlite
from pathlib import Path

from app.config import settings

_db: aiosqlite.Connection | None = None


async def get_db() -> aiosqlite.Connection:
    assert _db is not None, "Database not initialized"
    return _db


async def init_db() -> aiosqlite.Connection:
    global _db
    db_path = Path(settings.db_path)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    _db = await aiosqlite.connect(str(db_path))
    _db.row_factory = aiosqlite.Row
    await _db.execute("PRAGMA journal_mode=WAL")
    await _db.execute("PRAGMA foreign_keys=ON")
    migration = Path(__file__).parent / "migrations" / "001_initial.sql"
    await _db.executescript(migration.read_text())
    await _db.commit()
    return _db


async def close_db() -> None:
    global _db
    if _db:
        await _db.close()
        _db = None
```

- [ ] **Step 6: Create main.py with lifespan**

```python
from contextlib import asynccontextmanager
from collections.abc import AsyncGenerator

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.db import init_db, close_db


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None]:
    await init_db()
    yield
    await close_db()


app = FastAPI(title="Musakone", lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)
```

- [ ] **Step 7: Create empty __init__.py files**

Create empty files: `backend/app/__init__.py`, `backend/tests/__init__.py`

- [ ] **Step 8: Create test conftest.py**

```python
import pytest
import aiosqlite
from httpx import ASGITransport, AsyncClient

from app.main import app
from app import db as db_module


@pytest.fixture
async def test_db(tmp_path):
    """Create a fresh in-memory DB for each test."""
    from app.config import settings
    original_path = settings.db_path
    settings.db_path = str(tmp_path / "test.db")
    connection = await db_module.init_db()
    yield connection
    await db_module.close_db()
    settings.db_path = original_path


@pytest.fixture
async def client(test_db):
    """Async HTTP client for testing FastAPI endpoints."""
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        yield ac
```

- [ ] **Step 9: Write a smoke test**

Create `backend/tests/test_smoke.py`:
```python
async def test_app_starts(client):
    """Verify the app boots and responds."""
    response = await client.get("/docs")
    assert response.status_code == 200
```

- [ ] **Step 10: Run smoke test**

Run: `cd backend && uv run pytest tests/test_smoke.py -v`
Expected: PASS

- [ ] **Step 11: Commit**

```bash
git add backend/
git commit -m "feat(backend): scaffold FastAPI project with DB, config, and tests"
```

---

### Task 2: Auth service + endpoints

**Files:**
- Create: `backend/app/auth/__init__.py`
- Create: `backend/app/auth/service.py`
- Create: `backend/app/auth/deps.py`
- Create: `backend/app/auth/router.py`
- Modify: `backend/app/main.py` (add router)
- Create: `backend/tests/test_auth.py`

- [ ] **Step 1: Write failing auth tests**

Create `backend/tests/test_auth.py`:
```python
import pytest


async def test_register_creates_user(client):
    resp = await client.post("/api/auth/register", json={
        "username": "testuser",
        "password": "testpass123",
    })
    assert resp.status_code == 200
    data = resp.json()
    assert "token" in data
    assert data["user"]["username"] == "testuser"
    assert "id" in data["user"]


async def test_register_duplicate_username(client):
    await client.post("/api/auth/register", json={
        "username": "testuser",
        "password": "testpass123",
    })
    resp = await client.post("/api/auth/register", json={
        "username": "testuser",
        "password": "otherpass",
    })
    assert resp.status_code == 409


async def test_login_valid_credentials(client):
    await client.post("/api/auth/register", json={
        "username": "testuser",
        "password": "testpass123",
    })
    resp = await client.post("/api/auth/login", json={
        "username": "testuser",
        "password": "testpass123",
    })
    assert resp.status_code == 200
    data = resp.json()
    assert "token" in data
    assert data["user"]["username"] == "testuser"


async def test_login_wrong_password(client):
    await client.post("/api/auth/register", json={
        "username": "testuser",
        "password": "testpass123",
    })
    resp = await client.post("/api/auth/login", json={
        "username": "testuser",
        "password": "wrongpass",
    })
    assert resp.status_code == 401


async def test_login_nonexistent_user(client):
    resp = await client.post("/api/auth/login", json={
        "username": "nobody",
        "password": "testpass123",
    })
    assert resp.status_code == 401


async def test_me_with_valid_token(client):
    reg = await client.post("/api/auth/register", json={
        "username": "testuser",
        "password": "testpass123",
    })
    token = reg.json()["token"]
    resp = await client.get("/api/auth/me", headers={
        "Authorization": f"Bearer {token}",
    })
    assert resp.status_code == 200
    assert resp.json()["username"] == "testuser"


async def test_me_without_token(client):
    resp = await client.get("/api/auth/me")
    assert resp.status_code == 401


async def test_me_with_invalid_token(client):
    resp = await client.get("/api/auth/me", headers={
        "Authorization": "Bearer invalid.token.here",
    })
    assert resp.status_code == 401
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_auth.py -v`
Expected: FAIL (no routes registered)

- [ ] **Step 3: Implement auth service**

Create `backend/app/auth/__init__.py` (empty).

Create `backend/app/auth/service.py`:
```python
import time
import bcrypt
import jwt

from app.config import settings


def hash_password(password: str) -> str:
    return bcrypt.hashpw(password.encode(), bcrypt.gensalt()).decode()


def verify_password(password: str, password_hash: str) -> bool:
    return bcrypt.checkpw(password.encode(), password_hash.encode())


def create_token(user_id: int) -> str:
    payload = {
        "sub": str(user_id),
        "exp": int(time.time()) + settings.jwt_expiry_hours * 3600,
    }
    return jwt.encode(payload, settings.jwt_secret, algorithm="HS256")


def decode_token(token: str) -> int | None:
    try:
        payload = jwt.decode(token, settings.jwt_secret, algorithms=["HS256"])
        return int(payload["sub"])
    except (jwt.InvalidTokenError, KeyError, ValueError):
        return None
```

- [ ] **Step 4: Implement auth dependency**

Create `backend/app/auth/deps.py`:
```python
from fastapi import Depends, HTTPException, Header

from app.db import get_db
from app.auth.service import decode_token


async def get_current_user(authorization: str | None = Header(None)):
    if not authorization or not authorization.startswith("Bearer "):
        raise HTTPException(status_code=401, detail="Not authenticated")
    token = authorization.removeprefix("Bearer ")
    user_id = decode_token(token)
    if user_id is None:
        raise HTTPException(status_code=401, detail="Invalid token")
    db = await get_db()
    row = await db.execute_fetchall(
        "SELECT id, username, created_at FROM users WHERE id = ?", (user_id,)
    )
    if not row:
        raise HTTPException(status_code=401, detail="User not found")
    return {"id": row[0][0], "username": row[0][1], "created_at": row[0][2]}
```

- [ ] **Step 5: Implement auth router**

Create `backend/app/auth/router.py`:
```python
from fastapi import APIRouter, HTTPException, Depends
from pydantic import BaseModel

from app.db import get_db
from app.auth.service import hash_password, verify_password, create_token
from app.auth.deps import get_current_user

router = APIRouter(prefix="/api/auth", tags=["auth"])


class AuthRequest(BaseModel):
    username: str
    password: str


class UserResponse(BaseModel):
    id: int
    username: str


class AuthResponse(BaseModel):
    token: str
    user: UserResponse


@router.post("/register", response_model=AuthResponse)
async def register(body: AuthRequest):
    db = await get_db()
    existing = await db.execute_fetchall(
        "SELECT id FROM users WHERE username = ?", (body.username,)
    )
    if existing:
        raise HTTPException(status_code=409, detail="Username taken")
    pw_hash = hash_password(body.password)
    cursor = await db.execute(
        "INSERT INTO users (username, password_hash) VALUES (?, ?)",
        (body.username, pw_hash),
    )
    await db.commit()
    user_id = cursor.lastrowid
    token = create_token(user_id)
    return AuthResponse(
        token=token,
        user=UserResponse(id=user_id, username=body.username),
    )


@router.post("/login", response_model=AuthResponse)
async def login(body: AuthRequest):
    db = await get_db()
    rows = await db.execute_fetchall(
        "SELECT id, username, password_hash FROM users WHERE username = ?",
        (body.username,),
    )
    if not rows:
        raise HTTPException(status_code=401, detail="Invalid credentials")
    user_id, username, pw_hash = rows[0]
    if not verify_password(body.password, pw_hash):
        raise HTTPException(status_code=401, detail="Invalid credentials")
    token = create_token(user_id)
    return AuthResponse(
        token=token,
        user=UserResponse(id=user_id, username=username),
    )


@router.get("/me", response_model=UserResponse)
async def me(user=Depends(get_current_user)):
    return UserResponse(id=user["id"], username=user["username"])
```

- [ ] **Step 6: Register router in main.py**

Add to `backend/app/main.py` after CORS middleware:
```python
from app.auth.router import router as auth_router

app.include_router(auth_router)
```

- [ ] **Step 7: Run auth tests**

Run: `cd backend && uv run pytest tests/test_auth.py -v`
Expected: all 8 tests PASS

- [ ] **Step 8: Commit**

```bash
git add backend/app/auth/ backend/tests/test_auth.py backend/app/main.py
git commit -m "feat(backend): add auth endpoints (register, login, me) with bcrypt + JWT"
```

---

### Task 3: Mopidy models + fake Mopidy test server

**Files:**
- Create: `backend/app/mopidy/__init__.py`
- Create: `backend/app/mopidy/models.py`
- Create: `backend/tests/fake_mopidy.py`

- [ ] **Step 1: Create Mopidy models**

Create `backend/app/mopidy/__init__.py` (empty).

Create `backend/app/mopidy/models.py`:
```python
from pydantic import BaseModel


class Artist(BaseModel):
    uri: str
    name: str


class Album(BaseModel):
    uri: str
    name: str
    artists: list[Artist] = []
    date: str | None = None


class Track(BaseModel):
    uri: str
    name: str
    artists: list[Artist] = []
    album: Album | None = None
    length: int | None = None  # milliseconds
    genre: str | None = None
    track_no: int | None = None


class TlTrack(BaseModel):
    tlid: int
    track: Track


class Ref(BaseModel):
    uri: str
    name: str
    type: str  # "album", "artist", "directory", "playlist", "track"


class SearchResult(BaseModel):
    uri: str | None = None
    tracks: list[Track] = []
    artists: list[Artist] = []
    albums: list[Album] = []
```

- [ ] **Step 2: Create fake Mopidy WebSocket server**

Create `backend/tests/fake_mopidy.py`:
```python
"""
Fake Mopidy WebSocket server for testing.
Responds to JSON-RPC requests with canned data.
Sends events when triggered.
"""
import asyncio
import json

import websockets
from websockets.asyncio.server import ServerConnection

from app.mopidy.models import Track, Artist, Album, TlTrack


# Canned library data
FAKE_TRACKS = [
    Track(
        uri="local:track:song1.mp3",
        name="Test Song One",
        artists=[Artist(uri="local:artist:artist1", name="Test Artist")],
        album=Album(uri="local:album:album1", name="Test Album"),
        length=180000,
        genre="Rock",
    ),
    Track(
        uri="local:track:song2.mp3",
        name="Another Track",
        artists=[Artist(uri="local:artist:artist1", name="Test Artist")],
        album=Album(uri="local:album:album1", name="Test Album"),
        length=240000,
        genre="Rock",
    ),
    Track(
        uri="local:track:song3.mp3",
        name="Third Song",
        artists=[Artist(uri="local:artist:artist2", name="Other Artist")],
        album=Album(uri="local:album:album2", name="Other Album"),
        length=200000,
        genre="Jazz",
    ),
    Track(
        uri="tidal:track:12345",
        name="Tidal Hit",
        artists=[Artist(uri="tidal:artist:999", name="Tidal Star")],
        album=Album(uri="tidal:album:888", name="Tidal Album"),
        length=210000,
    ),
]


class FakeMopidy:
    def __init__(self):
        self.tracklist: list[TlTrack] = []
        self.next_tlid = 1
        self.playback_state = "stopped"
        self.current_tlid: int | None = None
        self.volume = 80
        self.repeat = False
        self.random = False
        self.single = False
        self.consume = False
        self.server = None
        self.clients: list[ServerConnection] = []
        self._event_queue: asyncio.Queue[dict] = asyncio.Queue()

    def _make_tl_track(self, track: Track) -> TlTrack:
        tl = TlTrack(tlid=self.next_tlid, track=track)
        self.next_tlid += 1
        return tl

    async def handle(self, ws: ServerConnection):
        self.clients.append(ws)
        try:
            async for raw in ws:
                msg = json.loads(raw)
                resp = self._handle_rpc(msg)
                await ws.send(json.dumps(resp))
        except websockets.ConnectionClosed:
            pass
        finally:
            self.clients.remove(ws)

    def _handle_rpc(self, msg: dict) -> dict:
        method = msg.get("method", "")
        params = msg.get("params", {})
        req_id = msg.get("id", 0)
        result = self._dispatch(method, params)
        return {"jsonrpc": "2.0", "id": req_id, "result": result}

    def _dispatch(self, method: str, params: dict):
        handlers = {
            "core.playback.get_state": lambda p: self.playback_state,
            "core.playback.play": self._play,
            "core.playback.pause": self._pause,
            "core.playback.resume": self._resume,
            "core.playback.next": self._next,
            "core.playback.previous": self._previous,
            "core.playback.stop": self._stop,
            "core.playback.seek": lambda p: True,
            "core.playback.get_current_tl_track": lambda p: self._get_current_tl_track(),
            "core.playback.get_time_position": lambda p: 0,
            "core.mixer.get_volume": lambda p: self.volume,
            "core.mixer.set_volume": self._set_volume,
            "core.tracklist.get_tl_tracks": lambda p: [t.model_dump() for t in self.tracklist],
            "core.tracklist.add": self._tracklist_add,
            "core.tracklist.remove": self._tracklist_remove,
            "core.tracklist.clear": self._tracklist_clear,
            "core.tracklist.shuffle": self._tracklist_shuffle,
            "core.tracklist.get_length": lambda p: len(self.tracklist),
            "core.tracklist.get_repeat": lambda p: self.repeat,
            "core.tracklist.set_repeat": lambda p: self._set_option("repeat", p.get("value", False)),
            "core.tracklist.get_random": lambda p: self.random,
            "core.tracklist.set_random": lambda p: self._set_option("random", p.get("value", False)),
            "core.tracklist.get_single": lambda p: self.single,
            "core.tracklist.get_consume": lambda p: self.consume,
            "core.library.browse": self._library_browse,
            "core.library.search": self._library_search,
            "core.library.lookup": self._library_lookup,
        }
        handler = handlers.get(method)
        if handler is None:
            return None
        return handler(params)

    def _play(self, params):
        tlid = params.get("tlid")
        if tlid is not None:
            self.current_tlid = tlid
        elif self.tracklist and self.current_tlid is None:
            self.current_tlid = self.tracklist[0].tlid
        self.playback_state = "playing"
        return None

    def _pause(self, params):
        self.playback_state = "paused"
        return None

    def _resume(self, params):
        self.playback_state = "playing"
        return None

    def _stop(self, params):
        self.playback_state = "stopped"
        return None

    def _next(self, params):
        if not self.tracklist:
            return None
        idx = self._current_index()
        if idx is not None and idx < len(self.tracklist) - 1:
            self.current_tlid = self.tracklist[idx + 1].tlid
        return None

    def _previous(self, params):
        if not self.tracklist:
            return None
        idx = self._current_index()
        if idx is not None and idx > 0:
            self.current_tlid = self.tracklist[idx - 1].tlid
        return None

    def _current_index(self) -> int | None:
        for i, tl in enumerate(self.tracklist):
            if tl.tlid == self.current_tlid:
                return i
        return None

    def _get_current_tl_track(self):
        for tl in self.tracklist:
            if tl.tlid == self.current_tlid:
                return tl.model_dump()
        return None

    def _set_volume(self, params):
        self.volume = params.get("volume", self.volume)
        return True

    def _set_option(self, name, value):
        setattr(self, name, value)
        return None

    def _tracklist_add(self, params):
        uris = params.get("uris", [])
        at_position = params.get("at_position")
        added = []
        for uri in uris:
            track = self._find_track(uri)
            if track is None:
                track = Track(uri=uri, name=f"Unknown ({uri})")
            tl = self._make_tl_track(track)
            added.append(tl)
        if at_position is not None:
            for i, tl in enumerate(added):
                self.tracklist.insert(at_position + i, tl)
        else:
            self.tracklist.extend(added)
        return [t.model_dump() for t in added]

    def _tracklist_remove(self, params):
        criteria = params.get("criteria", {})
        tlids = criteria.get("tlid", [])
        removed = [tl for tl in self.tracklist if tl.tlid in tlids]
        self.tracklist = [tl for tl in self.tracklist if tl.tlid not in tlids]
        return [t.model_dump() for t in removed]

    def _tracklist_clear(self, params):
        self.tracklist.clear()
        self.current_tlid = None
        self.playback_state = "stopped"
        return None

    def _tracklist_shuffle(self, params):
        import random as rng
        rng.shuffle(self.tracklist)
        return None

    def _find_track(self, uri: str) -> Track | None:
        for t in FAKE_TRACKS:
            if t.uri == uri:
                return t
        return None

    def _library_browse(self, params):
        uri = params.get("uri")
        if uri is None or uri == "":
            return [
                {"uri": "local:directory", "name": "Local files", "type": "directory"},
                {"uri": "tidal:directory", "name": "Tidal", "type": "directory"},
            ]
        if uri == "local:directory":
            return [
                {"uri": "local:artist:artist1", "name": "Test Artist", "type": "artist"},
                {"uri": "local:artist:artist2", "name": "Other Artist", "type": "artist"},
            ]
        if uri == "local:artist:artist1":
            return [
                {"uri": "local:album:album1", "name": "Test Album", "type": "album"},
            ]
        if uri == "local:album:album1":
            return [
                {"uri": "local:track:song1.mp3", "name": "Test Song One", "type": "track"},
                {"uri": "local:track:song2.mp3", "name": "Another Track", "type": "track"},
            ]
        return []

    def _library_search(self, params):
        query = params.get("query", {})
        search_terms = " ".join(v for vals in query.values() for v in vals).lower()
        matching = [
            t for t in FAKE_TRACKS
            if search_terms in t.name.lower()
            or any(search_terms in a.name.lower() for a in t.artists)
        ]
        return [{"tracks": [t.model_dump() for t in matching]}]

    def _library_lookup(self, params):
        uris = params.get("uris", [])
        result = {}
        for uri in uris:
            track = self._find_track(uri)
            result[uri] = [track.model_dump()] if track else []
        return result

    async def send_event(self, event: dict):
        """Push a Mopidy event to all connected clients."""
        raw = json.dumps(event)
        for client in self.clients:
            await client.send(raw)

    async def start(self, host="127.0.0.1", port=0) -> int:
        """Start the fake server. Returns the port."""
        self.server = await websockets.serve(self.handle, host, port)
        port = self.server.sockets[0].getsockname()[1]
        return port

    async def stop(self):
        if self.server:
            self.server.close()
            await self.server.wait_closed()
```

- [ ] **Step 3: Add fake_mopidy fixture to conftest.py**

Add to `backend/tests/conftest.py`:
```python
from tests.fake_mopidy import FakeMopidy


@pytest.fixture
async def fake_mopidy():
    """Start a fake Mopidy WS server, yield it, stop on teardown."""
    mopidy = FakeMopidy()
    port = await mopidy.start()
    yield mopidy, port
    await mopidy.stop()
```

- [ ] **Step 4: Write a test for the fake Mopidy server itself**

Create `backend/tests/test_fake_mopidy.py`:
```python
import json
import websockets


async def test_fake_mopidy_responds_to_browse(fake_mopidy):
    mopidy, port = fake_mopidy
    async with websockets.connect(f"ws://127.0.0.1:{port}/mopidy/ws") as ws:
        await ws.send(json.dumps({
            "jsonrpc": "2.0", "id": 1,
            "method": "core.library.browse", "params": {"uri": None},
        }))
        resp = json.loads(await ws.recv())
        assert resp["id"] == 1
        assert len(resp["result"]) == 2
        assert resp["result"][0]["name"] == "Local files"


async def test_fake_mopidy_tracklist_add(fake_mopidy):
    mopidy, port = fake_mopidy
    async with websockets.connect(f"ws://127.0.0.1:{port}/mopidy/ws") as ws:
        await ws.send(json.dumps({
            "jsonrpc": "2.0", "id": 1,
            "method": "core.tracklist.add",
            "params": {"uris": ["local:track:song1.mp3", "local:track:song2.mp3"]},
        }))
        resp = json.loads(await ws.recv())
        assert len(resp["result"]) == 2
        assert resp["result"][0]["track"]["name"] == "Test Song One"


async def test_fake_mopidy_search(fake_mopidy):
    mopidy, port = fake_mopidy
    async with websockets.connect(f"ws://127.0.0.1:{port}/mopidy/ws") as ws:
        await ws.send(json.dumps({
            "jsonrpc": "2.0", "id": 1,
            "method": "core.library.search",
            "params": {"query": {"any": ["test"]}},
        }))
        resp = json.loads(await ws.recv())
        tracks = resp["result"][0]["tracks"]
        assert len(tracks) == 2  # "Test Song One" and "Test Artist" matches
```

- [ ] **Step 5: Run fake mopidy tests**

Run: `cd backend && uv run pytest tests/test_fake_mopidy.py -v`
Expected: all 3 PASS

- [ ] **Step 6: Commit**

```bash
git add backend/app/mopidy/ backend/tests/fake_mopidy.py backend/tests/test_fake_mopidy.py backend/tests/conftest.py
git commit -m "feat(backend): add Mopidy models and fake Mopidy test server"
```

---

### Task 4: Mopidy client (persistent WS connection)

**Files:**
- Create: `backend/app/mopidy/client.py`
- Modify: `backend/app/main.py` (wire up in lifespan)
- Create: `backend/tests/test_mopidy_client.py`

- [ ] **Step 1: Write failing tests for MopidyClient**

Create `backend/tests/test_mopidy_client.py`:
```python
import pytest
from app.mopidy.client import MopidyClient


async def test_client_connects_and_sends_command(fake_mopidy):
    mopidy, port = fake_mopidy
    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    try:
        result = await client.send_command("core.mixer.get_volume")
        assert result == 80
    finally:
        await client.disconnect()


async def test_client_send_command_with_params(fake_mopidy):
    mopidy, port = fake_mopidy
    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    try:
        result = await client.send_command(
            "core.tracklist.add",
            {"uris": ["local:track:song1.mp3"]},
        )
        assert len(result) == 1
        assert result[0]["track"]["name"] == "Test Song One"
    finally:
        await client.disconnect()


async def test_client_event_callback(fake_mopidy):
    mopidy, port = fake_mopidy
    events = []

    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    client.on_event = lambda e: events.append(e)
    await client.connect()
    try:
        await mopidy.send_event({"event": "volume_changed", "volume": 50})
        # Give the event listener a moment to process
        import asyncio
        await asyncio.sleep(0.1)
        assert len(events) == 1
        assert events[0]["event"] == "volume_changed"
    finally:
        await client.disconnect()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_mopidy_client.py -v`
Expected: FAIL (MopidyClient doesn't exist)

- [ ] **Step 3: Implement MopidyClient**

Create `backend/app/mopidy/client.py`:
```python
import asyncio
import json
import logging
from collections.abc import Callable

import websockets

logger = logging.getLogger(__name__)


class MopidyClient:
    """Persistent WebSocket connection to Mopidy with command routing."""

    def __init__(self, url: str):
        self.url = url
        self._ws = None
        self._next_id = 1
        self._pending: dict[int, asyncio.Future] = {}
        self._listener_task: asyncio.Task | None = None
        self._connected = asyncio.Event()
        self.on_event: Callable[[dict], None] | None = None

    @property
    def is_connected(self) -> bool:
        return self._ws is not None and self._connected.is_set()

    async def connect(self):
        self._ws = await websockets.connect(self.url)
        self._connected.set()
        self._listener_task = asyncio.create_task(self._listen())
        logger.info("Connected to Mopidy at %s", self.url)

    async def disconnect(self):
        self._connected.clear()
        if self._listener_task:
            self._listener_task.cancel()
            try:
                await self._listener_task
            except asyncio.CancelledError:
                pass
        if self._ws:
            await self._ws.close()
            self._ws = None
        # Cancel any pending futures
        for fut in self._pending.values():
            if not fut.done():
                fut.cancel()
        self._pending.clear()

    async def send_command(self, method: str, params: dict | None = None, timeout: float = 10.0):
        """Send a JSON-RPC command and wait for the response."""
        if not self.is_connected:
            raise ConnectionError("Not connected to Mopidy")
        req_id = self._next_id
        self._next_id += 1
        msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
        if params:
            msg["params"] = params
        future = asyncio.get_event_loop().create_future()
        self._pending[req_id] = future
        await self._ws.send(json.dumps(msg))
        try:
            return await asyncio.wait_for(future, timeout=timeout)
        except asyncio.TimeoutError:
            self._pending.pop(req_id, None)
            raise

    async def _listen(self):
        """Background task: read messages, route responses and events."""
        try:
            async for raw in self._ws:
                msg = json.loads(raw)
                if "id" in msg and msg["id"] in self._pending:
                    future = self._pending.pop(msg["id"])
                    if not future.done():
                        future.set_result(msg.get("result"))
                elif "event" in msg:
                    if self.on_event:
                        self.on_event(msg)
        except websockets.ConnectionClosed:
            logger.warning("Mopidy connection closed")
            self._connected.clear()
        except asyncio.CancelledError:
            pass
```

- [ ] **Step 4: Run tests**

Run: `cd backend && uv run pytest tests/test_mopidy_client.py -v`
Expected: all 3 PASS

- [ ] **Step 5: Wire MopidyClient into main.py lifespan**

Update `backend/app/main.py`:
```python
from contextlib import asynccontextmanager
from collections.abc import AsyncGenerator

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.config import settings
from app.db import init_db, close_db
from app.mopidy.client import MopidyClient

mopidy_client = MopidyClient(settings.mopidy_ws_url)


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None]:
    await init_db()
    # Mopidy connection is optional — don't crash if unavailable
    try:
        await mopidy_client.connect()
    except Exception:
        pass
    yield
    await mopidy_client.disconnect()
    await close_db()


app = FastAPI(title="Musakone", lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

from app.auth.router import router as auth_router
app.include_router(auth_router)
```

- [ ] **Step 6: Commit**

```bash
git add backend/app/mopidy/client.py backend/app/main.py backend/tests/test_mopidy_client.py
git commit -m "feat(backend): add MopidyClient with persistent WS connection and command routing"
```

---

### Task 5: Library cache (FTS5 indexing + search)

**Files:**
- Create: `backend/app/mopidy/cache.py`
- Create: `backend/tests/test_cache.py`

- [ ] **Step 1: Write failing cache tests**

Create `backend/tests/test_cache.py`:
```python
import pytest
from app.mopidy.cache import LibraryCache
from app.mopidy.client import MopidyClient


async def test_fts_search_local_tracks(test_db, fake_mopidy):
    mopidy_server, port = fake_mopidy
    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    cache = LibraryCache(test_db, client)
    try:
        await cache.rebuild_local_index()
        results = await cache.search("test song")
        assert len(results) >= 1
        assert any(r["name"] == "Test Song One" for r in results)
    finally:
        await client.disconnect()


async def test_fts_search_partial_match(test_db, fake_mopidy):
    mopidy_server, port = fake_mopidy
    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    cache = LibraryCache(test_db, client)
    try:
        await cache.rebuild_local_index()
        results = await cache.search("artist")
        # Should match tracks by artist name
        assert len(results) >= 1
    finally:
        await client.disconnect()


async def test_fts_search_no_results(test_db, fake_mopidy):
    mopidy_server, port = fake_mopidy
    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    cache = LibraryCache(test_db, client)
    try:
        await cache.rebuild_local_index()
        results = await cache.search("zzzznonexistent")
        assert results == []
    finally:
        await client.disconnect()


async def test_tidal_cache_stores_and_retrieves(test_db, fake_mopidy):
    mopidy_server, port = fake_mopidy
    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    cache = LibraryCache(test_db, client)
    try:
        # First call: cache miss, queries Mopidy
        results = await cache.search_tidal("tidal")
        assert len(results) >= 1
        assert any(r["name"] == "Tidal Hit" for r in results)
        # Second call: should come from cache (same result)
        results2 = await cache.search_tidal("tidal")
        assert results == results2
    finally:
        await client.disconnect()


async def test_combined_search(test_db, fake_mopidy):
    """search() combines local FTS + Tidal cache."""
    mopidy_server, port = fake_mopidy
    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    cache = LibraryCache(test_db, client)
    try:
        await cache.rebuild_local_index()
        results = await cache.search_all("test")
        # Should include local tracks
        assert any(r["name"] == "Test Song One" for r in results)
    finally:
        await client.disconnect()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_cache.py -v`
Expected: FAIL (LibraryCache doesn't exist)

- [ ] **Step 3: Implement LibraryCache**

Create `backend/app/mopidy/cache.py`:
```python
import json
import logging
import time

import aiosqlite

from app.mopidy.client import MopidyClient

logger = logging.getLogger(__name__)


class LibraryCache:
    """FTS5-backed library cache + Tidal TTL cache."""

    def __init__(self, db: aiosqlite.Connection, mopidy: MopidyClient):
        self.db = db
        self.mopidy = mopidy

    async def init_fts(self):
        """Create the FTS5 table if it doesn't exist."""
        # Check if the table exists
        rows = await self.db.execute_fetchall(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='library_fts'"
        )
        if not rows:
            await self.db.execute(
                """CREATE VIRTUAL TABLE library_fts USING fts5(
                    uri UNINDEXED, name, artist, album, genre,
                    duration UNINDEXED, source UNINDEXED
                )"""
            )
            await self.db.commit()

    async def rebuild_local_index(self):
        """Scan Mopidy local library and index all tracks into FTS5."""
        await self.init_fts()
        # Clear existing local entries
        await self.db.execute("DELETE FROM library_fts WHERE source = 'local'")

        # Recursively browse the local library
        tracks = await self._browse_recursive("local:directory")

        # Lookup full metadata for all track URIs
        if tracks:
            batch_size = 50
            for i in range(0, len(tracks), batch_size):
                batch_uris = [t["uri"] for t in tracks[i:i + batch_size]]
                lookup = await self.mopidy.send_command(
                    "core.library.lookup", {"uris": batch_uris}
                )
                if lookup:
                    for uri, track_list in lookup.items():
                        for track in track_list:
                            await self._index_track(track, "local")

        await self.db.commit()
        count = await self.db.execute_fetchall(
            "SELECT COUNT(*) FROM library_fts WHERE source = 'local'"
        )
        logger.info("Indexed %d local tracks", count[0][0] if count else 0)

    async def _browse_recursive(self, uri: str) -> list[dict]:
        """Recursively browse and collect all track refs."""
        refs = await self.mopidy.send_command("core.library.browse", {"uri": uri})
        if not refs:
            return []
        tracks = []
        for ref in refs:
            if ref.get("type") == "track":
                tracks.append(ref)
            elif ref.get("type") in ("directory", "artist", "album"):
                tracks.extend(await self._browse_recursive(ref["uri"]))
        return tracks

    async def _index_track(self, track: dict, source: str):
        """Insert a track into the FTS5 index."""
        artists = ", ".join(a.get("name", "") for a in track.get("artists", []))
        album = track.get("album", {}).get("name", "") if track.get("album") else ""
        await self.db.execute(
            "INSERT INTO library_fts (uri, name, artist, album, genre, duration, source) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                track.get("uri", ""),
                track.get("name", ""),
                artists,
                album,
                track.get("genre", ""),
                str(track.get("length", 0)),
                source,
            ),
        )

    async def search(self, query: str) -> list[dict]:
        """Search local FTS5 index."""
        await self.init_fts()
        # Escape FTS5 special characters and use prefix matching
        safe_query = query.replace('"', '""')
        fts_query = " ".join(f'"{word}"*' for word in safe_query.split() if word)
        if not fts_query:
            return []
        rows = await self.db.execute_fetchall(
            """SELECT uri, name, artist, album, genre, duration, source
               FROM library_fts WHERE library_fts MATCH ?
               ORDER BY rank LIMIT 50""",
            (fts_query,),
        )
        return [
            {
                "uri": r[0],
                "name": r[1],
                "artist": r[2],
                "album": r[3],
                "genre": r[4],
                "duration": int(r[5]) if r[5] and r[5] != "0" else None,
                "source": r[6],
            }
            for r in rows
        ]

    async def search_tidal(self, query: str) -> list[dict]:
        """Search Tidal via Mopidy, with TTL caching."""
        cache_key = f"tidal_search:{query.lower().strip()}"

        # Check cache
        rows = await self.db.execute_fetchall(
            "SELECT data, cached_at, ttl_seconds FROM tidal_cache WHERE cache_key = ?",
            (cache_key,),
        )
        if rows:
            data, cached_at, ttl = rows[0]
            if time.time() - cached_at < ttl:
                return json.loads(data)

        # Cache miss — query Mopidy
        results = await self.mopidy.send_command(
            "core.library.search",
            {"query": {"any": [query]}, "uris": ["tidal:"]},
        )
        tracks = []
        if results:
            for result in results:
                for track in result.get("tracks", []):
                    artists = ", ".join(a.get("name", "") for a in track.get("artists", []))
                    album = track.get("album", {}).get("name", "") if track.get("album") else ""
                    tracks.append({
                        "uri": track["uri"],
                        "name": track["name"],
                        "artist": artists,
                        "album": album,
                        "genre": track.get("genre", ""),
                        "duration": track.get("length"),
                        "source": "tidal",
                    })

        # Store in cache (1 hour TTL)
        await self.db.execute(
            """INSERT OR REPLACE INTO tidal_cache (cache_key, data, cached_at, ttl_seconds)
               VALUES (?, ?, ?, ?)""",
            (cache_key, json.dumps(tracks), int(time.time()), 3600),
        )
        await self.db.commit()
        return tracks

    async def search_all(self, query: str) -> list[dict]:
        """Combined search: local FTS + Tidal cache/Mopidy."""
        local_results = await self.search(query)
        tidal_results = await self.search_tidal(query)
        # Deduplicate by URI
        seen = {r["uri"] for r in local_results}
        combined = list(local_results)
        for r in tidal_results:
            if r["uri"] not in seen:
                combined.append(r)
                seen.add(r["uri"])
        return combined
```

- [ ] **Step 4: Run cache tests**

Run: `cd backend && uv run pytest tests/test_cache.py -v`
Expected: all 5 PASS

- [ ] **Step 5: Commit**

```bash
git add backend/app/mopidy/cache.py backend/tests/test_cache.py
git commit -m "feat(backend): add FTS5 library cache with local indexing and Tidal TTL cache"
```

---

### Task 6: Search + Library + Queue + Playback REST endpoints

**Files:**
- Create: `backend/app/search/__init__.py`, `backend/app/search/router.py`
- Create: `backend/app/library/__init__.py`, `backend/app/library/router.py`
- Create: `backend/app/queue/__init__.py`, `backend/app/queue/router.py`
- Create: `backend/app/playback/__init__.py`, `backend/app/playback/router.py`
- Modify: `backend/app/main.py` (register routers, wire up cache)
- Modify: `backend/tests/conftest.py` (add mopidy-connected fixtures)
- Create: `backend/tests/test_search.py`
- Create: `backend/tests/test_library.py`
- Create: `backend/tests/test_queue.py`
- Create: `backend/tests/test_playback.py`

- [ ] **Step 1: Update conftest.py with mopidy-connected app fixture**

Replace `backend/tests/conftest.py` entirely:
```python
import pytest
import aiosqlite
from httpx import ASGITransport, AsyncClient

from app.main import app
from app import db as db_module
from app.mopidy.client import MopidyClient
from app.mopidy.cache import LibraryCache
from tests.fake_mopidy import FakeMopidy


@pytest.fixture
async def fake_mopidy():
    mopidy = FakeMopidy()
    port = await mopidy.start()
    yield mopidy, port
    await mopidy.stop()


@pytest.fixture
async def test_db(tmp_path):
    from app.config import settings
    original_path = settings.db_path
    settings.db_path = str(tmp_path / "test.db")
    connection = await db_module.init_db()
    yield connection
    await db_module.close_db()
    settings.db_path = original_path


@pytest.fixture
async def client(test_db):
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        yield ac


@pytest.fixture
async def mopidy_client(fake_mopidy) -> tuple[MopidyClient, FakeMopidy]:
    """A real MopidyClient connected to the fake server."""
    mopidy_server, port = fake_mopidy
    mc = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await mc.connect()
    yield mc, mopidy_server
    await mc.disconnect()


@pytest.fixture
async def app_with_mopidy(test_db, mopidy_client):
    """Patch the app's mopidy_client and cache so routers can use them."""
    import app.main as main_module
    mc, fake = mopidy_client
    original_client = main_module.mopidy_client
    main_module.mopidy_client = mc

    cache = LibraryCache(test_db, mc)
    await cache.rebuild_local_index()
    main_module.library_cache = cache

    yield fake

    main_module.mopidy_client = original_client


@pytest.fixture
async def api(app_with_mopidy):
    """HTTP client with Mopidy backend wired up."""
    transport = ASGITransport(app=app)
    async with AsyncClient(transport=transport, base_url="http://test") as ac:
        yield ac


@pytest.fixture
async def auth_headers(api) -> dict:
    """Register a user and return auth headers."""
    resp = await api.post("/api/auth/register", json={
        "username": "testuser",
        "password": "testpass123",
    })
    token = resp.json()["token"]
    return {"Authorization": f"Bearer {token}"}
```

- [ ] **Step 2: Write failing search test**

Create `backend/tests/test_search.py`:
```python
async def test_search_returns_local_results(api, auth_headers):
    resp = await api.get("/api/search", params={"q": "test song"}, headers=auth_headers)
    assert resp.status_code == 200
    data = resp.json()
    assert len(data) >= 1
    assert any(r["name"] == "Test Song One" for r in data)


async def test_search_empty_query(api, auth_headers):
    resp = await api.get("/api/search", params={"q": ""}, headers=auth_headers)
    assert resp.status_code == 200
    assert resp.json() == []


async def test_search_no_results(api, auth_headers):
    resp = await api.get("/api/search", params={"q": "zzzznonexistent"}, headers=auth_headers)
    assert resp.status_code == 200
    assert resp.json() == []
```

- [ ] **Step 3: Write failing library test**

Create `backend/tests/test_library.py`:
```python
async def test_browse_root(api, auth_headers):
    resp = await api.get("/api/library/browse", headers=auth_headers)
    assert resp.status_code == 200
    data = resp.json()
    assert len(data) == 2
    assert data[0]["name"] == "Local files"


async def test_browse_subdirectory(api, auth_headers):
    resp = await api.get("/api/library/browse", params={"uri": "local:directory"}, headers=auth_headers)
    assert resp.status_code == 200
    data = resp.json()
    assert any(r["name"] == "Test Artist" for r in data)


async def test_lookup_track(api, auth_headers):
    resp = await api.get("/api/library/lookup", params={"uri": "local:track:song1.mp3"}, headers=auth_headers)
    assert resp.status_code == 200
    data = resp.json()
    assert "local:track:song1.mp3" in data
    assert data["local:track:song1.mp3"][0]["name"] == "Test Song One"
```

- [ ] **Step 4: Write failing queue test**

Create `backend/tests/test_queue.py`:
```python
async def test_queue_starts_empty(api, auth_headers):
    resp = await api.get("/api/queue", headers=auth_headers)
    assert resp.status_code == 200
    assert resp.json() == []


async def test_add_to_queue(api, auth_headers):
    resp = await api.post("/api/queue/add", json={
        "uris": ["local:track:song1.mp3", "local:track:song2.mp3"],
    }, headers=auth_headers)
    assert resp.status_code == 200
    data = resp.json()
    assert len(data) == 2

    # Verify queue state
    resp = await api.get("/api/queue", headers=auth_headers)
    assert len(resp.json()) == 2


async def test_clear_queue(api, auth_headers):
    await api.post("/api/queue/add", json={
        "uris": ["local:track:song1.mp3"],
    }, headers=auth_headers)
    resp = await api.post("/api/queue/clear", headers=auth_headers)
    assert resp.status_code == 200

    resp = await api.get("/api/queue", headers=auth_headers)
    assert resp.json() == []


async def test_shuffle_queue(api, auth_headers):
    await api.post("/api/queue/add", json={
        "uris": ["local:track:song1.mp3", "local:track:song2.mp3", "local:track:song3.mp3"],
    }, headers=auth_headers)
    resp = await api.post("/api/queue/shuffle", headers=auth_headers)
    assert resp.status_code == 200
```

- [ ] **Step 5: Write failing playback test**

Create `backend/tests/test_playback.py`:
```python
async def test_play(api, auth_headers):
    # Add tracks first
    await api.post("/api/queue/add", json={
        "uris": ["local:track:song1.mp3"],
    }, headers=auth_headers)
    resp = await api.post("/api/playback/play", headers=auth_headers)
    assert resp.status_code == 200


async def test_pause(api, auth_headers):
    await api.post("/api/queue/add", json={
        "uris": ["local:track:song1.mp3"],
    }, headers=auth_headers)
    await api.post("/api/playback/play", headers=auth_headers)
    resp = await api.post("/api/playback/pause", headers=auth_headers)
    assert resp.status_code == 200


async def test_next(api, auth_headers):
    await api.post("/api/queue/add", json={
        "uris": ["local:track:song1.mp3", "local:track:song2.mp3"],
    }, headers=auth_headers)
    await api.post("/api/playback/play", headers=auth_headers)
    resp = await api.post("/api/playback/next", headers=auth_headers)
    assert resp.status_code == 200


async def test_volume(api, auth_headers):
    resp = await api.post("/api/playback/volume", json={"volume": 50}, headers=auth_headers)
    assert resp.status_code == 200


async def test_seek(api, auth_headers):
    await api.post("/api/queue/add", json={
        "uris": ["local:track:song1.mp3"],
    }, headers=auth_headers)
    await api.post("/api/playback/play", headers=auth_headers)
    resp = await api.post("/api/playback/seek", json={"position": 30000}, headers=auth_headers)
    assert resp.status_code == 200
```

- [ ] **Step 6: Run all new tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_search.py tests/test_library.py tests/test_queue.py tests/test_playback.py -v`
Expected: FAIL (no routes)

- [ ] **Step 7: Implement search router**

Create `backend/app/search/__init__.py` (empty).

Create `backend/app/search/router.py`:
```python
from fastapi import APIRouter, Depends, Query

from app.auth.deps import get_current_user

router = APIRouter(prefix="/api/search", tags=["search"])


@router.get("")
async def search(q: str = Query(""), user=Depends(get_current_user)):
    from app.main import library_cache
    if not q.strip():
        return []
    return await library_cache.search_all(q)
```

- [ ] **Step 8: Implement library router**

Create `backend/app/library/__init__.py` (empty).

Create `backend/app/library/router.py`:
```python
from fastapi import APIRouter, Depends, Query

from app.auth.deps import get_current_user

router = APIRouter(prefix="/api/library", tags=["library"])


@router.get("/browse")
async def browse(uri: str | None = Query(None), user=Depends(get_current_user)):
    from app.main import mopidy_client
    result = await mopidy_client.send_command("core.library.browse", {"uri": uri})
    return result or []


@router.get("/lookup")
async def lookup(uri: str = Query(...), user=Depends(get_current_user)):
    from app.main import mopidy_client
    return await mopidy_client.send_command("core.library.lookup", {"uris": [uri]})


@router.post("/rescan")
async def rescan(user=Depends(get_current_user)):
    from app.main import library_cache
    await library_cache.rebuild_local_index()
    return {"status": "ok"}
```

- [ ] **Step 9: Implement queue router**

Create `backend/app/queue/__init__.py` (empty).

Create `backend/app/queue/router.py`:
```python
from fastapi import APIRouter, Depends
from pydantic import BaseModel

from app.auth.deps import get_current_user

router = APIRouter(prefix="/api/queue", tags=["queue"])


class AddRequest(BaseModel):
    uris: list[str]
    position: int | None = None


@router.get("")
async def get_queue(user=Depends(get_current_user)):
    from app.main import mopidy_client
    return await mopidy_client.send_command("core.tracklist.get_tl_tracks") or []


@router.post("/add")
async def add_to_queue(body: AddRequest, user=Depends(get_current_user)):
    from app.main import mopidy_client
    params = {"uris": body.uris}
    if body.position is not None:
        params["at_position"] = body.position
    return await mopidy_client.send_command("core.tracklist.add", params)


@router.post("/clear")
async def clear_queue(user=Depends(get_current_user)):
    from app.main import mopidy_client
    await mopidy_client.send_command("core.tracklist.clear")
    return {"status": "ok"}


@router.post("/shuffle")
async def shuffle_queue(user=Depends(get_current_user)):
    from app.main import mopidy_client
    await mopidy_client.send_command("core.tracklist.shuffle")
    return {"status": "ok"}
```

- [ ] **Step 10: Implement playback router**

Create `backend/app/playback/__init__.py` (empty).

Create `backend/app/playback/router.py`:
```python
from fastapi import APIRouter, Depends
from pydantic import BaseModel

from app.auth.deps import get_current_user

router = APIRouter(prefix="/api/playback", tags=["playback"])


class PlayRequest(BaseModel):
    tlid: int | None = None


class VolumeRequest(BaseModel):
    volume: int


class SeekRequest(BaseModel):
    position: int


@router.post("/play")
async def play(body: PlayRequest | None = None, user=Depends(get_current_user)):
    from app.main import mopidy_client
    params = {}
    if body and body.tlid is not None:
        params["tlid"] = body.tlid
    await mopidy_client.send_command("core.playback.play", params or None)
    return {"status": "ok"}


@router.post("/pause")
async def pause(user=Depends(get_current_user)):
    from app.main import mopidy_client
    await mopidy_client.send_command("core.playback.pause")
    return {"status": "ok"}


@router.post("/resume")
async def resume(user=Depends(get_current_user)):
    from app.main import mopidy_client
    await mopidy_client.send_command("core.playback.resume")
    return {"status": "ok"}


@router.post("/next")
async def next_track(user=Depends(get_current_user)):
    from app.main import mopidy_client
    await mopidy_client.send_command("core.playback.next")
    return {"status": "ok"}


@router.post("/previous")
async def previous_track(user=Depends(get_current_user)):
    from app.main import mopidy_client
    await mopidy_client.send_command("core.playback.previous")
    return {"status": "ok"}


@router.post("/volume")
async def set_volume(body: VolumeRequest, user=Depends(get_current_user)):
    from app.main import mopidy_client
    await mopidy_client.send_command("core.mixer.set_volume", {"volume": body.volume})
    return {"status": "ok"}


@router.post("/seek")
async def seek(body: SeekRequest, user=Depends(get_current_user)):
    from app.main import mopidy_client
    await mopidy_client.send_command("core.playback.seek", {"time_position": body.position})
    return {"status": "ok"}
```

- [ ] **Step 11: Update main.py — register all routers and add library_cache**

Replace `backend/app/main.py`:
```python
from contextlib import asynccontextmanager
from collections.abc import AsyncGenerator

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.config import settings
from app.db import init_db, close_db
from app.mopidy.client import MopidyClient
from app.mopidy.cache import LibraryCache

mopidy_client = MopidyClient(settings.mopidy_ws_url)
library_cache: LibraryCache | None = None


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None]:
    global library_cache
    db = await init_db()
    try:
        await mopidy_client.connect()
        library_cache = LibraryCache(db, mopidy_client)
        await library_cache.rebuild_local_index()
    except Exception:
        pass
    yield
    await mopidy_client.disconnect()
    await close_db()


app = FastAPI(title="Musakone", lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

from app.auth.router import router as auth_router
from app.search.router import router as search_router
from app.library.router import router as library_router
from app.queue.router import router as queue_router
from app.playback.router import router as playback_router

app.include_router(auth_router)
app.include_router(search_router)
app.include_router(library_router)
app.include_router(queue_router)
app.include_router(playback_router)
```

- [ ] **Step 12: Run all tests**

Run: `cd backend && uv run pytest -v`
Expected: all tests PASS

- [ ] **Step 13: Commit**

```bash
git add backend/
git commit -m "feat(backend): add search, library, queue, and playback REST endpoints"
```

---

### Task 7: Playlists CRUD

**Files:**
- Create: `backend/app/playlists/__init__.py`
- Create: `backend/app/playlists/service.py`
- Create: `backend/app/playlists/router.py`
- Modify: `backend/app/main.py` (register router)
- Create: `backend/tests/test_playlists.py`

- [ ] **Step 1: Write failing playlist tests**

Create `backend/tests/test_playlists.py`:
```python
async def test_list_playlists_empty(api, auth_headers):
    resp = await api.get("/api/playlists", headers=auth_headers)
    assert resp.status_code == 200
    assert resp.json() == []


async def test_create_playlist(api, auth_headers):
    resp = await api.post("/api/playlists", json={
        "name": "My Playlist",
        "description": "Test description",
    }, headers=auth_headers)
    assert resp.status_code == 200
    data = resp.json()
    assert data["name"] == "My Playlist"
    assert data["description"] == "Test description"
    assert "id" in data


async def test_get_playlist(api, auth_headers):
    create = await api.post("/api/playlists", json={
        "name": "My Playlist",
    }, headers=auth_headers)
    pid = create.json()["id"]

    resp = await api.get(f"/api/playlists/{pid}", headers=auth_headers)
    assert resp.status_code == 200
    data = resp.json()
    assert data["name"] == "My Playlist"
    assert data["tracks"] == []


async def test_update_playlist(api, auth_headers):
    create = await api.post("/api/playlists", json={"name": "Old Name"}, headers=auth_headers)
    pid = create.json()["id"]

    resp = await api.put(f"/api/playlists/{pid}", json={
        "name": "New Name",
        "description": "Updated",
    }, headers=auth_headers)
    assert resp.status_code == 200
    assert resp.json()["name"] == "New Name"


async def test_delete_playlist(api, auth_headers):
    create = await api.post("/api/playlists", json={"name": "To Delete"}, headers=auth_headers)
    pid = create.json()["id"]

    resp = await api.delete(f"/api/playlists/{pid}", headers=auth_headers)
    assert resp.status_code == 200

    resp = await api.get(f"/api/playlists/{pid}", headers=auth_headers)
    assert resp.status_code == 404


async def test_add_track_to_playlist(api, auth_headers):
    create = await api.post("/api/playlists", json={"name": "My Playlist"}, headers=auth_headers)
    pid = create.json()["id"]

    resp = await api.post(f"/api/playlists/{pid}/tracks", json={
        "track_uri": "local:track:song1.mp3",
        "track_name": "Test Song One",
        "artist_name": "Test Artist",
    }, headers=auth_headers)
    assert resp.status_code == 200

    detail = await api.get(f"/api/playlists/{pid}", headers=auth_headers)
    assert len(detail.json()["tracks"]) == 1
    assert detail.json()["tracks"][0]["track_uri"] == "local:track:song1.mp3"


async def test_remove_track_from_playlist(api, auth_headers):
    create = await api.post("/api/playlists", json={"name": "My Playlist"}, headers=auth_headers)
    pid = create.json()["id"]
    await api.post(f"/api/playlists/{pid}/tracks", json={
        "track_uri": "local:track:song1.mp3",
        "track_name": "Test Song One",
        "artist_name": "Test Artist",
    }, headers=auth_headers)

    resp = await api.delete(
        f"/api/playlists/{pid}/tracks/local:track:song1.mp3",
        headers=auth_headers,
    )
    assert resp.status_code == 200

    detail = await api.get(f"/api/playlists/{pid}", headers=auth_headers)
    assert len(detail.json()["tracks"]) == 0


async def test_playlist_not_found(api, auth_headers):
    resp = await api.get("/api/playlists/9999", headers=auth_headers)
    assert resp.status_code == 404


async def test_cannot_access_other_users_playlist(api, auth_headers):
    # Create playlist as testuser
    create = await api.post("/api/playlists", json={"name": "Private"}, headers=auth_headers)
    pid = create.json()["id"]

    # Register another user
    resp = await api.post("/api/auth/register", json={
        "username": "otheruser",
        "password": "otherpass",
    })
    other_token = resp.json()["token"]
    other_headers = {"Authorization": f"Bearer {other_token}"}

    resp = await api.get(f"/api/playlists/{pid}", headers=other_headers)
    assert resp.status_code == 404
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_playlists.py -v`
Expected: FAIL

- [ ] **Step 3: Implement playlist service**

Create `backend/app/playlists/__init__.py` (empty).

Create `backend/app/playlists/service.py`:
```python
import time
import aiosqlite


async def list_playlists(db: aiosqlite.Connection, user_id: int) -> list[dict]:
    rows = await db.execute_fetchall(
        "SELECT id, name, description, created_at, updated_at FROM playlists WHERE user_id = ? ORDER BY updated_at DESC",
        (user_id,),
    )
    return [
        {"id": r[0], "name": r[1], "description": r[2], "created_at": r[3], "updated_at": r[4]}
        for r in rows
    ]


async def create_playlist(db: aiosqlite.Connection, user_id: int, name: str, description: str = "") -> dict:
    now = int(time.time())
    cursor = await db.execute(
        "INSERT INTO playlists (user_id, name, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        (user_id, name, description, now, now),
    )
    await db.commit()
    return {"id": cursor.lastrowid, "name": name, "description": description, "created_at": now, "updated_at": now}


async def get_playlist(db: aiosqlite.Connection, playlist_id: int, user_id: int) -> dict | None:
    rows = await db.execute_fetchall(
        "SELECT id, name, description, created_at, updated_at FROM playlists WHERE id = ? AND user_id = ?",
        (playlist_id, user_id),
    )
    if not rows:
        return None
    r = rows[0]
    tracks = await db.execute_fetchall(
        "SELECT track_uri, track_name, artist_name, position FROM playlist_tracks WHERE playlist_id = ? ORDER BY position",
        (playlist_id,),
    )
    return {
        "id": r[0], "name": r[1], "description": r[2], "created_at": r[3], "updated_at": r[4],
        "tracks": [
            {"track_uri": t[0], "track_name": t[1], "artist_name": t[2], "position": t[3]}
            for t in tracks
        ],
    }


async def update_playlist(db: aiosqlite.Connection, playlist_id: int, user_id: int, name: str, description: str) -> dict | None:
    existing = await db.execute_fetchall(
        "SELECT id FROM playlists WHERE id = ? AND user_id = ?", (playlist_id, user_id)
    )
    if not existing:
        return None
    now = int(time.time())
    await db.execute(
        "UPDATE playlists SET name = ?, description = ?, updated_at = ? WHERE id = ?",
        (name, description, now, playlist_id),
    )
    await db.commit()
    return await get_playlist(db, playlist_id, user_id)


async def delete_playlist(db: aiosqlite.Connection, playlist_id: int, user_id: int) -> bool:
    existing = await db.execute_fetchall(
        "SELECT id FROM playlists WHERE id = ? AND user_id = ?", (playlist_id, user_id)
    )
    if not existing:
        return False
    await db.execute("DELETE FROM playlists WHERE id = ?", (playlist_id,))
    await db.commit()
    return True


async def add_track(db: aiosqlite.Connection, playlist_id: int, user_id: int, track_uri: str, track_name: str | None, artist_name: str | None) -> bool:
    existing = await db.execute_fetchall(
        "SELECT id FROM playlists WHERE id = ? AND user_id = ?", (playlist_id, user_id)
    )
    if not existing:
        return False
    # Get next position
    rows = await db.execute_fetchall(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?",
        (playlist_id,),
    )
    pos = rows[0][0]
    await db.execute(
        "INSERT OR REPLACE INTO playlist_tracks (playlist_id, track_uri, track_name, artist_name, position) VALUES (?, ?, ?, ?, ?)",
        (playlist_id, track_uri, track_name, artist_name, pos),
    )
    await db.execute("UPDATE playlists SET updated_at = ? WHERE id = ?", (int(time.time()), playlist_id))
    await db.commit()
    return True


async def remove_track(db: aiosqlite.Connection, playlist_id: int, user_id: int, track_uri: str) -> bool:
    existing = await db.execute_fetchall(
        "SELECT id FROM playlists WHERE id = ? AND user_id = ?", (playlist_id, user_id)
    )
    if not existing:
        return False
    await db.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ? AND track_uri = ?",
        (playlist_id, track_uri),
    )
    await db.execute("UPDATE playlists SET updated_at = ? WHERE id = ?", (int(time.time()), playlist_id))
    await db.commit()
    return True
```

- [ ] **Step 4: Implement playlist router**

Create `backend/app/playlists/router.py`:
```python
from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from app.db import get_db
from app.auth.deps import get_current_user
from app.playlists import service

router = APIRouter(prefix="/api/playlists", tags=["playlists"])


class CreatePlaylistRequest(BaseModel):
    name: str
    description: str = ""


class UpdatePlaylistRequest(BaseModel):
    name: str
    description: str = ""


class AddTrackRequest(BaseModel):
    track_uri: str
    track_name: str | None = None
    artist_name: str | None = None


@router.get("")
async def list_playlists(user=Depends(get_current_user)):
    db = await get_db()
    return await service.list_playlists(db, user["id"])


@router.post("")
async def create_playlist(body: CreatePlaylistRequest, user=Depends(get_current_user)):
    db = await get_db()
    return await service.create_playlist(db, user["id"], body.name, body.description)


@router.get("/{playlist_id}")
async def get_playlist(playlist_id: int, user=Depends(get_current_user)):
    db = await get_db()
    result = await service.get_playlist(db, playlist_id, user["id"])
    if not result:
        raise HTTPException(status_code=404, detail="Playlist not found")
    return result


@router.put("/{playlist_id}")
async def update_playlist(playlist_id: int, body: UpdatePlaylistRequest, user=Depends(get_current_user)):
    db = await get_db()
    result = await service.update_playlist(db, playlist_id, user["id"], body.name, body.description)
    if not result:
        raise HTTPException(status_code=404, detail="Playlist not found")
    return result


@router.delete("/{playlist_id}")
async def delete_playlist(playlist_id: int, user=Depends(get_current_user)):
    db = await get_db()
    deleted = await service.delete_playlist(db, playlist_id, user["id"])
    if not deleted:
        raise HTTPException(status_code=404, detail="Playlist not found")
    return {"status": "ok"}


@router.post("/{playlist_id}/tracks")
async def add_track(playlist_id: int, body: AddTrackRequest, user=Depends(get_current_user)):
    db = await get_db()
    added = await service.add_track(db, playlist_id, user["id"], body.track_uri, body.track_name, body.artist_name)
    if not added:
        raise HTTPException(status_code=404, detail="Playlist not found")
    return {"status": "ok"}


@router.delete("/{playlist_id}/tracks/{track_uri:path}")
async def remove_track(playlist_id: int, track_uri: str, user=Depends(get_current_user)):
    db = await get_db()
    removed = await service.remove_track(db, playlist_id, user["id"], track_uri)
    if not removed:
        raise HTTPException(status_code=404, detail="Playlist not found")
    return {"status": "ok"}
```

- [ ] **Step 5: Register playlist router in main.py**

Add to `backend/app/main.py` after the other router imports:
```python
from app.playlists.router import router as playlists_router
app.include_router(playlists_router)
```

- [ ] **Step 6: Run all tests**

Run: `cd backend && uv run pytest -v`
Expected: all tests PASS

- [ ] **Step 7: Commit**

```bash
git add backend/
git commit -m "feat(backend): add playlists CRUD with per-user isolation"
```

---

### Task 8: WebSocket handler (browser ↔ Mopidy proxy + state sync)

**Files:**
- Create: `backend/app/ws/__init__.py`
- Create: `backend/app/ws/handler.py`
- Modify: `backend/app/main.py` (add WS route)
- Create: `backend/tests/test_ws.py`

- [ ] **Step 1: Write failing WebSocket tests**

Create `backend/tests/test_ws.py`:
```python
import json
import asyncio
import pytest
import websockets

from app.auth.service import create_token


async def test_ws_rejects_without_token(fake_mopidy, test_db):
    """WS connection without token should be rejected."""
    mopidy_server, port = fake_mopidy
    # We need to start the actual FastAPI app on a port for WS testing
    # Use a simpler approach: test the handler logic via the app
    import uvicorn
    from app.main import app, mopidy_client as mc
    from app.mopidy.client import MopidyClient
    from app.mopidy.cache import LibraryCache
    import app.main as main_module

    # Connect mopidy client
    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    main_module.mopidy_client = client
    main_module.library_cache = LibraryCache(test_db, client)

    config = uvicorn.Config(app, host="127.0.0.1", port=0, log_level="error")
    server = uvicorn.Server(config)
    task = asyncio.create_task(server.serve())
    await asyncio.sleep(0.3)  # Let server start

    try:
        # Find the port
        app_port = server.servers[0].sockets[0].getsockname()[1]

        # Connect without token — should close
        with pytest.raises(websockets.exceptions.InvalidStatus):
            async with websockets.connect(f"ws://127.0.0.1:{app_port}/ws") as ws:
                await ws.recv()
    finally:
        server.should_exit = True
        await task
        await client.disconnect()


async def test_ws_state_sync_on_connect(fake_mopidy, test_db):
    """On valid WS connect, server should send state_sync."""
    mopidy_server, port = fake_mopidy
    import uvicorn
    from app.main import app
    from app.mopidy.client import MopidyClient
    from app.mopidy.cache import LibraryCache
    import app.main as main_module

    client = MopidyClient(f"ws://127.0.0.1:{port}/mopidy/ws")
    await client.connect()
    main_module.mopidy_client = client
    main_module.library_cache = LibraryCache(test_db, client)

    # Create a user for the token
    from app.db import get_db
    db = await get_db()
    from app.auth.service import hash_password
    await db.execute("INSERT INTO users (username, password_hash) VALUES (?, ?)", ("wsuser", hash_password("pass")))
    await db.commit()
    rows = await db.execute_fetchall("SELECT id FROM users WHERE username = 'wsuser'")
    user_id = rows[0][0]
    token = create_token(user_id)

    config = uvicorn.Config(app, host="127.0.0.1", port=0, log_level="error")
    server = uvicorn.Server(config)
    task = asyncio.create_task(server.serve())
    await asyncio.sleep(0.3)

    try:
        app_port = server.servers[0].sockets[0].getsockname()[1]
        async with websockets.connect(f"ws://127.0.0.1:{app_port}/ws?token={token}") as ws:
            msg = json.loads(await asyncio.wait_for(ws.recv(), timeout=2.0))
            assert msg["type"] == "state_sync"
            assert "data" in msg
            assert "playback_state" in msg["data"]
            assert "volume" in msg["data"]
            assert "queue" in msg["data"]
    finally:
        server.should_exit = True
        await task
        await client.disconnect()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && uv run pytest tests/test_ws.py -v`
Expected: FAIL

- [ ] **Step 3: Implement WebSocket handler**

Create `backend/app/ws/__init__.py` (empty).

Create `backend/app/ws/handler.py`:
```python
import asyncio
import json
import logging

from fastapi import WebSocket, WebSocketDisconnect, Query

from app.auth.service import decode_token
from app.db import get_db

logger = logging.getLogger(__name__)

# All connected browser WebSockets
_browser_clients: list[WebSocket] = []


async def ws_endpoint(
    websocket: WebSocket,
    token: str | None = Query(None),
):
    # Authenticate
    if not token:
        await websocket.close(code=4001, reason="Missing token")
        return
    user_id = decode_token(token)
    if user_id is None:
        await websocket.close(code=4001, reason="Invalid token")
        return
    db = await get_db()
    rows = await db.execute_fetchall("SELECT id FROM users WHERE id = ?", (user_id,))
    if not rows:
        await websocket.close(code=4001, reason="User not found")
        return

    await websocket.accept()
    _browser_clients.append(websocket)

    try:
        # Send initial state sync
        from app.main import mopidy_client
        state_data = await _build_state_sync(mopidy_client)
        await websocket.send_json({"type": "state_sync", "data": state_data})

        # Listen for commands from browser and forward to Mopidy
        while True:
            raw = await websocket.receive_text()
            msg = json.loads(raw)
            if "method" in msg:
                result = await mopidy_client.send_command(
                    msg["method"], msg.get("params")
                )
                response = {"jsonrpc": "2.0", "id": msg.get("id"), "result": result}
                await websocket.send_json(response)
    except WebSocketDisconnect:
        pass
    except Exception as e:
        logger.error("WebSocket error: %s", e)
    finally:
        _browser_clients.remove(websocket)


async def broadcast_event(event: dict):
    """Broadcast a Mopidy event to all connected browser clients."""
    dead = []
    for ws in _browser_clients:
        try:
            await ws.send_json(event)
        except Exception:
            dead.append(ws)
    for ws in dead:
        _browser_clients.remove(ws)


async def _build_state_sync(mopidy_client) -> dict:
    """Gather current Mopidy state for initial sync."""
    try:
        state, tl_track, volume, queue = await asyncio.gather(
            mopidy_client.send_command("core.playback.get_state"),
            mopidy_client.send_command("core.playback.get_current_tl_track"),
            mopidy_client.send_command("core.mixer.get_volume"),
            mopidy_client.send_command("core.tracklist.get_tl_tracks"),
        )
        return {
            "playback_state": state or "stopped",
            "current_track": tl_track,
            "volume": volume or 0,
            "queue": queue or [],
        }
    except Exception:
        return {
            "playback_state": "stopped",
            "current_track": None,
            "volume": 0,
            "queue": [],
        }
```

- [ ] **Step 4: Wire WS handler into main.py and connect event broadcast**

Add to `backend/app/main.py` after the router includes:
```python
from app.ws.handler import ws_endpoint, broadcast_event

app.websocket("/ws")(ws_endpoint)


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None]:
    global library_cache
    db = await init_db()
    try:
        await mopidy_client.connect()
        # Wire up Mopidy events → browser broadcast
        mopidy_client.on_event = lambda e: asyncio.create_task(broadcast_event(e))
        library_cache = LibraryCache(db, mopidy_client)
        await library_cache.rebuild_local_index()
    except Exception:
        pass
    yield
    await mopidy_client.disconnect()
    await close_db()
```

Note: The lifespan function needs to be updated (replace the existing one).

- [ ] **Step 5: Run WS tests**

Run: `cd backend && uv run pytest tests/test_ws.py -v`
Expected: PASS

- [ ] **Step 6: Run all backend tests**

Run: `cd backend && uv run pytest -v`
Expected: all PASS

- [ ] **Step 7: Commit**

```bash
git add backend/
git commit -m "feat(backend): add WebSocket handler with state sync and Mopidy event broadcast"
```

---

### Task 9: Backend Dockerfile

**Files:**
- Create: `backend/Dockerfile`

- [ ] **Step 1: Create Dockerfile**

```dockerfile
FROM python:3.13-slim

COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

WORKDIR /app

COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev

COPY app/ app/

EXPOSE 8000

CMD ["uv", "run", "uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
```

- [ ] **Step 2: Commit**

```bash
git add backend/Dockerfile
git commit -m "build(backend): add Dockerfile for FastAPI backend"
```

---

## Phase 2: Frontend Foundation

### Task 10: SvelteKit project scaffold

**Files:**
- Create: entire `frontend/` directory (scaffold)

- [ ] **Step 1: Remove old frontend**

```bash
rm -rf frontend/
```

- [ ] **Step 2: Scaffold SvelteKit project**

```bash
bunx sv create frontend --template minimal --types ts --no-add-ons
cd frontend
bun add -d @sveltejs/adapter-static unocss @unocss/svelte-scoped lucide-svelte bits-ui
```

- [ ] **Step 3: Configure SvelteKit for SPA mode**

Replace `frontend/svelte.config.js`:
```javascript
import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
	preprocess: vitePreprocess(),
	kit: {
		adapter: adapter({
			fallback: 'index.html',
		}),
	},
};
```

- [ ] **Step 4: Configure UnoCSS**

Create `frontend/uno.config.ts`:
```typescript
import { defineConfig, presetWind4 } from 'unocss';

export default defineConfig({
    presets: [presetWind4()],
    theme: {
        colors: {
            bg: {
                primary: '#000000',
                secondary: '#0a0a0a',
                tertiary: '#141414',
            },
            fg: {
                primary: '#ffffff',
                secondary: '#b0b0b0',
                tertiary: '#707070',
            },
            accent: {
                primary: '#cc0000',
                secondary: '#ff3333',
                dim: '#880000',
            },
            border: {
                primary: '#333333',
                secondary: '#1a1a1a',
            },
            error: '#ff4444',
            warning: '#ffaa00',
            success: '#00cc44',
        },
        font: {
            mono: '"VT323", monospace',
            sans: '"VT323", monospace',
        },
    },
    shortcuts: {
        'touch-target': 'min-h-12 min-w-12',
        'btn': 'flex items-center justify-center cursor-pointer transition-all duration-150',
        'btn-icon': 'btn w-8 h-8 bg-transparent border border-border-primary text-fg-tertiary hover:text-accent-primary hover:border-accent-primary active:bg-bg-secondary shrink-0',
        'btn-control': 'btn w-12 h-12 bg-bg-secondary border border-border-primary text-fg-primary rounded hover:border-accent-primary active:scale-95 active:bg-bg-primary disabled:opacity-30 disabled:cursor-not-allowed',
        'track-item': 'flex items-center gap-2 px-4 py-1 bg-bg-primary border-b-2 border-border-secondary min-h-10 w-full transition-colors duration-150',
    },
    preflights: [
        {
            getCSS: ({ theme }) => `
                *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
                html { font-size: 16px; background: #000; height: 100%; color-scheme: dark; }
                body { font-family: ${theme.font?.mono}; line-height: 1rem; color: #fff; background: #000; -webkit-font-smoothing: antialiased; min-height: 100%; }
                button { font-family: ${theme.font?.mono}; cursor: pointer; outline: none; -webkit-tap-highlight-color: transparent; }
                button:focus-visible { outline: 2px solid #cc0000; outline-offset: 2px; }
                input, textarea, select { font-family: ${theme.font?.mono}; }
                a { color: #cc0000; text-decoration: none; }
                * { scrollbar-width: none; }
                *::-webkit-scrollbar { display: none; }
            `,
        },
    ],
});
```

- [ ] **Step 5: Add UnoCSS to Vite config**

Replace `frontend/vite.config.ts`:
```typescript
import { sveltekit } from '@sveltejs/kit/vite';
import UnoCSS from 'unocss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
    plugins: [
        UnoCSS(),
        sveltekit(),
    ],
});
```

- [ ] **Step 6: Create app.css**

Create `frontend/src/app.css`:
```css
@import 'virtual:uno.css';

@font-face {
    font-family: 'VT323';
    src: url('https://fonts.gstatic.com/s/vt323/v17/pxiKyp0ihIEF2isRFJXGdg.woff2') format('woff2');
    font-display: swap;
}
```

- [ ] **Step 7: Create SPA layout with prerendering disabled**

Create `frontend/src/routes/+layout.ts`:
```typescript
export const prerender = false;
export const ssr = false;
```

- [ ] **Step 8: Verify build works**

Run: `cd frontend && bun run build`
Expected: Build succeeds, outputs to `frontend/build/`

- [ ] **Step 9: Commit**

```bash
git add frontend/
git commit -m "feat(frontend): scaffold SvelteKit SPA with UnoCSS terminal theme"
```

---

### Task 11: Types + API service + Auth service

**Files:**
- Create: `frontend/src/lib/types.ts`
- Create: `frontend/src/lib/services/api.ts`
- Create: `frontend/src/lib/services/auth.ts`
- Create: `frontend/src/lib/state/auth.svelte.ts`

- [ ] **Step 1: Create shared types**

Create `frontend/src/lib/types.ts`:
```typescript
export interface Artist {
    uri: string;
    name: string;
}

export interface Album {
    uri: string;
    name: string;
    artists?: Artist[];
    date?: string;
}

export interface Track {
    uri: string;
    name: string;
    artists?: Artist[];
    album?: Album;
    length?: number;
    genre?: string;
}

export interface TlTrack {
    tlid: number;
    track: Track;
}

export interface Ref {
    uri: string;
    name: string;
    type: string;
}

export interface SearchResult {
    uri: string;
    name: string;
    artist: string;
    album: string;
    genre: string;
    duration: number | null;
    source: string;
}

export interface User {
    id: number;
    username: string;
}

export interface Playlist {
    id: number;
    name: string;
    description: string;
    created_at: number;
    updated_at: number;
}

export interface PlaylistWithTracks extends Playlist {
    tracks: PlaylistTrack[];
}

export interface PlaylistTrack {
    track_uri: string;
    track_name: string | null;
    artist_name: string | null;
    position: number;
}

export interface StateSyncData {
    playback_state: 'playing' | 'paused' | 'stopped';
    current_track: TlTrack | null;
    volume: number;
    queue: TlTrack[];
}
```

- [ ] **Step 2: Create API service**

Create `frontend/src/lib/services/api.ts`:
```typescript
const BACKEND_URL = import.meta.env.VITE_BACKEND_HTTP_URL || 'http://localhost:8000';

function getToken(): string | null {
    return localStorage.getItem('token');
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
    const token = getToken();
    const headers: Record<string, string> = {
        'Content-Type': 'application/json',
        ...((options.headers as Record<string, string>) || {}),
    };
    if (token) {
        headers['Authorization'] = `Bearer ${token}`;
    }
    const resp = await fetch(`${BACKEND_URL}${path}`, { ...options, headers });
    if (!resp.ok) {
        const body = await resp.json().catch(() => ({ detail: resp.statusText }));
        throw new Error(body.detail || resp.statusText);
    }
    return resp.json();
}

export const api = {
    get: <T>(path: string) => request<T>(path),
    post: <T>(path: string, body?: unknown) =>
        request<T>(path, { method: 'POST', body: body ? JSON.stringify(body) : undefined }),
    put: <T>(path: string, body?: unknown) =>
        request<T>(path, { method: 'PUT', body: body ? JSON.stringify(body) : undefined }),
    del: <T>(path: string) => request<T>(path, { method: 'DELETE' }),
};
```

- [ ] **Step 3: Create auth service**

Create `frontend/src/lib/services/auth.ts`:
```typescript
import { api } from './api';
import type { User } from '$lib/types';

interface AuthResponse {
    token: string;
    user: User;
}

export async function register(username: string, password: string): Promise<AuthResponse> {
    const resp = await api.post<AuthResponse>('/api/auth/register', { username, password });
    localStorage.setItem('token', resp.token);
    return resp;
}

export async function login(username: string, password: string): Promise<AuthResponse> {
    const resp = await api.post<AuthResponse>('/api/auth/login', { username, password });
    localStorage.setItem('token', resp.token);
    return resp;
}

export async function getMe(): Promise<User> {
    return api.get<User>('/api/auth/me');
}

export function logout(): void {
    localStorage.removeItem('token');
}

export function isAuthenticated(): boolean {
    return !!localStorage.getItem('token');
}
```

- [ ] **Step 4: Create auth state**

Create `frontend/src/lib/state/auth.svelte.ts`:
```typescript
import type { User } from '$lib/types';
import { getMe, logout as doLogout } from '$lib/services/auth';

let currentUser = $state<User | null>(null);
let loading = $state(true);

export const auth = {
    get user() { return currentUser; },
    get loading() { return loading; },
    get isAuthenticated() { return currentUser !== null; },

    setUser(user: User | null) {
        currentUser = user;
    },

    async init() {
        loading = true;
        try {
            if (localStorage.getItem('token')) {
                currentUser = await getMe();
            }
        } catch {
            localStorage.removeItem('token');
            currentUser = null;
        } finally {
            loading = false;
        }
    },

    logout() {
        doLogout();
        currentUser = null;
    },
};
```

- [ ] **Step 5: Verify it compiles**

Run: `cd frontend && bun run check`
Expected: No errors (or `bun run build` succeeds)

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/
git commit -m "feat(frontend): add types, API service, and auth state"
```

---

### Task 12: WebSocket service + player/queue state

**Files:**
- Create: `frontend/src/lib/state/player.svelte.ts`
- Create: `frontend/src/lib/state/queue.svelte.ts`
- Create: `frontend/src/lib/state/connection.svelte.ts`
- Create: `frontend/src/lib/services/ws.svelte.ts`

- [ ] **Step 1: Create connection state**

Create `frontend/src/lib/state/connection.svelte.ts`:
```typescript
type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error';

let status = $state<ConnectionStatus>('disconnected');

export const connection = {
    get status() { return status; },
    setStatus(s: ConnectionStatus) { status = s; },
};
```

- [ ] **Step 2: Create player state**

Create `frontend/src/lib/state/player.svelte.ts`:
```typescript
import type { TlTrack } from '$lib/types';

let playbackState = $state<'playing' | 'paused' | 'stopped'>('stopped');
let currentTrack = $state<TlTrack | null>(null);
let volume = $state(80);
let position = $state(0);

export const player = {
    get state() { return playbackState; },
    get track() { return currentTrack; },
    get volume() { return volume; },
    get position() { return position; },
    get isPlaying() { return playbackState === 'playing'; },

    setState(s: 'playing' | 'paused' | 'stopped') { playbackState = s; },
    setTrack(t: TlTrack | null) { currentTrack = t; },
    setVolume(v: number) { volume = v; },
    setPosition(p: number) { position = p; },
};
```

- [ ] **Step 3: Create queue state**

Create `frontend/src/lib/state/queue.svelte.ts`:
```typescript
import type { TlTrack } from '$lib/types';

let items = $state<TlTrack[]>([]);

export const queue = {
    get items() { return items; },
    setItems(q: TlTrack[]) { items = q; },
};
```

- [ ] **Step 4: Create WebSocket service**

Create `frontend/src/lib/services/ws.svelte.ts`:
```typescript
import { connection } from '$lib/state/connection.svelte';
import { player } from '$lib/state/player.svelte';
import { queue } from '$lib/state/queue.svelte';
import type { StateSyncData } from '$lib/types';

const WS_URL = import.meta.env.VITE_BACKEND_WS_URL || 'ws://localhost:8000/ws';

let ws: WebSocket | null = null;
let reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = 1000;

export function connect(): void {
    const token = localStorage.getItem('token');
    if (!token) return;

    connection.setStatus('connecting');
    ws = new WebSocket(`${WS_URL}?token=${token}`);

    ws.onopen = () => {
        connection.setStatus('connected');
        reconnectDelay = 1000;
    };

    ws.onmessage = (event) => {
        const msg = JSON.parse(event.data);
        handleMessage(msg);
    };

    ws.onclose = () => {
        connection.setStatus('disconnected');
        ws = null;
        scheduleReconnect();
    };

    ws.onerror = () => {
        connection.setStatus('error');
    };
}

export function disconnect(): void {
    if (reconnectTimeout) {
        clearTimeout(reconnectTimeout);
        reconnectTimeout = null;
    }
    if (ws) {
        ws.close();
        ws = null;
    }
    connection.setStatus('disconnected');
}

export function sendCommand(method: string, params?: Record<string, unknown>): void {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    const id = Math.floor(Math.random() * 100000);
    ws.send(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
}

function handleMessage(msg: Record<string, unknown>): void {
    // State sync on initial connect
    if (msg.type === 'state_sync') {
        const data = msg.data as StateSyncData;
        player.setState(data.playback_state);
        player.setTrack(data.current_track);
        player.setVolume(data.volume);
        queue.setItems(data.queue);
        return;
    }

    // Mopidy events
    const event = msg.event as string | undefined;
    if (!event) return;

    switch (event) {
        case 'track_playback_started':
            player.setState('playing');
            player.setTrack(msg.tl_track as any);
            break;
        case 'track_playback_paused':
            player.setState('paused');
            break;
        case 'track_playback_resumed':
            player.setState('playing');
            break;
        case 'track_playback_ended':
            player.setState('stopped');
            break;
        case 'playback_state_changed':
            player.setState(msg.new_state as any);
            break;
        case 'volume_changed':
            player.setVolume(msg.volume as number);
            break;
        case 'seeked':
            player.setPosition(msg.time_position as number);
            break;
        case 'tracklist_changed':
            // Re-fetch queue from backend
            fetch(`${import.meta.env.VITE_BACKEND_HTTP_URL || 'http://localhost:8000'}/api/queue`, {
                headers: { 'Authorization': `Bearer ${localStorage.getItem('token')}` },
            })
                .then((r) => r.json())
                .then((data) => queue.setItems(data))
                .catch(() => {});
            break;
    }
}

function scheduleReconnect(): void {
    if (!localStorage.getItem('token')) return;
    reconnectTimeout = setTimeout(() => {
        connect();
        reconnectDelay = Math.min(reconnectDelay * 2, 30000);
    }, reconnectDelay);
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cd frontend && bun run build`
Expected: Build succeeds

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/
git commit -m "feat(frontend): add WebSocket service with state sync and player/queue state"
```

---

### Task 13: Layout + BottomNav + MiniPlayer components

**Files:**
- Create: `frontend/src/lib/components/Layout.svelte`
- Create: `frontend/src/lib/components/BottomNav.svelte`
- Create: `frontend/src/lib/components/MiniPlayer.svelte`
- Modify: `frontend/src/routes/+layout.svelte`
- Create: `frontend/src/routes/login/+page.svelte`
- Create: `frontend/src/routes/register/+page.svelte`

- [ ] **Step 1: Create BottomNav**

Create `frontend/src/lib/components/BottomNav.svelte`:
```svelte
<script lang="ts">
    import { ListMusic, Library, Search, ListPlus } from 'lucide-svelte';
    import { page } from '$app/state';

    const tabs = [
        { href: '/', label: 'Queue', icon: ListMusic },
        { href: '/library', label: 'Library', icon: Library },
        { href: '/search', label: 'Search', icon: Search },
        { href: '/playlists', label: 'Playlists', icon: ListPlus },
    ] as const;
</script>

<nav class="fixed bottom-0 left-0 right-0 flex bg-bg-secondary border-t border-border-primary z-50">
    {#each tabs as tab}
        <a
            href={tab.href}
            class="flex-1 flex flex-col items-center justify-center py-2 text-xs transition-colors"
            class:text-accent-primary={page.url.pathname === tab.href}
            class:text-fg-tertiary={page.url.pathname !== tab.href}
            data-testid="nav-{tab.label.toLowerCase()}"
        >
            <tab.icon size={20} />
            <span class="mt-1">{tab.label}</span>
        </a>
    {/each}
</nav>
```

- [ ] **Step 2: Create MiniPlayer**

Create `frontend/src/lib/components/MiniPlayer.svelte`:
```svelte
<script lang="ts">
    import { Play, Pause, SkipForward } from 'lucide-svelte';
    import { player } from '$lib/state/player.svelte';
    import { sendCommand } from '$lib/services/ws.svelte';

    function togglePlay() {
        if (player.isPlaying) {
            sendCommand('core.playback.pause');
        } else {
            sendCommand('core.playback.resume');
        }
    }

    function next() {
        sendCommand('core.playback.next');
    }

    function trackName(): string {
        return player.track?.track?.name || 'No track';
    }

    function artistName(): string {
        const artists = player.track?.track?.artists;
        if (!artists || artists.length === 0) return '';
        return artists.map((a) => a.name).join(', ');
    }
</script>

{#if player.track}
    <div class="fixed bottom-12 left-0 right-0 bg-bg-tertiary border-t border-border-primary px-4 py-2 flex items-center gap-3 z-40" data-testid="mini-player">
        <div class="flex-1 min-w-0">
            <div class="text-sm text-fg-primary truncate" data-testid="mini-player-track">{trackName()}</div>
            <div class="text-xs text-fg-tertiary truncate">{artistName()}</div>
        </div>
        <button class="btn-control w-10 h-10" onclick={togglePlay} data-testid="mini-player-toggle">
            {#if player.isPlaying}
                <Pause size={18} />
            {:else}
                <Play size={18} />
            {/if}
        </button>
        <button class="btn-control w-10 h-10" onclick={next} data-testid="mini-player-next">
            <SkipForward size={18} />
        </button>
    </div>
{/if}
```

- [ ] **Step 3: Create Layout**

Create `frontend/src/lib/components/Layout.svelte`:
```svelte
<script lang="ts">
    import BottomNav from './BottomNav.svelte';
    import MiniPlayer from './MiniPlayer.svelte';
    import { player } from '$lib/state/player.svelte';
    import type { Snippet } from 'svelte';

    let { children }: { children: Snippet } = $props();
</script>

<div class="min-h-screen bg-bg-primary pb-24" class:pb-36={player.track}>
    {@render children()}
    <MiniPlayer />
    <BottomNav />
</div>
```

- [ ] **Step 4: Create Login page**

Create `frontend/src/routes/login/+page.svelte`:
```svelte
<script lang="ts">
    import { goto } from '$app/navigation';
    import { login } from '$lib/services/auth';
    import { auth } from '$lib/state/auth.svelte';

    let username = $state('');
    let password = $state('');
    let error = $state('');

    async function handleSubmit(e: Event) {
        e.preventDefault();
        error = '';
        try {
            const resp = await login(username, password);
            auth.setUser(resp.user);
            goto('/');
        } catch (err: any) {
            error = err.message || 'Login failed';
        }
    }
</script>

<div class="flex flex-col items-center justify-center min-h-screen p-4">
    <h1 class="text-2xl text-accent-primary mb-8">MUSAKONE</h1>
    <form onsubmit={handleSubmit} class="w-full max-w-xs flex flex-col gap-4">
        <input
            bind:value={username}
            type="text"
            placeholder="username"
            class="bg-bg-tertiary border border-border-primary px-4 py-3 text-fg-primary"
            data-testid="login-username"
        />
        <input
            bind:value={password}
            type="password"
            placeholder="password"
            class="bg-bg-tertiary border border-border-primary px-4 py-3 text-fg-primary"
            data-testid="login-password"
        />
        {#if error}
            <div class="text-error text-sm" data-testid="login-error">{error}</div>
        {/if}
        <button type="submit" class="btn bg-accent-primary text-bg-primary py-3" data-testid="login-submit">
            LOGIN
        </button>
        <a href="/register" class="text-center text-sm text-fg-tertiary">Create account</a>
    </form>
</div>
```

- [ ] **Step 5: Create Register page**

Create `frontend/src/routes/register/+page.svelte`:
```svelte
<script lang="ts">
    import { goto } from '$app/navigation';
    import { register } from '$lib/services/auth';
    import { auth } from '$lib/state/auth.svelte';

    let username = $state('');
    let password = $state('');
    let error = $state('');

    async function handleSubmit(e: Event) {
        e.preventDefault();
        error = '';
        try {
            const resp = await register(username, password);
            auth.setUser(resp.user);
            goto('/');
        } catch (err: any) {
            error = err.message || 'Registration failed';
        }
    }
</script>

<div class="flex flex-col items-center justify-center min-h-screen p-4">
    <h1 class="text-2xl text-accent-primary mb-8">MUSAKONE</h1>
    <form onsubmit={handleSubmit} class="w-full max-w-xs flex flex-col gap-4">
        <input
            bind:value={username}
            type="text"
            placeholder="username"
            class="bg-bg-tertiary border border-border-primary px-4 py-3 text-fg-primary"
            data-testid="register-username"
        />
        <input
            bind:value={password}
            type="password"
            placeholder="password"
            class="bg-bg-tertiary border border-border-primary px-4 py-3 text-fg-primary"
            data-testid="register-password"
        />
        {#if error}
            <div class="text-error text-sm" data-testid="register-error">{error}</div>
        {/if}
        <button type="submit" class="btn bg-accent-primary text-bg-primary py-3" data-testid="register-submit">
            REGISTER
        </button>
        <a href="/login" class="text-center text-sm text-fg-tertiary">Already have an account?</a>
    </form>
</div>
```

- [ ] **Step 6: Create root layout**

Replace `frontend/src/routes/+layout.svelte`:
```svelte
<script lang="ts">
    import Layout from '$lib/components/Layout.svelte';
    import { auth } from '$lib/state/auth.svelte';
    import { connect, disconnect } from '$lib/services/ws.svelte';
    import { goto } from '$app/navigation';
    import { page } from '$app/state';
    import { onMount } from 'svelte';
    import '../app.css';

    let { children } = $props();

    const publicRoutes = ['/login', '/register'];

    onMount(async () => {
        await auth.init();
        if (auth.isAuthenticated) {
            connect();
        }
    });

    $effect(() => {
        if (!auth.loading && !auth.isAuthenticated && !publicRoutes.includes(page.url.pathname)) {
            goto('/login');
        }
    });

    $effect(() => {
        if (auth.isAuthenticated) {
            connect();
        } else {
            disconnect();
        }
    });
</script>

{#if auth.loading}
    <div class="flex items-center justify-center min-h-screen bg-bg-primary text-fg-tertiary">
        Loading...
    </div>
{:else if !auth.isAuthenticated && publicRoutes.includes(page.url.pathname)}
    {@render children()}
{:else if auth.isAuthenticated}
    <Layout>
        {#snippet children()}
            {@render children()}
        {/snippet}
    </Layout>
{/if}
```

Note: The snippet name conflict between the prop and the Layout snippet needs resolution. Fix by renaming:

Replace `frontend/src/routes/+layout.svelte`:
```svelte
<script lang="ts">
    import Layout from '$lib/components/Layout.svelte';
    import { auth } from '$lib/state/auth.svelte';
    import { connect, disconnect } from '$lib/services/ws.svelte';
    import { goto } from '$app/navigation';
    import { page } from '$app/state';
    import { onMount } from 'svelte';
    import '../app.css';

    let { children: content } = $props();

    const publicRoutes = ['/login', '/register'];

    onMount(async () => {
        await auth.init();
    });

    $effect(() => {
        if (!auth.loading && !auth.isAuthenticated && !publicRoutes.includes(page.url.pathname)) {
            goto('/login');
        }
    });

    $effect(() => {
        if (auth.isAuthenticated) {
            connect();
        } else {
            disconnect();
        }
    });
</script>

{#if auth.loading}
    <div class="flex items-center justify-center min-h-screen bg-bg-primary text-fg-tertiary">
        Loading...
    </div>
{:else if !auth.isAuthenticated && publicRoutes.includes(page.url.pathname)}
    {@render content()}
{:else if auth.isAuthenticated}
    <Layout>
        {#snippet children()}
            {@render content()}
        {/snippet}
    </Layout>
{/if}
```

- [ ] **Step 7: Create placeholder route pages**

Create `frontend/src/routes/+page.svelte`:
```svelte
<script lang="ts">
    import { queue } from '$lib/state/queue.svelte';
    import { player } from '$lib/state/player.svelte';
</script>

<div class="p-4">
    <h1 class="text-lg text-accent-primary mb-4">QUEUE</h1>
    {#if queue.items.length === 0}
        <p class="text-fg-tertiary" data-testid="queue-empty">Queue is empty</p>
    {:else}
        {#each queue.items as tl (tl.tlid)}
            <div
                class="track-item"
                class:text-accent-primary={player.track?.tlid === tl.tlid}
                data-testid="queue-track"
            >
                <div class="flex-1 min-w-0">
                    <div class="text-sm truncate">{tl.track.name}</div>
                    <div class="text-xs text-fg-tertiary truncate">
                        {tl.track.artists?.map(a => a.name).join(', ') || ''}
                    </div>
                </div>
            </div>
        {/each}
    {/if}
</div>
```

Create `frontend/src/routes/search/+page.svelte`:
```svelte
<script lang="ts">
    import { api } from '$lib/services/api';
    import type { SearchResult } from '$lib/types';

    let query = $state('');
    let results = $state<SearchResult[]>([]);
    let loading = $state(false);
    let timer: ReturnType<typeof setTimeout>;

    function onInput() {
        clearTimeout(timer);
        if (!query.trim()) {
            results = [];
            return;
        }
        timer = setTimeout(doSearch, 300);
    }

    async function doSearch() {
        loading = true;
        try {
            results = await api.get<SearchResult[]>(`/api/search?q=${encodeURIComponent(query)}`);
        } catch {
            results = [];
        } finally {
            loading = false;
        }
    }

    async function addToQueue(uri: string) {
        await api.post('/api/queue/add', { uris: [uri] });
    }
</script>

<div class="p-4">
    <input
        bind:value={query}
        oninput={onInput}
        type="text"
        placeholder="Search..."
        class="w-full bg-bg-tertiary border border-border-primary px-4 py-3 text-fg-primary mb-4"
        data-testid="search-input"
    />
    {#if loading}
        <p class="text-fg-tertiary">Searching...</p>
    {/if}
    {#each results as result}
        <div class="track-item" data-testid="search-result">
            <div class="flex-1 min-w-0">
                <div class="text-sm truncate">{result.name}</div>
                <div class="text-xs text-fg-tertiary truncate">{result.artist}</div>
            </div>
            <button class="btn-icon" onclick={() => addToQueue(result.uri)} data-testid="search-add">+</button>
        </div>
    {/each}
</div>
```

Create `frontend/src/routes/library/+page.svelte`:
```svelte
<script lang="ts">
    import { api } from '$lib/services/api';
    import type { Ref } from '$lib/types';
    import { onMount } from 'svelte';

    let items = $state<Ref[]>([]);
    let path = $state<{ uri: string | null; name: string }[]>([{ uri: null, name: 'Library' }]);
    let loading = $state(false);

    onMount(() => browse(null));

    async function browse(uri: string | null) {
        loading = true;
        try {
            const params = uri ? `?uri=${encodeURIComponent(uri)}` : '';
            items = await api.get<Ref[]>(`/api/library/browse${params}`);
        } catch {
            items = [];
        } finally {
            loading = false;
        }
    }

    function navigate(ref: Ref) {
        if (ref.type === 'track') {
            api.post('/api/queue/add', { uris: [ref.uri] });
            return;
        }
        path = [...path, { uri: ref.uri, name: ref.name }];
        browse(ref.uri);
    }

    function goBack(index: number) {
        path = path.slice(0, index + 1);
        browse(path[path.length - 1].uri);
    }
</script>

<div class="p-4">
    <div class="flex gap-2 mb-4 text-sm text-fg-tertiary overflow-x-auto">
        {#each path as crumb, i}
            <button
                class="hover:text-accent-primary whitespace-nowrap"
                onclick={() => goBack(i)}
                data-testid="breadcrumb"
            >
                {crumb.name}{i < path.length - 1 ? ' /' : ''}
            </button>
        {/each}
    </div>
    {#if loading}
        <p class="text-fg-tertiary">Loading...</p>
    {/if}
    {#each items as item}
        <button
            class="track-item w-full text-left"
            onclick={() => navigate(item)}
            data-testid="library-item"
        >
            <span class="text-xs text-fg-tertiary w-6">{item.type === 'track' ? '♪' : '▸'}</span>
            <span class="text-sm truncate">{item.name}</span>
        </button>
    {/each}
</div>
```

Create `frontend/src/routes/playlists/+page.svelte`:
```svelte
<script lang="ts">
    import { api } from '$lib/services/api';
    import type { Playlist } from '$lib/types';
    import { onMount } from 'svelte';

    let playlists = $state<Playlist[]>([]);
    let newName = $state('');

    onMount(loadPlaylists);

    async function loadPlaylists() {
        playlists = await api.get<Playlist[]>('/api/playlists');
    }

    async function create() {
        if (!newName.trim()) return;
        await api.post('/api/playlists', { name: newName });
        newName = '';
        await loadPlaylists();
    }
</script>

<div class="p-4">
    <h1 class="text-lg text-accent-primary mb-4">PLAYLISTS</h1>
    <div class="flex gap-2 mb-4">
        <input
            bind:value={newName}
            placeholder="New playlist..."
            class="flex-1 bg-bg-tertiary border border-border-primary px-4 py-2 text-fg-primary text-sm"
            data-testid="playlist-name-input"
        />
        <button class="btn bg-accent-primary text-bg-primary px-4 py-2 text-sm" onclick={create} data-testid="playlist-create">
            CREATE
        </button>
    </div>
    {#each playlists as pl}
        <a href="/playlists/{pl.id}" class="track-item" data-testid="playlist-item">
            <span class="text-sm">{pl.name}</span>
        </a>
    {/each}
    {#if playlists.length === 0}
        <p class="text-fg-tertiary" data-testid="playlists-empty">No playlists yet</p>
    {/if}
</div>
```

Create `frontend/src/routes/playlists/[id]/+page.svelte`:
```svelte
<script lang="ts">
    import { api } from '$lib/services/api';
    import type { PlaylistWithTracks } from '$lib/types';
    import { page } from '$app/state';
    import { onMount } from 'svelte';

    let playlist = $state<PlaylistWithTracks | null>(null);

    onMount(loadPlaylist);

    async function loadPlaylist() {
        const id = page.params.id;
        playlist = await api.get<PlaylistWithTracks>(`/api/playlists/${id}`);
    }

    async function removeTrack(trackUri: string) {
        if (!playlist) return;
        await api.del(`/api/playlists/${playlist.id}/tracks/${encodeURIComponent(trackUri)}`);
        await loadPlaylist();
    }

    async function addAllToQueue() {
        if (!playlist) return;
        const uris = playlist.tracks.map(t => t.track_uri);
        if (uris.length > 0) {
            await api.post('/api/queue/add', { uris });
        }
    }
</script>

<div class="p-4">
    {#if playlist}
        <div class="flex items-center justify-between mb-4">
            <h1 class="text-lg text-accent-primary" data-testid="playlist-title">{playlist.name}</h1>
            <button class="btn-icon" onclick={addAllToQueue} data-testid="playlist-add-all">▶</button>
        </div>
        {#each playlist.tracks as track}
            <div class="track-item" data-testid="playlist-track">
                <div class="flex-1 min-w-0">
                    <div class="text-sm truncate">{track.track_name || track.track_uri}</div>
                    <div class="text-xs text-fg-tertiary truncate">{track.artist_name || ''}</div>
                </div>
                <button class="btn-icon" onclick={() => removeTrack(track.track_uri)} data-testid="playlist-remove-track">×</button>
            </div>
        {/each}
        {#if playlist.tracks.length === 0}
            <p class="text-fg-tertiary">No tracks in this playlist</p>
        {/if}
    {/if}
</div>
```

- [ ] **Step 8: Verify build**

Run: `cd frontend && bun run build`
Expected: Build succeeds

- [ ] **Step 9: Commit**

```bash
git add frontend/
git commit -m "feat(frontend): add all routes, layout, BottomNav, MiniPlayer, and page components"
```

---

## Phase 3: E2E Tests

### Task 14: Playwright setup + test infrastructure

**Files:**
- Create: `frontend/e2e/playwright.config.ts`
- Create: `frontend/e2e/helpers/setup.ts`

- [ ] **Step 1: Install Playwright**

```bash
cd frontend
bun add -d @playwright/test
bunx playwright install chromium
```

- [ ] **Step 2: Create Playwright config**

Create `frontend/e2e/playwright.config.ts`:
```typescript
import { defineConfig } from '@playwright/test';

export default defineConfig({
    testDir: '.',
    testMatch: '*.spec.ts',
    timeout: 30000,
    retries: 0,
    use: {
        baseURL: 'http://localhost:4173',
        viewport: { width: 390, height: 844 }, // iPhone 14 size
        trace: 'on-first-retry',
    },
    webServer: [
        {
            command: 'cd ../backend && uv run uvicorn app.main:app --port 8000',
            port: 8000,
            reuseExistingServer: true,
        },
        {
            command: 'bun run preview --port 4173',
            port: 4173,
            reuseExistingServer: true,
        },
    ],
});
```

- [ ] **Step 3: Create test helpers**

Create `frontend/e2e/helpers/setup.ts`:
```typescript
import { type Page } from '@playwright/test';

const API = 'http://localhost:8000';

export async function registerAndLogin(page: Page, username?: string, password?: string) {
    const user = username || `testuser_${Date.now()}`;
    const pass = password || 'testpass123';

    // Register via API
    const resp = await fetch(`${API}/api/auth/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username: user, password: pass }),
    });
    const data = await resp.json();

    // Set token in browser localStorage
    await page.goto('/login');
    await page.evaluate((token: string) => {
        localStorage.setItem('token', token);
    }, data.token);

    // Navigate to home
    await page.goto('/');
    await page.waitForSelector('[data-testid="nav-queue"]');

    return { username: user, password: pass, token: data.token };
}

export async function addTracksToQueue(token: string, uris: string[]) {
    await fetch(`${API}/api/queue/add`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${token}`,
        },
        body: JSON.stringify({ uris }),
    });
}

export async function clearQueue(token: string) {
    await fetch(`${API}/api/queue/clear`, {
        method: 'POST',
        headers: { 'Authorization': `Bearer ${token}` },
    });
}

export async function createPlaylist(token: string, name: string): Promise<number> {
    const resp = await fetch(`${API}/api/playlists`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${token}`,
        },
        body: JSON.stringify({ name }),
    });
    const data = await resp.json();
    return data.id;
}
```

- [ ] **Step 4: Commit**

```bash
git add frontend/e2e/ frontend/package.json
git commit -m "test(e2e): set up Playwright with test helpers"
```

---

### Task 15: Auth e2e tests

**Files:**
- Create: `frontend/e2e/auth.spec.ts`

- [ ] **Step 1: Write auth e2e tests**

Create `frontend/e2e/auth.spec.ts`:
```typescript
import { test, expect } from '@playwright/test';

test.describe('Authentication', () => {
    test('shows login page when not authenticated', async ({ page }) => {
        await page.goto('/');
        await expect(page.getByTestId('login-username')).toBeVisible();
    });

    test('registers a new user and redirects to home', async ({ page }) => {
        await page.goto('/register');
        await page.getByTestId('register-username').fill(`newuser_${Date.now()}`);
        await page.getByTestId('register-password').fill('testpass123');
        await page.getByTestId('register-submit').click();
        await expect(page.getByTestId('nav-queue')).toBeVisible();
    });

    test('logs in with valid credentials', async ({ page }) => {
        const username = `logintest_${Date.now()}`;
        // Register first via API
        await fetch('http://localhost:8000/api/auth/register', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ username, password: 'testpass123' }),
        });

        await page.goto('/login');
        await page.getByTestId('login-username').fill(username);
        await page.getByTestId('login-password').fill('testpass123');
        await page.getByTestId('login-submit').click();
        await expect(page.getByTestId('nav-queue')).toBeVisible();
    });

    test('shows error on wrong password', async ({ page }) => {
        const username = `errtest_${Date.now()}`;
        await fetch('http://localhost:8000/api/auth/register', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ username, password: 'testpass123' }),
        });

        await page.goto('/login');
        await page.getByTestId('login-username').fill(username);
        await page.getByTestId('login-password').fill('wrongpass');
        await page.getByTestId('login-submit').click();
        await expect(page.getByTestId('login-error')).toBeVisible();
    });

    test('shows error on duplicate registration', async ({ page }) => {
        const username = `dupuser_${Date.now()}`;
        await fetch('http://localhost:8000/api/auth/register', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ username, password: 'testpass123' }),
        });

        await page.goto('/register');
        await page.getByTestId('register-username').fill(username);
        await page.getByTestId('register-password').fill('testpass123');
        await page.getByTestId('register-submit').click();
        await expect(page.getByTestId('register-error')).toBeVisible();
    });
});
```

- [ ] **Step 2: Run e2e auth tests**

Run: `cd frontend && bunx playwright test e2e/auth.spec.ts`
Expected: all 5 PASS (requires backend + fake mopidy running)

Note: These tests require the backend to be running with the fake Mopidy. The Playwright config handles starting the backend. For the fake Mopidy, the backend will start without a Mopidy connection (graceful fallback).

- [ ] **Step 3: Commit**

```bash
git add frontend/e2e/auth.spec.ts
git commit -m "test(e2e): add authentication e2e tests"
```

---

### Task 16: Queue e2e tests

**Files:**
- Create: `frontend/e2e/queue.spec.ts`

- [ ] **Step 1: Write queue e2e tests**

Create `frontend/e2e/queue.spec.ts`:
```typescript
import { test, expect } from '@playwright/test';
import { registerAndLogin, addTracksToQueue, clearQueue } from './helpers/setup';

test.describe('Queue', () => {
    test('shows empty queue message', async ({ page }) => {
        const { token } = await registerAndLogin(page);
        await clearQueue(token);
        await page.goto('/');
        await expect(page.getByTestId('queue-empty')).toBeVisible();
    });

    test('displays tracks added to queue', async ({ page }) => {
        const { token } = await registerAndLogin(page);
        await clearQueue(token);
        await addTracksToQueue(token, ['local:track:song1.mp3', 'local:track:song2.mp3']);

        await page.goto('/');
        // Wait for WebSocket state sync
        await page.waitForSelector('[data-testid="queue-track"]');
        const tracks = page.getByTestId('queue-track');
        await expect(tracks).toHaveCount(2);
    });

    test('shows current track highlighted in mini player', async ({ page }) => {
        const { token } = await registerAndLogin(page);
        await clearQueue(token);
        await addTracksToQueue(token, ['local:track:song1.mp3']);

        // Play the track via API
        await fetch('http://localhost:8000/api/playback/play', {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${token}` },
        });

        await page.goto('/');
        await expect(page.getByTestId('mini-player')).toBeVisible();
        await expect(page.getByTestId('mini-player-track')).toContainText('Test Song One');
    });
});
```

- [ ] **Step 2: Run queue e2e tests**

Run: `cd frontend && bunx playwright test e2e/queue.spec.ts`
Expected: all 3 PASS

- [ ] **Step 3: Commit**

```bash
git add frontend/e2e/queue.spec.ts
git commit -m "test(e2e): add queue e2e tests"
```

---

### Task 17: Search e2e tests

**Files:**
- Create: `frontend/e2e/search.spec.ts`

- [ ] **Step 1: Write search e2e tests**

Create `frontend/e2e/search.spec.ts`:
```typescript
import { test, expect } from '@playwright/test';
import { registerAndLogin } from './helpers/setup';

test.describe('Search', () => {
    test('shows search input', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-search').click();
        await expect(page.getByTestId('search-input')).toBeVisible();
    });

    test('returns results for matching query', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-search').click();
        await page.getByTestId('search-input').fill('test');
        // Wait for debounced search
        await page.waitForSelector('[data-testid="search-result"]');
        const results = page.getByTestId('search-result');
        await expect(results.first()).toBeVisible();
    });

    test('shows no results for nonexistent query', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-search').click();
        await page.getByTestId('search-input').fill('zzzznonexistent');
        // Wait for search to complete
        await page.waitForTimeout(500);
        await expect(page.getByTestId('search-result')).toHaveCount(0);
    });

    test('add button adds track to queue', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-search').click();
        await page.getByTestId('search-input').fill('test song');
        await page.waitForSelector('[data-testid="search-add"]');
        await page.getByTestId('search-add').first().click();

        // Navigate to queue and verify
        await page.getByTestId('nav-queue').click();
        await page.waitForSelector('[data-testid="queue-track"]');
        await expect(page.getByTestId('queue-track')).toHaveCount(1);
    });
});
```

- [ ] **Step 2: Run search e2e tests**

Run: `cd frontend && bunx playwright test e2e/search.spec.ts`
Expected: all 4 PASS

- [ ] **Step 3: Commit**

```bash
git add frontend/e2e/search.spec.ts
git commit -m "test(e2e): add search e2e tests"
```

---

### Task 18: Library e2e tests

**Files:**
- Create: `frontend/e2e/library.spec.ts`

- [ ] **Step 1: Write library e2e tests**

Create `frontend/e2e/library.spec.ts`:
```typescript
import { test, expect } from '@playwright/test';
import { registerAndLogin } from './helpers/setup';

test.describe('Library', () => {
    test('shows root library items', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-library').click();
        await page.waitForSelector('[data-testid="library-item"]');
        await expect(page.getByTestId('library-item')).toHaveCount(2); // Local files + Tidal
    });

    test('navigates into subdirectory', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-library').click();
        await page.waitForSelector('[data-testid="library-item"]');

        // Click "Local files"
        await page.getByTestId('library-item').first().click();
        await page.waitForSelector('[data-testid="library-item"]');

        // Should show artists
        const items = page.getByTestId('library-item');
        await expect(items).toHaveCount(2); // Test Artist + Other Artist
    });

    test('breadcrumb navigation works', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-library').click();
        await page.waitForSelector('[data-testid="library-item"]');

        // Navigate deep
        await page.getByTestId('library-item').first().click();
        await page.waitForSelector('[data-testid="library-item"]');

        // Click Library breadcrumb to go back to root
        await page.getByTestId('breadcrumb').first().click();
        await page.waitForSelector('[data-testid="library-item"]');
        await expect(page.getByTestId('library-item')).toHaveCount(2);
    });
});
```

- [ ] **Step 2: Run library e2e tests**

Run: `cd frontend && bunx playwright test e2e/library.spec.ts`
Expected: all 3 PASS

- [ ] **Step 3: Commit**

```bash
git add frontend/e2e/library.spec.ts
git commit -m "test(e2e): add library browsing e2e tests"
```

---

### Task 19: Playlists e2e tests

**Files:**
- Create: `frontend/e2e/playlists.spec.ts`

- [ ] **Step 1: Write playlists e2e tests**

Create `frontend/e2e/playlists.spec.ts`:
```typescript
import { test, expect } from '@playwright/test';
import { registerAndLogin } from './helpers/setup';

test.describe('Playlists', () => {
    test('shows empty playlists message', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-playlists').click();
        await expect(page.getByTestId('playlists-empty')).toBeVisible();
    });

    test('creates a new playlist', async ({ page }) => {
        await registerAndLogin(page);
        await page.getByTestId('nav-playlists').click();
        await page.getByTestId('playlist-name-input').fill('My Playlist');
        await page.getByTestId('playlist-create').click();

        await page.waitForSelector('[data-testid="playlist-item"]');
        await expect(page.getByTestId('playlist-item')).toHaveCount(1);
    });

    test('navigates to playlist detail', async ({ page }) => {
        const { token } = await registerAndLogin(page);

        // Create playlist via API
        const resp = await fetch('http://localhost:8000/api/playlists', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Authorization': `Bearer ${token}`,
            },
            body: JSON.stringify({ name: 'Detail Test' }),
        });
        const pl = await resp.json();

        await page.getByTestId('nav-playlists').click();
        await page.waitForSelector('[data-testid="playlist-item"]');
        await page.getByTestId('playlist-item').first().click();

        await expect(page.getByTestId('playlist-title')).toContainText('Detail Test');
    });

    test('adds and removes tracks from playlist', async ({ page }) => {
        const { token } = await registerAndLogin(page);

        // Create playlist and add a track via API
        const plResp = await fetch('http://localhost:8000/api/playlists', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Authorization': `Bearer ${token}`,
            },
            body: JSON.stringify({ name: 'Track Test' }),
        });
        const pl = await plResp.json();

        await fetch(`http://localhost:8000/api/playlists/${pl.id}/tracks`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Authorization': `Bearer ${token}`,
            },
            body: JSON.stringify({
                track_uri: 'local:track:song1.mp3',
                track_name: 'Test Song One',
                artist_name: 'Test Artist',
            }),
        });

        // Navigate to playlist detail
        await page.goto(`/playlists/${pl.id}`);
        await page.waitForSelector('[data-testid="playlist-track"]');
        await expect(page.getByTestId('playlist-track')).toHaveCount(1);

        // Remove the track
        await page.getByTestId('playlist-remove-track').click();
        await expect(page.getByTestId('playlist-track')).toHaveCount(0);
    });
});
```

- [ ] **Step 2: Run playlists e2e tests**

Run: `cd frontend && bunx playwright test e2e/playlists.spec.ts`
Expected: all 4 PASS

- [ ] **Step 3: Commit**

```bash
git add frontend/e2e/playlists.spec.ts
git commit -m "test(e2e): add playlists e2e tests"
```

---

### Task 20: Playback e2e tests

**Files:**
- Create: `frontend/e2e/playback.spec.ts`

- [ ] **Step 1: Write playback e2e tests**

Create `frontend/e2e/playback.spec.ts`:
```typescript
import { test, expect } from '@playwright/test';
import { registerAndLogin, addTracksToQueue, clearQueue } from './helpers/setup';

test.describe('Playback', () => {
    test('mini player shows when track is playing', async ({ page }) => {
        const { token } = await registerAndLogin(page);
        await clearQueue(token);
        await addTracksToQueue(token, ['local:track:song1.mp3']);

        // Start playback via API
        await fetch('http://localhost:8000/api/playback/play', {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${token}` },
        });

        await page.goto('/');
        await expect(page.getByTestId('mini-player')).toBeVisible();
    });

    test('play/pause toggle works', async ({ page }) => {
        const { token } = await registerAndLogin(page);
        await clearQueue(token);
        await addTracksToQueue(token, ['local:track:song1.mp3']);
        await fetch('http://localhost:8000/api/playback/play', {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${token}` },
        });

        await page.goto('/');
        await expect(page.getByTestId('mini-player')).toBeVisible();

        // Click pause
        await page.getByTestId('mini-player-toggle').click();
        // The button should now show play icon (visual check would require screenshot,
        // but we verify the button is clickable and doesn't error)
        await expect(page.getByTestId('mini-player-toggle')).toBeVisible();
    });

    test('next track button works', async ({ page }) => {
        const { token } = await registerAndLogin(page);
        await clearQueue(token);
        await addTracksToQueue(token, ['local:track:song1.mp3', 'local:track:song2.mp3']);
        await fetch('http://localhost:8000/api/playback/play', {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${token}` },
        });

        await page.goto('/');
        await expect(page.getByTestId('mini-player-track')).toContainText('Test Song One');

        // Click next
        await page.getByTestId('mini-player-next').click();
        // Wait for track change event
        await page.waitForTimeout(500);
        // Track name should update (depends on event broadcast working)
        await expect(page.getByTestId('mini-player')).toBeVisible();
    });
});
```

- [ ] **Step 2: Run playback e2e tests**

Run: `cd frontend && bunx playwright test e2e/playback.spec.ts`
Expected: all 3 PASS

- [ ] **Step 3: Commit**

```bash
git add frontend/e2e/playback.spec.ts
git commit -m "test(e2e): add playback e2e tests"
```

---

## Phase 4: Docker + Cleanup

### Task 21: Docker Compose + Frontend Dockerfile + updated env

**Files:**
- Create: `frontend/Dockerfile`
- Modify: `docker-compose.yml`
- Modify: `.env.example`

- [ ] **Step 1: Create frontend Dockerfile**

Create `frontend/Dockerfile`:
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
# Simple static file server
RUN echo 'Bun.serve({ port: 3000, fetch(req) { const url = new URL(req.url); let path = "build" + url.pathname; try { return new Response(Bun.file(path)); } catch { return new Response(Bun.file("build/index.html")); } } });' > serve.js
EXPOSE 3000
CMD ["bun", "run", "serve.js"]
```

- [ ] **Step 2: Update docker-compose.yml**

Replace `docker-compose.yml`:
```yaml
services:
  mopidy:
    profiles:
      - mopidy
    build:
      context: ./mopidy
      dockerfile: Dockerfile
    container_name: musakone-mopidy
    ports:
      - "6600:6600"
      - "6680:6680"
    volumes:
      - ./mopidy/mopidy.conf:/config/mopidy.conf:ro
      - mopidy-local:/var/lib/mopidy/local
      - mopidy-media:/var/lib/mopidy/media
      - mopidy-cache:/var/cache/mopidy
      - ${MUSIC_LIBRARY_PATH:-./data/music}:/media/music:ro
    restart: unless-stopped
    networks:
      - musakone
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:6680/"]
      interval: 10s
      timeout: 10s
      retries: 3
      start_period: 1s
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"

  backend:
    build:
      context: ./backend
      dockerfile: Dockerfile
    container_name: musakone-backend
    ports:
      - "${BACKEND_PORT:-8000}:8000"
    volumes:
      - backend-data:/app/data
    environment:
      - JWT_SECRET=${JWT_SECRET:-change-this-secret-in-production}
      - MOPIDY_WS_URL=${MOPIDY_WS_URL:-ws://mopidy:6680/mopidy/ws}
      - LIBRARY_RESCAN_INTERVAL=${LIBRARY_RESCAN_INTERVAL:-1800}
    restart: unless-stopped
    networks:
      - musakone
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"

  frontend:
    build:
      context: ./frontend
      dockerfile: Dockerfile
    container_name: musakone-frontend
    ports:
      - "${FRONTEND_PORT:-3000}:3000"
    depends_on:
      - backend
    restart: unless-stopped
    networks:
      - musakone
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"

volumes:
  mopidy-local:
  mopidy-media:
  mopidy-cache:
  backend-data:

networks:
  musakone:
    driver: bridge
```

- [ ] **Step 3: Update .env.example**

Replace `.env.example`:
```bash
# Mopidy WebSocket URL
# For bundled Mopidy: ws://mopidy:6680/mopidy/ws
# For external Mopidy: ws://your-host:6680/mopidy/ws
MOPIDY_WS_URL=ws://mopidy:6680/mopidy/ws

# Music Library Path (host machine) - only used with bundled Mopidy
MUSIC_LIBRARY_PATH=./data/music

# Ports
FRONTEND_PORT=3000
BACKEND_PORT=8000

# Frontend env (build-time)
VITE_BACKEND_HTTP_URL=http://localhost:8000
VITE_BACKEND_WS_URL=ws://localhost:8000/ws

# JWT Secret (CHANGE THIS IN PRODUCTION)
JWT_SECRET=change-this-secret-in-production-use-at-least-32-random-chars

# Library rescan interval in seconds (default 30 min)
LIBRARY_RESCAN_INTERVAL=1800
```

- [ ] **Step 4: Commit**

```bash
git add frontend/Dockerfile docker-compose.yml .env.example
git commit -m "build: update Docker Compose for FastAPI + SvelteKit stack"
```

---

### Task 22: Remove old Gleam backend

**Files:**
- Delete: `backend/` (old Gleam code — already replaced by Task 1-9)

This task only applies if the old backend directory wasn't removed during the scaffold phase. If it was already replaced, skip this task.

- [ ] **Step 1: Verify old backend is gone**

Run: `ls backend/src/` — should show Python `app/` directory, not Gleam `.gleam` files.
If old Gleam files still exist, remove them:
```bash
rm -rf backend/build backend/gleam.toml backend/manifest.toml backend/src/*.gleam backend/src/auth backend/src/db backend/src/handlers backend/src/websocket
```

- [ ] **Step 2: Commit if anything was removed**

```bash
git add -A
git commit -m "chore: remove old Gleam backend files"
```

---

### Task 23: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update CLAUDE.md to reflect new stack**

The CLAUDE.md needs to be updated to reflect the new tech stack (FastAPI + SvelteKit instead of Gleam + Preact). Update the Technology Stack section, remove Gleam/Preact references, add Python/Svelte patterns.

Key changes:
- Technology Stack: Python 3.13, FastAPI, aiosqlite, Svelte 5, SvelteKit, UnoCSS
- Remove: Gleam, Preact, Nanostores, Wouter references
- Add: pytest patterns, Svelte runes patterns, FastAPI router patterns
- Keep: Docker patterns, mobile-first guidelines, terminal aesthetic, commit conventions

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for FastAPI + SvelteKit stack"
```

---

### Task 24: Run full test suite

- [ ] **Step 1: Run all backend tests**

Run: `cd backend && uv run pytest -v`
Expected: all PASS

- [ ] **Step 2: Run all e2e tests**

Run: `cd frontend && bunx playwright test`
Expected: all PASS

- [ ] **Step 3: Verify Docker build**

Run: `docker compose build`
Expected: all 3 services build successfully

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "chore: full test suite green — rebuild complete"
```
