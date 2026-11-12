<div align="center">
  <h1>Marshall</h1>
  <p><strong>A fast, native music desktop app — built in Rust.</strong></p>
  <p>Built with egui and rodio. No Electron, no embedded browser player, and light on memory while playing.</p>

  <p>
    <a href="https://github.com/bhagyarajkota45/Marshall/releases/latest"><img src="https://img.shields.io/github/v/release/bhagyarajkota45/Marshall?style=flat-square&color=a855f7&labelColor=0d1117" alt="Latest release"></a>
    <a href="https://github.com/bhagyarajkota45/Marshall/releases"><img src="https://img.shields.io/github/downloads/bhagyarajkota45/Marshall/total?style=flat-square&color=a855f7&labelColor=0d1117" alt="Downloads"></a>
    <img src="https://img.shields.io/badge/platform-Windows-333?style=flat-square&labelColor=0d1117" alt="Platform: Windows">
    <a href="LICENSE"><img src="https://img.shields.io/github/license/bhagyarajkota45/Marshall?style=flat-square&color=333&labelColor=0d1117" alt="License: GPL-3.0"></a>
  </p>

  <p>
    <a href="https://discord.gg/cWQSGjpXM">Discord</a> ·
    <a href="https://github.com/bhagyarajkota45/Marshall/releases/latest">Download</a> ·
    <a href="#features">Features</a> ·
    <a href="#installation">Installation</a> ·
    <a href="#building-from-source">Build from source</a>
  </p>
</div>

<p align="center">
  <img src="docs/readme/screenshot-6.png" alt="Marshall player with artwork, lyrics and controls" width="100%">
</p>

> Marshall is an independent, unofficial client. It is not affiliated with, endorsed by, or connected to YouTube, Google, or Apple.

## Screenshots

<table>
  <tr>
   <td width="50%"><img src="docs/readme/screenshot-4.png" alt="Player view with Cruel Summer lyrics"></td>
<td width="50%"><img src="docs/readme/screenshot-5.png" alt="Browse view with Cinnamon Girl lyrics"></td>>
  </tr>
  <tr>
    <td align="center"><b>Player</b> — artwork, lyrics, queue and playback in one view</td>
    <td align="center"><b>Browse</b> — your music, shelves and navigation</td>
  </tr>
  <tr>
    <td colspan="2"><img src="docs/readme/screenshot-3.png" alt="Marshall interface and playback" width="100%"></td>
  </tr>
  <tr>
    <td colspan="2" align="center"><b>Desktop experience</b> — native UI, fast playback, and quality controls</td>
  </tr>
</table>

## Features

**Playback**
- **YouTube Music, natively** — search, browse, albums, playlists, artist pages, radio, mixes, charts and your library.
- **Lossless / Hi-Res Lossless / Dolby Atmos** — play higher-quality copies from configured hi-res addon sources when a matching copy is available, with YouTube audio as the fallback.
- **Quality controls in the player** — see the active audio quality, prefer Dolby Atmos when available, and revert or upgrade quality from the player menu.
- **Automix and crossfade** — DJ-style transitions between songs, with a mixing state shown in the player during transitions.
- **Equalizer & Sound FX** — a live ten-band curve, Studio Master Clarity processing, genre presets, acoustic target profiles, and automatic headroom.
- **Still high-resolution artwork** — Now Playing keeps the album cover sharp and motion-free.
- **Touchpad controls** — two-finger swipes change tracks and open or close lyrics; the lyrics presentation is unchanged.
- **Queue and playback controls** — shuffle, repeat, autoplay, song radio, next-track prefetch, remove with Undo, volume normalization, output device selection, and a sleep timer.
- **Downloads** — save songs for offline playback.
- **Local music** — scan a selected folder and its subfolders for MP3 and FLAC files, then play them with the shared queue and transport controls.

**Lyrics and interface**
- **Synced lyrics** — timed lyrics with provider fallbacks, timing controls, romanization and translation options.
- **Discovery and history** — genre cards use artist portraits when available; Home can start radio from a recent play; Statistics, Mix Lab and Podcasts use Marshall's existing services.
- **Smart lists and backup** — Recently Played, Played This Week, and Most Played lists update from local history. Export or restore preferences and sidebar playlist organization without account credentials.
- **Separated app toolbar** — a distinct top toolbar and consistent space before page content across the app.
- **Native Rust desktop UI** — a fast egui/rodio interface with system media controls, tray integration, and keyboard-friendly navigation.
- **Discord Rich Presence** — show what you’re listening to on Discord.
- **Last.fm scrobbling** — connect your Last.fm account in Settings.

**Accounts and library**
- **Google sign-in** — unlock your YouTube Music feed, likes, playlists, history and recommendations.
- **Library and uploads** — access saved music and uploaded tracks where available.
- **Spotify transfer** — bring playlists and liked music over from Spotify.

## Installation

Download the latest build from [Releases](https://github.com/bhagyarajkota45/Marshall/releases/latest).

- **Windows** — run the Marshall executable or build it from source with Cargo.
- **macOS / Linux** — use the platform release assets when available.

If Windows SmartScreen appears for an unsigned build, choose **More info → Run anyway**.

## Building from source

You need the [Rust toolchain](https://rustup.rs/).

Open the extracted `Marshall-2.0.5-Update` folder in VS Code, then run `cargo build` in its terminal. Use `cargo run` to launch the app.

The built app will be in `target/release/`.

Platform builds may require native dependencies (audio/system libraries and toolchain components used by sign-in or platform integrations). Check the repository’s platform docs and release workflow for the exact prerequisites.

## Notes

- Audio and the main interface are native (Rust + egui + rodio). Sign-in and some web flows may open a system webview window.
- Hi-res/Lossless/Atmos playback depends on configured addon sources and whether a matching, playable copy exists for the selected track.
- Deezer cookie playback is not supported. Marshall does not use session cookies to bypass a streaming service's playback protections.
- Marshall does not bypass subscriptions, DRM, or regional restrictions. Availability depends on the services and sources you use.
- This customized build does not check for, download, or install upstream app updates. Update it manually from a trusted Marshall package.
- Third-party services (YouTube Music, lyrics providers, Discord, Last.fm, Spotify, addon sources) can change behavior over time.

## Contributing

Bug reports and pull requests are welcome. Please open an issue with clear reproduction steps, platform details, and relevant logs (with secrets/tokens removed).

## License

Marshall is licensed under [GPL-3.0](LICENSE).

## Acknowledgements

- [egui](https://github.com/emilk/egui) for the interface.
- [rodio](https://github.com/RustAudio/rodio) and Symphonia for audio playback and decoding.
- Lyrics, metadata, playback, and integrations depend on the upstream libraries and third-party services used by this project.

