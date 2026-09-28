# Character creation MVP

The browser demo supports a local character roster for testing the character-selection and save/load flow without pretending that the browser owns authoritative MMO accounts.

## Creation flow

From **Choose your character**, select **Create character** and provide:

- a 2–24 character name;
- one of exactly three starter classes: **Warden**, **Ranger**, or **Arcanist**;
- **Male** or **Female** presentation.

The 3D preview updates before creation and remains rotatable. Class changes update the starter equipment and main-hand presentation: Warden uses a sword, Ranger a bow, and Arcanist a staff. Sex changes the presentation frame only; it does not change gameplay statistics.

Created characters start at level 1 in Greyhaven Outpost. The original Aelric Stormward preview remains a built-in level-18 character.

## Local identity and persistence

Created characters receive stable local IDs such as `local-1`. The browser stores only the created roster in the versioned `mmorpg.offline-roster.v1` record; the built-in character is not duplicated into storage.

Each character ID owns its own:

- paused in-memory demo session;
- appearance-only save key;
- full game checkpoint key;
- import/export identity check.

Switching characters stores the current paused session in memory, activates the selected character's own session, and refreshes save/load controls for that identity. A checkpoint for one character cannot be loaded into another.

Roster parsing fails closed for unsupported schemas, malformed records, duplicate IDs, duplicate names (including the built-in character), invalid class/sex values, excessive slots, and oversized data. If roster storage is unavailable or corrupt, the existing bytes are not silently replaced; newly created characters remain session-only.

## Authority boundary

This is local browser-demo state. It does not add account creation, durable multiplayer character records, class combat mechanics, inventory authority, or server-side character persistence. Those remain future server-owned boundaries. The feature does not change `mmorpg-core`, `physics-engine`, `game-server`, or control-plane authority.
