# MusakoneV3

A lightweight, mobile-first web frontend for Mopidy music server with a terminal-inspired aesthetic.

## Features

- 🎵 **Mobile-First Design**: Optimized for touch interaction and mobile devices
- 🎨 **Terminal Aesthetic**: ncmpcpp-inspired UI with high contrast and monospace fonts
- ⚡ **Lightweight**: < 50KB gzipped bundle size
- 🔄 **Real-time Updates**: WebSocket integration for live playback status
- 📱 **PWA Support**: Install on mobile home screen
- 🎹 **Full Control**: Playback, queue management (drag to reorder), library browsing, and search
- ✨ **Mix**: Recommendations from the room's listening history + Tidal discovery, autoplay that keeps the queue from running dry, and listening stats
- 🐳 **Docker Ready**: Easy deployment with Docker Compose

## Architecture

- **Frontend**: Preact + TypeScript + Nanostores (minimal bundle)
- **Backend**: Mopidy music server
- **Runtime**: Bun (fast JavaScript runtime)
- **Deployment**: Docker Compose

## Quick Start

### Prerequisites

- Docker and Docker Compose
- Music files (optional, can use streaming services)

### Setup

1. **Clone the repository**:
   ```bash
   git clone https://github.com/yourusername/musakoneV3.git
   cd musakoneV3
   ```

2. **Configure environment**:
   ```bash
   cp .env.example .env
   nano .env  # Edit with your settings
   ```

3. **Set your music library path** (optional):
   Edit `.env` and set `MUSIC_LIBRARY_PATH` to your local music directory:
   ```env
   MUSIC_LIBRARY_PATH=/path/to/your/music
   ```

4. **Start the services**:
   ```bash
   docker-compose up -d
   ```

5. **Access the application**:
   - Frontend: http://localhost:3000
   - Mopidy HTTP API: http://localhost:6680
   - Mopidy Web Client: http://localhost:6680/mopidy

### First Time Setup

After starting the services, scan your music library:

```bash
docker-compose exec mopidy mopidy local scan
```

## Development

### Frontend Development

For local development with hot reload:

```bash
# Start Mopidy only
docker-compose up -d mopidy

# Run frontend locally
cd frontend
bun install
bun dev
```

The frontend will be available at http://localhost:5173 with hot module replacement.

### Rebuilding Services

```bash
# Rebuild and restart all services
docker-compose up -d --build

# Rebuild specific service
docker-compose up -d --build frontend
```

## Docker Commands

```bash
# Start all services
docker-compose up -d

# View logs
docker-compose logs -f

# View logs for specific service
docker-compose logs -f mopidy
docker-compose logs -f frontend

# Stop services
docker-compose down

# Stop and remove volumes (WARNING: deletes data)
docker-compose down -v

# Restart a service
docker-compose restart mopidy

# Execute command in container
docker-compose exec mopidy mopidy local scan
docker-compose exec frontend sh
```

## Configuration

### Mopidy Configuration

Edit `mopidy/mopidy.conf` to customize Mopidy settings. After changes:

```bash
docker-compose restart mopidy
```

### Adding Streaming Services

#### Spotify

1. Get credentials from https://developer.spotify.com/dashboard
2. Edit `.env`:
   ```env
   SPOTIFY_ENABLED=true
   SPOTIFY_USERNAME=your_username
   SPOTIFY_PASSWORD=your_password
   SPOTIFY_CLIENT_ID=your_client_id
   SPOTIFY_CLIENT_SECRET=your_client_secret
   ```
3. Restart services:
   ```bash
   docker-compose down
   docker-compose up -d
   ```

#### SoundCloud

1. Get auth token from https://www.mopidy.com/ext/soundcloud/
2. Edit `.env`:
   ```env
   SOUNDCLOUD_ENABLED=true
   SOUNDCLOUD_AUTH_TOKEN=your_token
   ```
3. Restart services

## Project Structure

```
musakoneV3/
├── frontend/           # Preact web application
│   ├── src/           # Source code
│   ├── Dockerfile     # Frontend container
│   └── package.json   # Dependencies
├── mopidy/            # Mopidy configuration
│   ├── mopidy.conf    # Mopidy settings
│   ├── Dockerfile     # Mopidy container
│   └── MOPIDY_API.md  # API documentation
├── data/              # Persistent data (gitignored)
│   ├── music/         # Music library
│   └── mopidy/        # Mopidy data
├── docker-compose.yml # Service orchestration
├── .env.example       # Environment template
└── README.md          # This file
```

## API Documentation

See [mopidy/MOPIDY_API.md](mopidy/MOPIDY_API.md) for complete Mopidy WebSocket API reference.

## Design Specification

See [DESIGN_SPEC.md](DESIGN_SPEC.md) for detailed design principles, architecture, and implementation plan.

## Troubleshooting

### Port Already in Use

If port 3000 or 6680 is already in use, edit `.env`:

```env
FRONTEND_PORT=3001
MOPIDY_HTTP_PORT=6681
```

### Mopidy Can't Find Music

1. Check your `MUSIC_LIBRARY_PATH` in `.env`
2. Ensure the directory exists and contains music files
3. Scan the library:
   ```bash
   docker-compose exec mopidy mopidy local scan
   ```

### Frontend Can't Connect to Mopidy

1. Check Mopidy is running:
   ```bash
   docker-compose ps
   ```
2. Check Mopidy logs:
   ```bash
   docker-compose logs mopidy
   ```
3. Verify `VITE_MOPIDY_WS_URL` in `.env` matches your setup

### View Container Logs

```bash
# All services
docker-compose logs -f

# Specific service
docker-compose logs -f mopidy
docker-compose logs -f frontend

# Last 100 lines
docker-compose logs --tail=100 mopidy
```

## Performance

- **Bundle Size**: < 50KB gzipped
- **First Load**: < 1.5s on 4G
- **Touch Response**: < 50ms feedback
- **60fps**: Smooth scrolling and gestures

## Browser Support

- iOS Safari 14+
- Chrome Android 90+
- Chrome/Edge Desktop (latest)
- Firefox (latest)

## Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Commit changes: `git commit -m "feat(scope): description"`
4. Push to branch: `git push origin feature/my-feature`
5. Open a Pull Request

See [.github/copilot-instructions.md](.github/copilot-instructions.md) for development guidelines.

## License

MIT License - see LICENSE file for details

## Acknowledgments

- [Mopidy](https://mopidy.com/) - Extensible music server
- [Preact](https://preactjs.com/) - Fast 3KB React alternative
- [Bun](https://bun.sh/) - Fast JavaScript runtime
- [ncmpcpp](https://github.com/ncmpcpp/ncmpcpp) - Design inspiration

---

**Made for clubroom music control** 🎵
