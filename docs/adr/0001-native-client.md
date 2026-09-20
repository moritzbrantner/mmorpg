# Native game client; Tauri remains a launcher option

Status: accepted for the first connected playable slice.

The project already has Rust gameplay, deterministic physics and a WebTransport server. Its browser demo is useful for presentation experiments, but does not execute the same world or physics as the server.

Use a native `mmorpg-client` executable with winit for desktop window/input lifecycle and wgpu for GPU rendering. Consume the existing renderer-independent mesh and camera contracts from `3d-lab`. Keep physical truth in the server, send movement intent through the shared session protocol, and render only player-visible snapshots. Share immutable collision geometry and player dimensions between server and native presentation.

Tauri provides a Rust application core and system webviews. It would be useful for launcher/login/settings/patch-management workflows, but those workflows do not yet exist here. A Tauri shell is not a substitute for a native renderer or distributed authority. Introducing webview IPC into the per-frame simulation/render path would add another ownership/lifecycle problem without providing a needed capability in this slice. If a launcher is added, it should use a narrow launch/configuration contract while the native game process retains GPU, input and connection ownership.

This decision leaves the existing browser demo available. No platform packaging or production account authentication is implied by a native executable. The first native renderer intentionally draws the shared physical scene as boxes; authored meshes, materials, animation and LOD remain renderer/content work.

Sources: [Tauri architecture](https://v2.tauri.app/concept/architecture/), [Tauri process model](https://v2.tauri.app/concept/process-model/), [wgpu](https://wgpu.rs/). The exact native graphics dependency pin is recorded in the client manifest and workspace lockfile; the existing browser renderer pin is unchanged.
