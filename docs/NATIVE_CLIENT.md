# Run the native multiplayer slice

The native client is a Rust executable using wgpu and winit. It connects to the existing zone host over TLS/WebTransport. It renders the shared outpost's physical geometry and interpolated player snapshots. Position and collision outcomes come from the server.

## Prerequisites

Use the pinned Rust 1.98.0 toolchain. The native client needs a supported Vulkan, Metal, Direct3D or OpenGL backend and a desktop window system. Linux builds use the standard winit X11/Wayland dependencies; no GTK/WebKit installation is required. Offscreen GPU verification also works with a software Vulkan implementation.

The smoke helper additionally needs Python 3 and OpenSSL. Rust integration tests use local ephemeral TLS credentials and sockets, and do not require a GPU or display.

## One-command verification

```sh
python3 scripts/smoke-native.py --window
```

The helper builds locked sources, creates a disposable certificate/key, starts a real host on OS-selected ports, waits for readiness, connects the client, resumes the same player on a new connection epoch, and reads back a rendered GPU frame. With `--window`, it then opens a native game window for 120 frames. It stops its processes and removes temporary credentials on success or failure.

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

WASD or arrow keys move. Escape closes. Losing focus clears held movement. The camera follows your gold avatar; other players are blue. Movement is retransmitted at 20 Hz so a lost key-release datagram is corrected. There is no local movement prediction yet, so input response includes network and interpolation delay.

The default endpoint is `https://localhost:4433/game/matches/zone-1`. For another configured zone:

```sh
cargo run --locked -p mmorpg-client -- \
  --url https://localhost:4433/game/matches/zone-2 --zone 2 \
  --certificate target/dev-tls/cert.pem
```

For publicly trusted server certificates, omit `--certificate`; system certificate trust is then used. The client never disables certificate verification. `--smoke` connects, resumes the same session, and verifies a single offscreen GPU frame; `--frames N` limits a window run for lifecycle checks. Invalid configuration, incompatible tick rates/content, and failed connections exit nonzero.

## Connection interruptions

While the window remains open, a transport timeout/local connection loss or five seconds without advancing snapshots triggers one resume attempt. The title shows “reconnecting to world” while the last known scene remains visible. Resume uses the same TLS configuration and endpoint, keeps the same player, retains command sequencing, and resets interpolation. It sends a stopped movement intent and waits for acknowledgement before sending the current held input again.

Tokens stay in memory and never appear in CLI arguments or application logs. Session URLs must name a fresh hosted-match route without credentials, query parameters, or fragments. The server owns token rotation and expiry. Reconnect lasts at most ten seconds or the advertised grace period, whichever is shorter; it includes a 250 ms delay for server-side disconnect processing. That delay is best effort, not an acknowledgement protocol. The retained player may continue its last server-owned movement during the interruption until the stop reaches the server.

A rejected, expired, incompatible, or incomplete resume ends the session with an error. The client never substitutes a new player. Protocol errors and server application rejections are terminal. Closing the window cancels a pending attempt. Resume after closing the application, server restart recovery, and live zone rerouting are not implemented.

## Current limits

- This is a connected gameplay/graphics slice, not a production account system. Sessions are anonymous; account/character binding, persistent resume across application launches, and live zone handoff are pending.
- The host remains standalone; do not run competing hosts for one zone without the planned distributed lease integration.
- Geometry is shared collision-box content, not final art. Combat, inventory and NPC gameplay are not implemented.
- The existing shared transport sends complete snapshots in datagrams. Dense projections above the negotiated packet size fail closed. Transport-level bounded replication/chunking is required before crowded-zone use.
- The built-in outpost changes the standalone host's initial simulation state. Recovery bundles captured with the former empty world cannot be silently reused; arrange an explicit migration or fresh development state.
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

Native resume was implemented from repository baseline `80acad9d0d810fd048019fd824d42a1633774953`. The shared `game-server` pin remains `769de47005cc37891011fc76ae183c18b7c5e0ae`. The client now directly declares the already-locked `url` 2.5.8 parser; no locked package version changed.
