#!/bin/sh
# Local Docker stack and native TUI, isolated from the orchestra server.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

compose() {
    docker compose --project-name musakone-dev --file "$root/docker-compose.dev.yml" "$@"
}

command=${1:-help}
if [ "$#" -gt 0 ]; then
    shift
fi

case "$command" in
    up)
        compose up -d --build --wait "$@"
        # Only talks to the loopback-only development backend. Keep the user on restarts.
        status=$(curl -sS -o /dev/null -w '%{http_code}' \
            -H 'Content-Type: application/json' \
            -d '{"username":"dev","password":"dev"}' \
            http://127.0.0.1:3001/api/auth/register)
        case "$status" in
            201|409) ;;
            *) echo "Could not create the local dev user (HTTP $status)." >&2; exit 1 ;;
        esac
        echo "Web UI: http://127.0.0.1:3000 (login: dev / dev)"
        echo "TUI:    ./dev.sh tui"
        ;;
    tui)
        # Never read or overwrite the normal TUI token, URL, or environment file.
        umask 077
        export XDG_CONFIG_HOME="$root/.dev/config"
        export XDG_CACHE_HOME="$root/.dev/cache"
        export MUSAKONE_URL=http://127.0.0.1:3001
        export MUSAKONE_USER=dev
        export MUSAKONE_PASSWORD=dev
        exec cargo run --locked --manifest-path "$root/tui/Cargo.toml" -- "$MUSAKONE_URL"
        ;;
    preview)
        # Offline framebuffer simulation: no Docker, credentials or playback.
        exec cargo run --locked --manifest-path "$root/tui/Cargo.toml" -- \
            preview --output "$root/.dev/preview" "$@"
        ;;
    scan)
        compose exec mopidy python -m mopidy --config /config/mopidy.conf local scan "$@"
        ;;
    help|-h|--help)
        echo "Usage: ./dev.sh up          Build/start the local stack and scan music/"
        echo "       ./dev.sh tui         Run the native TUI against the local backend"
        echo "       ./dev.sh preview --open   Preview real-font Linux rendering on Mac"
        echo "       ./dev.sh scan        Rescan music/ after adding MP3s"
        echo "       ./dev.sh logs -f     Follow container logs"
        echo "       ./dev.sh down       Stop the stack (preserves local data)"
        echo "Other arguments are passed to Docker Compose (e.g. ps, build, restart)."
        ;;
    *)
        compose "$command" "$@"
        ;;
esac
