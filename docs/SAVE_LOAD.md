# Save/load MVP

## Playable browser demo

Both character selection and the in-world controls expose **Save game**, **Load game**,
**Export save**, and **Import save**. Load/import enter the world at the saved position.

- **Save game** replaces one local browser slot for the selected character.
- **Load game** resumes that slot, including after reloading the page.
- **Export save** downloads the current session as `mmorpg-greyhaven-save.json` without
  changing the local slot. Keep this file to move a save between browsers or machines.
- **Import save** validates and restores a JSON file. It does not overwrite the local
  slot: press **Save game** afterward to retain the imported session locally.

Saves preserve full-precision X/Z position, facing, waystone activation, hat appearance,
the decimal bigint demo tick, and the remaining fractional tick. They do not persist
held keys, renderer objects, camera interpolation, or old presentation snapshots.
Loading clears input, resets/reseeds snapshot history, snaps the camera, and refreshes
the objective from the restored waystone state. The offline demo clock rolls over
to zero at its unsigned 64-bit maximum and clears old presentation history. Thus even
a checkpoint at the maximum tick remains playable and saveable after advancing.
This does not change server tick semantics. The old appearance-only save buttons
and storage namespace remain supported separately; they are not fabricated game saves.

No autosave runs on startup, per frame, or during shutdown. An explicit save replaces
the previous slot; a failed import/load does not mutate the session or delete that slot.
A delayed file import cannot override a newer save/load/export/import command.
When browser storage is unavailable, file export/import still work.

## Boundary and format

This is **offline browser-demo progress**, not an authoritative multiplayer character
record or a `game-server` recovery bundle. It does not create inventory, account,
experience, ownership, or cross-zone persistence. The current browser demo's local
state is saved directly, not reconstructed from a player-visible protocol snapshot.
Rust gameplay, physics-engine movement, and the server/control-plane boundaries are
unchanged. The native host's existing opt-in `MMORPG_RECOVERY_DIR` graceful-restart
recovery remains separate.

The document identifies `format: mmorpg.offline-demo-save`, `version: 1`,
`worldId: greyhaven-outpost-v1`, and the exact selected character ID. Local storage
uses `mmorpg.offline-demo.v1.<characterId>`. Imported files are limited to **8 KiB**
before reading, and decoded UTF-8 content is bounded again before JSON parsing.

Validation rejects unknown/missing fields, unsupported format/version/world, foreign
character IDs, invalid hats, non-boolean waystone state, non-finite/out-of-bounds
coordinates, invalid facing, noncanonical/out-of-range ticks, and tick fractions
outside `[0, 1)`. Changing world geometry or introducing gameplay that needs more state
requires an explicit world/schema compatibility decision rather than silently applying
old progress to incompatible content. JSON saves are editable and are not a security
boundary; an online server must never trust them.

## Regression coverage

`web/tests/demo-save.test.ts` covers complete and deterministic codec round trips,
precision above JavaScript's safe integer range, reload via fresh controllers,
appearance-storage isolation, corrupt/oversized/foreign data, unavailable storage,
quota failures, validation-before-mutation, and async import ordering. It runs with
the existing `bun test` command and adds no packages or CI jobs.

Manual acceptance: move to the waystone, activate it, save, move away, reload the
page, and load. Confirm restored position/hat/objective; export, alter the session,
and import the file. Import corrupt JSON and verify the session is unchanged. Focus
save controls and use Enter/Space: controls must activate normally without moving the
character or triggering the character-selection Enter shortcut.
