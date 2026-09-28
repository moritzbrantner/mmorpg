# Save/load and character inspection

## Playable browser demo

Both character selection and the in-world controls expose **Save game**, **Load game**,
**Export save**, and **Import save**. Load/import enter the world at the saved position.

- **Save game** replaces one local browser slot for the selected character.
- **Load game** resumes that slot, including after reloading the page.
- **Export save** downloads the current session as `mmorpg-greyhaven-save.json` without
  changing the local slot. Keep this file to move a save between browsers or machines.
- **Import save** validates and restores a JSON file. It does not overwrite the local
  slot: press **Save game** afterward to retain the imported session locally.

The checkpoint summary displays the saved appearance, position, and waystone state.
Load is disabled when the slot is missing, corrupt, or unavailable. Merely opening
selection never restores or writes a save. Storage is queried on explicit transitions
and relevant other-tab storage events, not in the render/movement loop.

**Characters**, or **Escape** with world focus, returns to selection and pauses the
current session. **Resume exploration** continues it without resetting position,
objective, appearance, or clock. This is distinct from **Load game**, which replaces
the session with the saved checkpoint. Returning does not automatically save progress.
Appearance-only saves remain available in a collapsed section for existing users.
Click the world to return keyboard focus to movement after using its save controls.

Saves preserve full-precision X/Z position, facing, waystone activation, hat appearance,
the decimal bigint demo tick, and the remaining fractional tick. They do not persist
held keys, preview rotation, renderer objects, camera interpolation, or old snapshots.
Loading clears input, resets/reseeds snapshot history, snaps the camera, and refreshes
the objective from the restored waystone state. The offline demo clock rolls over
to zero at its unsigned 64-bit maximum and clears old presentation history. Thus even
a checkpoint at the maximum tick remains playable and saveable after advancing.
This does not change server tick semantics.

No autosave runs on startup, per frame, or during shutdown. An explicit save replaces
the previous slot; a failed import/load does not mutate the session or delete that slot.
A delayed file import cannot override a newer save/load/export/import command,
character customization, or navigation between selection and the world.
When browser storage is unavailable, file export/import still work.

## Turn your character

Drag horizontally over the 3D preview with the primary mouse button or one finger.
One preview-width of dragging is one full revolution. **Turn left/right** rotate by
15 degrees. Focus the preview and use arrow keys, **Home** to face forward, or **End**
to reach 359 degrees. **Reset view** returns to the front-facing view.

Rotation is a presentation-only turntable. All attached equipment follows the same
3D transform; it never changes gameplay facing or the saved character/world state.
The angle stays fixed until user input, including after changing a hat. A cancelled
drag, lost pointer capture, blur, or switching screens rolls back that unfinished
gesture; unrelated pointers cannot steal or finish it. Completed rotation remains
when you return to selection within the same page session. Reload starts front-facing.

On narrow displays, selection stacks vertically with a dedicated preview above the
controls; vertical touch scrolling is retained. The camera is fitted to that preview
on resize/scroll, without measuring layout in the animation loop.

## Boundary and format

This is **offline browser-demo progress**, not an authoritative multiplayer character
record or a `game-server` recovery bundle. It does not create inventory, account,
experience, ownership, or cross-zone persistence. Local state is saved directly,
not reconstructed from a player-visible protocol snapshot. Rust gameplay,
physics-engine movement, and server/control-plane boundaries are unchanged. The
native host's opt-in `MMORPG_RECOVERY_DIR` graceful-restart recovery remains separate.

The document identifies `format: mmorpg.offline-demo-save`, `version: 1`,
`worldId: greyhaven-outpost-v1`, and the exact selected character ID. Local storage
uses `mmorpg.offline-demo.v1.<characterId>`. Existing v1 saves remain compatible.
Imported files are limited to **8 KiB** before reading, and decoded UTF-8 content is
bounded again before JSON parsing.

Validation rejects unknown/missing fields, unsupported format/version/world, foreign
character IDs, invalid hats, non-boolean waystone state, non-finite/out-of-bounds
coordinates, invalid facing, noncanonical/out-of-range ticks, and tick fractions
outside `[0, 1)`. Changing world geometry or adding gameplay state requires an explicit
world/schema compatibility decision. JSON saves are editable and are not a security
boundary; an online server must never trust them.

## Regression and browser evidence

`bun test` covers the codec, storage failures, import ordering, terminal-tick recovery,
read-only checkpoint queries, navigation cancellation, and deterministic turntable
input. It uses the existing dependencies and separates correctness from wall-clock
performance. There are no per-frame persistence reads or writes.

`python3 scripts/smoke-browser.py` exercises the actual production bundle and renderer:
mouse/keyboard/touch rotation, rendered front/reset parity, pointer cancellation,
save/reload/import/export, paused navigation, delayed imports, denied storage, and
mobile layout. It retains screenshots and a JSON error report under `artifacts/browser`.

Build `web/` first, then install the optional runner:

```sh
python3 -m pip install playwright==1.57.0
python3 -m playwright install --with-deps chromium
python3 scripts/smoke-browser.py
```

The Pages workflow reuses its one production build for browser acceptance on main,
manual runs, or PRs carrying **browser-evidence**. Ordinary PRs keep the cheaper
unit/typecheck/build path; no second build or general-purpose CI job is added.
