# MusakoneV3 Backend

Gleam-based WebSocket server that acts as middleware between frontend clients and Mopidy music server.

## Features

- **WebSocket Server**: Real-time bidirectional communication
- **JWT Authentication**: Secure user authentication and session management
- **Message Routing**: Routes messages between clients and Mopidy
- **User Action Tracking**: Logs all user actions to SQLite database
- **Statistics**: Provides user listening statistics and history
- **Health Checks**: Docker-compatible health check endpoint

## Architecture

```
[Frontend Client] <--> [Backend WebSocket] <--> [Mopidy WebSocket]
                           ↓
                    [SQLite Database]
                    - Users
                    - Sessions
                    - Action Logs
```

## API Endpoints

### HTTP Endpoints

- `GET /health` - Health check endpoint
- `POST /api/auth/login` - User login (returns JWT token)
- `GET /api/auth/me` - Get current user info (requires auth)
- `GET /api/analytics/stats` - Get user statistics (requires auth)
- `GET /api/analytics/events|affinity|admin|export` - Activity, learned taste, room dashboard, raw export (requires auth)
- `GET /api/playback/state` - Public now-playing snapshot; `GET /api/playback/history` - state timeline (requires auth)
- `GET /api/recommendations?scope=me|room` - Tracks to play next (requires auth)
- `GET|PUT /api/autoplay`, `POST /api/autoplay/fill` - Autoplay settings and manual refill (requires auth)
- `GET /api/insights` - Everything behind the Mix → Stats page (requires auth)

See `API.md` for payloads.

### WebSocket Endpoint

- `WS /ws` - WebSocket connection for real-time communication

## WebSocket Message Format

### Client → Backend

**Authentication:**
```json
{
  "type": "auth",
  "token": "jwt_token_here"
}
```

**Mopidy Request:**
```json
{
  "type": "mopidy",
  "token": "jwt_token_here",
  "method": "core.playback.play",
  "params": {}
}
```

### Backend → Client

**Auth Success:**
```json
{
  "type": "auth_success",
  "user_id": 1
}
```

**Mopidy Response:**
```json
{
  "type": "mopidy_response",
  "method": "core.playback.play",
  "result": {}
}
```

**Error:**
```json
{
  "type": "auth_error",
  "error": "Invalid token"
}
```

## Environment Variables

- `PORT` - Server port (default: 3001)
- `DATABASE_PATH` - SQLite database path (default: /app/data/musakone.db)
- `JWT_SECRET` - Secret for JWT token signing (required in production)
- `MOPIDY_URL` - Mopidy WebSocket URL (default: ws://mopidy:6680/mopidy/ws)

## Development

### Local Development

```bash
# Install Gleam (using mise)
mise install

# Download dependencies
gleam deps download

# Run in development mode
gleam run

# Run tests
gleam test

# Format code
gleam format
```

### Docker Build

```bash
# Build image
docker build -t musakone-backend .

# Run container
docker run -p 3001:3001 \
  -e JWT_SECRET=your-secret \
  -v $(pwd)/data:/app/data \
  musakone-backend
```

## Database Schema

### Users Table
```sql
CREATE TABLE users (
  id INTEGER PRIMARY KEY,
  username TEXT UNIQUE,
  password_hash TEXT,
  created_at INTEGER,
  last_login INTEGER
)
```

### User Actions Table
```sql
CREATE TABLE user_actions (
  id INTEGER PRIMARY KEY,
  user_id INTEGER,
  action_type TEXT,
  track_uri TEXT,
  track_name TEXT,
  artist TEXT,
  position_ms INTEGER,
  metadata TEXT,
  timestamp INTEGER
)
```

### Sessions Table
```sql
CREATE TABLE sessions (
  id INTEGER PRIMARY KEY,
  user_id INTEGER,
  token_hash TEXT UNIQUE,
  expires_at INTEGER,
  created_at INTEGER
)
```

## Default Credentials

**⚠️ Change in production!**

- Username: `admin`
- Password: `admin`

## Project Structure

```
backend/
├── src/
│   ├── musakone_backend.gleam  # Main entry point
│   ├── auth/
│   │   ├── jwt.gleam           # JWT token generation/validation
│   │   ├── service.gleam       # User authentication
│   │   └── tracker.gleam       # User action logging
│   ├── db/
│   │   ├── connection.gleam    # Database connection
│   │   └── schema.gleam        # Database schema
│   ├── mopidy/
│   │   └── client.gleam        # Mopidy WebSocket client
│   └── websocket/
│       ├── handler.gleam       # WebSocket message handling
│       └── routes.gleam        # HTTP routes
├── gleam.toml                  # Project configuration
├── manifest.toml               # Dependency lock file
├── Dockerfile                  # Docker build configuration
└── README.md                   # This file
```

## Security Notes

1. **JWT Secret**: Always set a strong `JWT_SECRET` in production
2. **Default User**: Change the default admin password immediately
3. **HTTPS**: Use HTTPS in production (handled by reverse proxy)
4. **CORS**: Configure CORS properly for your frontend domain
5. **Rate Limiting**: Consider adding rate limiting for login endpoint

## Tracked Actions

The backend logs these user actions:
- `play` - User played a track
- `pause` - User paused playback
- `stop` - User stopped playback
- `skip` - User skipped to next track
- `previous` - User went to previous track
- `search` - User performed a search
- `queue_add` - User added track to queue
- `queue_remove` - User removed track from queue
- `volume_changed` - User changed volume