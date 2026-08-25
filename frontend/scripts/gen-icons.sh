#!/usr/bin/env bash
# Regenerates favicon + PWA icons from the RWBK logo (Retuperän WBK GitHub org avatar artwork).
# Requires ImageMagick 7 (magick) and potrace. Run from frontend/: bun run icons
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=branding/rwbk-logo.png     # black artwork on transparent, 899x445
RED='#FF0E1B'                   # colour used by the github.com/Retuperan-WBK avatar
BG='#000000'                    # app background (matches manifest background_color)
OUT=public
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT

# Red logo on transparent (favicon / any-purpose source)
magick "$SRC" -fill "$RED" -colorize 100 "$TMP/red.png"

# SVG favicon: trace the black artwork, fill with the brand red
magick "$SRC" -background white -alpha remove -threshold 50% "$TMP/mask.pbm"
potrace "$TMP/mask.pbm" --svg --color "$RED" --turdsize 4 -o "$OUT/favicon.svg"

# icon: logo centred on a black square. $1 = size, $2 = logo width as % of size, $3 = out file
icon() {
  local size=$1 pct=$2 out=$3
  local w=$(( size * pct / 100 ))
  magick -size "${size}x${size}" "xc:$BG" \
    \( "$TMP/red.png" -resize "${w}x${w}" \) -gravity center -composite \
    -strip "png32:$out"
}
icon 192 84 "$OUT/icon-192.png"
icon 512 84 "$OUT/icon-512.png"
icon 192 66 "$OUT/icon-maskable-192.png"   # keeps the artwork inside the maskable safe zone
icon 512 66 "$OUT/icon-maskable-512.png"
icon 180 84 "$OUT/apple-touch-icon.png"
magick "$TMP/red.png" -resize 32x32 -gravity center -background none -extent 32x32 -strip "png32:$OUT/favicon-32.png"
ls -la "$OUT"/favicon* "$OUT"/icon-* "$OUT"/apple-touch-icon.png
