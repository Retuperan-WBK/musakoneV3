# Browsing and rendering performance

## TUI

- Browse names appear as soon as the folder response arrives. Metadata lookups cover
  visible tracks plus one screen ahead (up to 100 rows), rather than the whole folder.
- At most two batches of 100 metadata URIs are pending. Repeated draws share pending
  lookups; failed/missing metadata keeps the filename until refresh or reconnect.
- Browse requests are deduplicated. Returning to a cached folder restores the cursor
  immediately; obsolete responses cannot clear the current folder's loading state.
- Folder cache: 64 folders / approximately 16 MiB. `u`, `library_changed`, and
  reconnect invalidate browse and metadata state.
- Render caches retain four cover variants (16 MiB) and eight lyric variants
  (8 MiB). Keys include content, dimensions and rendering mode; RGB buffers are shared.
- Unchanged framebuffer overlays skip conversion and writes. Resize and retired-overlay
  repairs force repaint; a five-second refresh also recovers from external fbcon repaints.
- RPC responses move their JSON result rather than cloning it. Disabled payload logging
  avoids formatting; enabled logging truncates safely at Unicode boundaries.

### Rendering measurements

Mac release build, 120×40 cells, 100 warm frames, synthetic 600×600 cover;
framebuffer rendering simulated at 960×640. Same workload before and after:

| Workload | Before | After |
| --- | ---: | ---: |
| Browser, 10,000 folders | 0.250 ms/frame | 0.247 ms/frame |
| Terminal cover | 1.161 ms/frame | 0.155 ms/frame |
| Framebuffer cover + lyric | 2.384 ms/frame | 0.108 ms/frame |

These measure CPU rendering into a test terminal, excluding network latency and actual
framebuffer writes. Cold cache misses still rasterize/resize once. A 20-row track view
now initially requests 40 metadata records instead of all 10,000 in a large directory.

```sh
cd tui
cargo test --locked
cargo test --locked --release rendering_benchmark -- --ignored --nocapture
```

## Web

- Shared in-flight folder requests and a five-minute LRU cache (64 folders / approximately
  8 MiB) avoid repeat network waits. Reconnect and library changes invalidate the cache.
- Cached navigation updates synchronously. Back/breadcrumbs work during loads, saved
  scroll positions survive navigation, and sequence checks reject obsolete results/errors.
- Lists over 500 items render the viewport plus overscan, measuring track/folder heights
  to accommodate fonts and resizing. A 10,000-folder mobile smoke test mounted **21 rows**.
- Playback updates run at 4 Hz instead of 10 Hz, pause in hidden tabs, and allow only one
  position-sync RPC at a time. Failed syncs retry at the normal interval; old replies cannot
  overwrite a seek or a new track.
- Startup no longer disconnects a socket opened by a protected view while user details load.

Cache/navigation regression tests:

```sh
cd frontend
bun test test/library.test.ts
bun run type-check
bun run build
```

Browser smoke checks cover large folder/track lists, Back during a delayed request,
cached root reuse, restored scroll, adding the final track, stale responses, resizing,
and slow playback-sync/seek races. Production JS is approximately 32 KiB gzipped.

Uncached folder response times still depend on Mopidy and its provider; no speculative
folder prefetching is used because slow providers can serialize those requests ahead of
the user's next navigation or playback command.
