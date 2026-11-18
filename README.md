# RustTube

A fast, native YouTube client for macOS and Windows, written in Rust.
The interface is drawn with [egui](https://github.com/emilk/egui) and videos
play through [libmpv](https://mpv.io) (hardware decoding) rendered directly
into the window. There is no browser or webview.

## Features
- Search, Explore categories, channel pages, related videos, comments
- Playback: quality picker (up to 4K source, defaults to ≤1080p), speed, volume,
  seek bar, fullscreen, keyboard shortcuts
- Local profiles (no sign-in): subscribe to channels, subscriptions feed,
  watch history with resume, and a **For You** feed built from your
  subscriptions and what you watch. Data lives in
  `~/Library/Application Support/RustTube/profiles.json` (macOS) or
  `%APPDATA%\RustTube\profiles.json` (Windows).

Shortcuts (on a video): Space/K pause · ←/→ ±5s · J/L ±10s · ↑/↓ volume · M mute · F fullscreen · Esc exit fullscreen

## Build from source
macOS: `brew install mpv dylibbundler && ./scripts/package-macos.sh` → `dist/RustTube-*.dmg`
(For a plain dev run: `LIBRARY_PATH=/opt/homebrew/lib DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib cargo run --release`.)

Windows: pushed tags (`v*`) or a manual run of the **Build installers** workflow produce
`RustTube-*-windows-x64-setup.exe` (NSIS) and the macOS `.dmg` files.

## Notes
- The first macOS launch of an ad-hoc-signed app: right-click → Open.
- Streams come from YouTube's internal (unofficial) API, which can change without notice.
  If playback breaks, `src/yt.rs` (`streams`) is the place to update.
- Debugging: `RUSTTUBE_AUTOPLAY=<video id>` opens a video at start; `RUSTTUBE_DEBUG=1` logs player state.
