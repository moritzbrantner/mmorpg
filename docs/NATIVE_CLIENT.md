# Run the native multiplayer slice

The native client is a Rust executable using wgpu and winit. It connects to the existing zone host over TLS/WebTransport. It renders Greyhaven Vale from `mmorpg-scenery` (a relief terrain mesh with biome colours, the lake surface and prop blockouts whose structure boxes are the server's exact colliders) and interpolated snapshots of players, creatures and NPCs. Position and collision outcomes come from the server; units are drawn at their physics position plus the shared presentation relief under them.

## Prerequisites

Use the pinned Rust 1.98.0 toolchain. The native client needs a supported Vulkan, Metal, Direct3D or OpenGL backend and a desktop window system. Linux builds use the standard winit X11/Wayland dependencies; no GTK/WebKit installation is required. Offscreen GPU verification also works with a software Vulkan implementation.

The smoke helper additionally needs Python 3 and OpenSSL. Rust integration tests use local ephemeral TLS credentials and sockets, and do not require a GPU or display.

## One-command verification

```sh
python3 scripts/smoke-native.py --window
```

The helper builds locked sources, creates a disposable certificate/key, starts a real host on OS-selected ports, waits for readiness, connects the client, resumes the same player on a new connection epoch, and reads back a rendered GPU frame of the vale, which must contain several colours with no single colour (such as the sky) covering three quarters of it. It also runs the explicit GPU health-bar test: restore two authoritative fixtures differing only in one creature's health, project them through core, and require visible pixel changes. With `--window`, it then opens a native game window for 120 frames. It stops its processes and removes temporary credentials on success or failure.

## Interactive play

Start a local host and connect one native client with one command:

```sh
./scripts/dev-native.sh
```

The launcher builds the native packages, reuses a valid disposable certificate under `target/dev-tls/` (or creates one valid for 10 days), starts zone `1` at `https://localhost:4433`, waits for `/readyz`, and opens the client. Closing the window or pressing Ctrl-C stops the host. It owns that host process, so do not use it while another local zone host is bound to ports `4433` or `8080`.

To exercise the exact launcher without leaving a window open:

```sh
./scripts/dev-native.sh --smoke
```

Run this command in another terminal for a second player after the launcher is ready:

```sh
cargo run --locked -p mmorpg-client -- --certificate target/dev-tls/cert.pem
```

Controls:

| Input | Action |
| --- | --- |
| W / S (or ↑ / ↓) | Run forward / backpedal |
| A / D, Q / E (or ← / →) | Strafe left / right |
| Space | Jump (only from the ground) |
| Tab | Select the nearest living attackable creature; press again to cycle outward |
| F | Start auto-attacking the target, or stop |
| R | Release your spirit while dead |
| Left or right mouse drag | Orbit the camera around your character |
| Mouse wheel | Zoom the camera |
| Escape | Close |

The third-person camera orbits your gold avatar; other players are blue. Each body shows a small dark nose on its facing side. While a movement key is held, your character turns to face the camera's direction, so W always runs away from the camera; releasing the keys leaves the character facing where it last moved. Running and strafing move at 6.3 m/s, backpedalling is slower. Losing focus clears held movement. Movement is sent on change and retransmitted at 20 Hz so a lost key-release datagram is corrected; each Space, Tab, F or R press sends one command and is never replayed after a reconnect.

Creatures and NPCs are placeholder boxes sized by their collision boxes, with the same facing nose: hostile creatures red-ish, neutral ones yellow-ish, friendly NPCs green (guards darker), creatures tapped by another player grey. Corpses lie flat and darkened, and a gold marker stands under your target. Living players and creatures show a green health bar above their rendered bodies, aligned with the orbit camera's horizontal direction. Its fill uses the received quantized health percentage and follows the interpolated snapshot sample; NPCs and corpses omit bars. The window title shows your health, combat state, target (name from the zone content, level and health percent) and whether you are attacking, or that R releases your spirit. Targeting and attacking are intents: the zone validates range and target and reports refusals as events. There is no local movement prediction yet, so input response includes network and interpolation delay.

The default endpoint is `https://localhost:4433/game/matches/zone-1`. For another configured zone:

```sh
cargo run --locked -p mmorpg-client -- \
  --url https://localhost:4433/game/matches/zone-2 --zone 2 \
  --certificate target/dev-tls/cert.pem
```

For publicly trusted server certificates, omit `--certificate`; system certificate trust is then used. The client never disables certificate verification. `--smoke` connects, resumes the same session, and verifies a single offscreen GPU frame; `--frames N` limits a window run for lifecycle checks. Invalid configuration, incompatible tick rates/content, and failed connections exit nonzero.

## Connection interruptions

While the window remains open, a transport timeout/local connection loss or five seconds without advancing snapshots triggers one resume attempt. The title shows “reconnecting to world” while the last known scene remains visible. Resume uses the same TLS configuration and endpoint, keeps the same player, retains command sequencing, and resets interpolation. It sends a stopped movement intent (keeping the last facing) and waits for acknowledgement before sending the current held input again. Jump presses made during the interruption are dropped.

Tokens stay in memory and never appear in CLI arguments or application logs. Session URLs must name a fresh hosted-match route without credentials, query parameters, or fragments. The server owns token rotation and expiry. Reconnect lasts at most ten seconds or the advertised grace period, whichever is shorter; it includes a 250 ms delay for server-side disconnect processing. That delay is best effort, not an acknowledgement protocol. The retained player may continue its last server-owned movement during the interruption until the stop reaches the server.

A rejected, expired, incompatible, or incomplete resume ends the session with an error. The client never substitutes a new player. Protocol errors and server application rejections are terminal. Closing the window cancels a pending attempt. Resume after closing the application, server restart recovery, and live zone rerouting are not implemented.

## Current limits

- This is a connected gameplay/graphics slice, not a production account system. Sessions are anonymous; account/character binding, persistent resume across application launches, and live zone handoff are pending.
- The host remains standalone; do not run competing hosts for one zone without the planned distributed lease integration.
- Scenery and units are a coloured blockout (boxes for trunks and canopies, walls, roof slabs, creatures and NPCs), not final art; full presentation parity is issue #28. Combat feedback includes projected world-space health bars and window-title status; target frames, nameplates and combat text remain pending. Classes, bag presentation and NPC interaction remain pending; bag intents and self sheets use the shared authority and transport.
- The shared transport fragments oversized session snapshots and the client reassembles them per connection. The current v7 MMO projection policy still keeps at most 64 relevant units and packs as many 21-byte records as fit 1,077 bytes (47 without events/sheet, 44 with a sheet); additional projected sections require a separate protocol and budget change.
- The built-in Greyhaven Vale (content revision 4, with creatures, NPCs and item content) and snapshot v7 change the standalone host's initial simulation state. Recovery bundles captured with earlier content or snapshot versions cannot be silently reused; arrange an explicit migration or fresh development state.
- Linux is the exercised desktop platform in this change. Windows/macOS builds and installers remain unverified.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
python3 scripts/smoke-native.py --window
```

The live convention stack for this change resolved to sourceRevision `e6acb5310afaf15c0cba24f87108f5f4ad1bedc3`. No existing locked package identity was removed during native dependency acquisition.

Native resume was implemented from repository baseline `80acad9d0d810fd048019fd824d42a1633774953`. At that baseline the shared `game-server` pin was `769de47005cc37891011fc76ae183c18b7c5e0ae`; the current pin is listed in the README. The client now directly declares the already-locked `url` 2.5.8 parser; no locked package version changed.

Health-bar slice (#71): rendering keeps the existing instanced-box path with at
most four boxes per admitted unit plus one target marker (257 at the core
projection cap of 64). Oversized presentation input fails before history changes.
No gameplay, projection bytes or scenery identity changes. The optional
`MMORPG_SMOKE_FRAME_DIR` environment variable saves the full/damaged GPU fixtures
as PPM files during native smoke. These are test artifacts, not gameplay state.
Resolved conventions sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
