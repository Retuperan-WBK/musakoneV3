#!/bin/sh
# Install or update the musakone TUI from the rolling CI release.
set -e
repo="Retuperan-WBK/musakoneV3"
case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch=x86_64 ;;
  arm64|aarch64) arch=aarch64 ;;
  *) echo "unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac
dir="${MUSAKONE_BIN:-$HOME/.local/bin}"
mkdir -p "$dir"
url="https://github.com/$repo/releases/download/tui-latest/musakone-$os-$arch"
echo "downloading $url"
curl -fsSL "$url" -o "$dir/musakone.new"
chmod +x "$dir/musakone.new"
mv "$dir/musakone.new" "$dir/musakone"
echo "installed $dir/musakone"
case ":$PATH:" in *":$dir:"*) ;; *) echo "note: $dir is not on your PATH" ;; esac
