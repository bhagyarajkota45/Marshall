# Marshall 2.0.5 update

This is a source ZIP for VS Code. It keeps Marshall's existing navigation, lyrics view, player, library, integrations, and GPL-3.0-only license.

## Included

- A distinct, lightly surfaced toolbar and a consistent gap before page content across the app.
- Search genre tiles fetch portraits from Marshall's own artist search results, with genre-tinted overlays and a colored fallback while an image is unavailable.
- Removed the Last.fm connect/disconnect card from Statistics. Last.fm connection remains in Settings.
- Rebuilt Equalizer & Sound FX around the supplied reference: 10-band sliders, curve preview, preset chips, quick bass/treble controls, four target profiles, and a Studio Master Clarity toggle. The clarity curve is processed by Marshall's existing biquad audio path.
- Home offers a radio starting from the listener's latest local play. “Not for me” hides that seed and moves to another recent pick.
- Settings › Library now has live Recently Played, Played This Week, and Most Played rule-based lists, plus export/import of Marshall preferences and sidebar playlist organization.
- Existing Marshall additions remain: static high-resolution covers, four two-finger trackpad gestures, sleep timer, queue Undo, thin scrollbars, and Marshall branding.

## Limits

- Deezer is not integrated. No session cookie was read, copied, or placed in this package. Deezer's developer terms list catalog API services and explicitly prohibit bypassing its technical protections; the cookie file therefore cannot be used to unlock lossless streams. [Deezer developer terms](https://cdn-content.dzcdn.net/pdf/CGU-developers.pdf)
- The supplied third-party source archive had no clear redistribution terms. No code or artwork from it was copied.
- Recommendations use the service's radio from a recent song, not a custom genre classifier. “Not for me” dismisses the local starting song; it does not train the remote service.
- Smart playlists are three automatic local-history rules, not a custom rule editor. Backups include preferences, recent searches, hidden/order state, discovery feedback, and cached playlist cards. Account cookies, keys, and credentials are excluded; your actual service playlists remain in your account.
- Portrait tiles depend on the artist search service and internet access. A colored tile remains if no portrait is returned.

## Verification

- `cargo check --offline --target-dir ..\validation-target` and `cargo build --offline --target-dir ..\validation-target` completed without compiler diagnostics.
- 161 tests passed; 25 are marked ignored by the project, and 3 Windows registry/secure-storage tests were filtered because this environment denies those operations. The new smart-playlist and queue-undo tests passed.
- These checks do not verify a physical touchpad, live service availability, or appearance on the user's display.

## Build in VS Code

Extract the ZIP, open `Marshall-2.0.5-Update` in VS Code, then run `cargo run` in the terminal. The Rust toolchain and platform build prerequisites are required.
