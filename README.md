# RustTube

A fast, native YouTube client for macOS and Windows, written in Rust.
The interface (search, feeds, channels, comments, subscriptions, history) is drawn
with [egui](https://github.com/emilk/egui). Videos play in YouTube's **official embedded
player**, hosted in the system web view (WKWebView on macOS, WebView2 on Windows) inside
the app window, so playback follows YouTube's terms and keeps working when YouTube changes.

## Features
- Search, Explore categories, channel pages, related videos, comments
- Local profiles (no sign-in): subscribe to channels, subscriptions feed,
  watch history with resume, and a **For You** feed built from your
  subscriptions and what you watch. Data lives in
  `~/Library/Application Support/RustTube/profiles.json` (macOS) or
  `%APPDATA%\RustTube\profiles.json` (Windows).
- Player controls (quality, captions, speed, fullscreen) come from YouTube's player.

## Build from source
macOS: `./scripts/package-macos.sh` → `dist/RustTube-*.dmg` (or just `cargo run --release`).

Windows: pushed tags (`v*`) or a manual run of the **Build installers** workflow produce
`RustTube-*-windows-x64-setup.exe` (NSIS; installs the WebView2 runtime if missing) and the macOS `.dmg` files.

## Notes
- Some videos (often official music videos) are not allowed to play outside YouTube;
  RustTube shows YouTube's message and an **Open on YouTube** button.
- Feeds, search and comments use YouTube's internal (unofficial) web API, which can change
  without notice; see `src/yt.rs`.
- The first launch of an ad-hoc-signed macOS app: right-click → Open.
- Debugging: `RUSTTUBE_AUTOPLAY=<video id>` opens a video at start; `RUSTTUBE_DEBUG=1` logs player state.
