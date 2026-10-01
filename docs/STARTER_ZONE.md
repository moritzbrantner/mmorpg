# Greyhaven Vale starter zone

Target: a playable tech demo comparable in *shape* to a classic MMO starter area. A new level-1 character leaves the Greyhaven Outpost hub, fights creatures in surrounding subzones, completes a short quest chain, gains levels and gear, and meets a named boss. Every gameplay outcome stays server-authoritative, deterministic and replay/recovery-complete.

This document is the design contract for that slice. The implementation plan below is tracked as GitHub issues. Each step lands as its own pull request.

## Authority map for the slice

| Concern | Owner | Notes |
| --- | --- | --- |
| Movement intent, facing, jump preconditions | `mmorpg-core` | Physics integration and contact stay in `physics-engine`. |
| Units, health, resources, combat, auras, abilities | `mmorpg-core` | Typed intent commands; the tick resolves outcomes. |
| Creature AI, spawns, respawn, threat, leash | `mmorpg-core` | Deterministic, ordered iteration and a zone RNG in canonical state. |
| Experience, levels, loot, inventory, equipment, vendors, money | `mmorpg-core` | Content tables are immutable and revisioned with the zone. |
| Quests: definitions, progress, rewards, markers | `mmorpg-core` | Markers are per-player projections. |
| Wire encoding of commands, projections, events and sheets | `mmorpg-protocol` | Golden fixtures shared with browser decoders. |
| Snapshot delivery above the datagram budget | `game-server` | Fragmentation is a transport concern, not gameplay. |
| Scenery, terrain relief, prop placement, visual models | `mmorpg-scenery` (new, presentation-only) | Consumed by the native client and, via WASM, the browser. Never by hosts. |
| Rendering primitives, instancing, lights, fog | `3d-lab` | Extended upstream, consumed by exact pin. |
| Browser presentation, HUD, input | `web/` | Presents projections; sends intent; no rule decisions. |
| Native presentation | `mmorpg-client` | Same projections and scenery as the browser. |

The browser demo runs the same `ZoneSimulation` through a WASM build as a local, single-player zone host ([ADR 0002](adr/0002-browser-embeds-zone-simulation.md)). The browser presentation consumes the same player-scoped wire projections that the network host sends. It never reads canonical state.

## Invariants every step keeps

- One writer per zone epoch; fencing, handoff and interest contracts are unchanged.
- Gameplay failures are **outcomes**, not transport errors. A command that is well-formed but not currently allowed, such as an ability on cooldown or an out-of-range target, is accepted by `apply_command`, produces a player-visible feedback event, and never closes the session. Only malformed payloads return errors.
- Discrete intents such as casting, looting or accepting quests are processed during the tick in `(player_id, sequence)` order. They are never applied inside `apply_command` against a partially advanced world.
- All authoritative state enters the canonical snapshot: units, auras, cooldowns, AI state, spawn timers, corpses, loot, inventories, quest progress and the zone RNG state. Recovery at any tick must reproduce the continuation exactly (tested per step).
- Iteration over units is ordered (`BTreeMap` or sorted `Vec`). No `HashMap` iteration, wall clock or floating point decides authoritative state. Integer trigonometry uses a const-generated table.
- Snapshot projections are bounded: a deterministic priority cap on entity count plus a byte budget test. Events per snapshot are bounded, and losing a datagram may drop cosmetic events only. Durable facts such as quest progress, inventory and XP also live in periodically re-sent state.
- Content (collision, creature templates, items, quests, abilities) is identified by the zone content revision. Changing it requires a new revision.

## Coordinates and scale

- 100 simulation units per metre, Y up, 30 Hz ticks.
- The playable square is about 240 m × 240 m, centred on the origin. Invisible boundary walls enclose it; mountains outside are scenery.
- **Walkable ground is physically flat** (y = 0). The pinned translational `World` is AABB-only and has no step-up or heightfield support. Terrain relief inside the walkable area is presentation-only, at most about 0.6 m. Both clients offset units vertically from the same shared relief function in `mmorpg-scenery`, and that function is flattened under buildings, roads and plazas. Walkable slopes would need a physics-engine heightfield or step-up controller. That is recorded as a foundation follow-up, not faked in core.
- Yaw is a `u16` angle (65 536 = one turn). Yaw 0 faces +Z and increasing yaw turns toward +X: `direction = (sin yaw, cos yaw)`.

## World layout

| Subzone | Approximate centre (m) | Content |
| --- | --- | --- |
| Greyhaven Outpost (hub) | (0, 20) | Keep, inn, smithy, houses, well, palisade, waystone, graveyard/spirit healer, quest givers, vendor, trainer, patrolling guards |
| Wolfrun Woods | (-75, 0) | Dense trees; Timber Wolves (L1–2), Young Boars (L1–2) |
| Millbrook Farm | (70, 35) | Fields, barn, windmill; Field Marauders (L2–3), Grain Rats (L1) |
| Stillwater Lake | (55, -55) | Shallow walkable lake, reeds; Mirefin Lurkers (L3) |
| Redbrand Hollow | (0, -90) | Mine entrance in the northern cliffs, tents, campfire; Redbrand Bandits (L3–5); Garrick Redbrand (L5 elite, named) |

The road network connects the hub to each subzone. Spawn density targets about 60 creatures zone-wide, with 5–15 relevant to a player at any time.

Map north is −Z (the Redbrand cliffs); yaw 0 faces +Z. Content revision 5 (`mmorpg_core::greyhaven_vale`) implements this layout: new characters spawn on a 32 × 16 grid, 1 m apart, on the collider-free hub plaza; roads are open corridors with no collider within 2 m of a centre line; the five subzones are named areas with disjoint bounds. `mmorpg-scenery` derives the visuals from those colliders. Revision 4 added item content; revision 5 binds ordinary loot rules. Both preserve revision 3’s AI/combat seed. Revision 3 added the creatures and NPCs: 59 spawns of the seven templates below (14 Timber Wolves and 6 Young Boars in Wolfrun Woods, 6 Grain Rats and 10 Field Marauders at Millbrook Farm, 8 Mirefin Lurkers at Stillwater Lake, 14 Redbrand Bandits and Garrick Redbrand in Redbrand Hollow), the hub's quest givers, innkeeper, spirit healer and four gate guards, and the graveyard.

| Template | Family | Level | Health (+ per level) | Damage | Swing (ticks) | Behaviour |
| --- | --- | --- | --- | --- | ---: | --- |
| Timber Wolf | Wolf | 1–2 | 42 (+14) | 2–4 (+1 per level) | 60 | aggressive |
| Young Boar | Boar | 1–2 | 48 (+14) | 2–4 | 66 | neutral |
| Grain Rat | Vermin | 1 | 30 | 1–3 | 48 | neutral |
| Field Marauder | Marauder | 2–3 | 60 (+16) | 3–6 | 66 | aggressive |
| Mirefin Lurker | Mirefin | 3 | 70 | 3–6 | 60 | aggressive |
| Redbrand Bandit | Redbrand | 3–5 | 75 (+16) | 4–7 | 66 | aggressive |
| Garrick Redbrand | Redbrand | 5 elite | 420 | 8–14 | 72 | aggressive, 5 min respawn |

Creatures respawn 60 s after death unless noted and wander within 6 m of their spawn point. These numbers are a starting point for tuning.

## Systems

### Movement

- Command `Move { forward: -1..=1, strafe: -1..=1, facing: u16 }`. The server rotates local intent by `facing` through the integer trig table. Run speed is 21 units/tick (6.3 m/s); a backward component uses 13 units/tick. Facing is intent and has no collision effect.
- Command `Jump`: takes effect only when grounded. A thin overlap query just below the feet decides that; touching any other body (geometry or another unit) counts, side contact with a wall does not. Vertical velocity is set to a constant, and physics integrates gravity. The next tick consumes the intent either way, so an airborne jump is ignored rather than buffered.
- Moving interrupts casts. Dead units cannot move.

### Units and combat

- A unit is a player, creature or NPC. `EntityRef = Player(u32) | Creature(u32) | Npc(u32)` on the wire as `kind: u8, id: u32`. Physics body IDs occupy disjoint namespaces per kind.
- Unit state: level, health/max, resource kind/value/max, faction, target, cast state, auras (bounded), combat flag, death state.
- Players: `SelectTarget(EntityRef | none)`, `StartAutoAttack`, `StopAttack`, `UseAbility { ability, target }`. The server validates range, target and resource.
- Auto-attack uses weapon swing timers. Damage is `base + level scaling` with deterministic variance from the zone RNG. Crits apply ×1.5.
- Death: creatures leave a lootable corpse. Players drop to a dead state and `ReleaseSpirit` respawns them at the graveyard with half health. Health and resource regenerate out of combat.

### Classes (level 1–10)

| Class | Resource | Abilities (unlock level) |
| --- | --- | --- |
| Warden (tank/melee) | Rage (gain on hit dealt/taken, decays out of combat) | Heroic Strike (1), Shield Bash: interrupt + 2 s stun (2), Rallying Cry: self heal-over-time (4), Cleave: frontal AoE (6) |
| Ranger (ranged physical) | Focus (regenerates) | Aimed Shot (1), Serpent Sting: DoT (2), Concussive Shot: snare (4), Rapid Fire: haste buff (6) |
| Arcanist (caster) | Mana (regenerates, faster out of combat) | Firebolt: 2.0 s cast (1), Frost Nova: AoE root (2), Arcane Barrier: absorb shield (4), Blizzard: channelled AoE (6) |

There is a 1.5 s global cooldown. Casts have a cast time, and moving or being stunned interrupts them. Auras have tick-precise durations and periodic effects.

### Creature AI

- States: `Idle` (wander within a radius using the zone RNG), `Engaged` (chase to melee or cast range, attack on the swing timer), `Evading` (leash beyond 40 m from spawn or no valid threat: run home, reset to full health, ignore damage), `Dead` (corpse until timeout, then a respawn timer; claiming rewards does not shorten this schedule).
- Aggro radius is 10 m ± 1 m per level difference, clamped. Nearby same-family creatures assist when one is engaged.
- The threat table is ordered by `(threat desc, entity asc)`.
- Steering is direct toward the target. Physics blocks obstacles; no navmesh in this slice.

### Progression, loot and economy

- Experience curve for levels 1–10. Kill XP scales by level difference; quest XP is fixed. Level-up raises health, resource and base damage.
- Loot tables per creature template: money plus weighted item drops and quest items (only while the quest is active). The corpse is lootable by its tapper (the first player to damage it).
- Inventory: 16 slots with stackable items. Equipment slots: main hand, off hand, head, chest, legs, feet. Gear adds stats (stamina → health, strength/agility/intellect → class damage).
- A vendor NPC buys and sells. Money is copper, `u32`.

### Quests

- Objective kinds: kill N of template, collect N items (drops), talk to NPC, explore area.
- The quest log holds at most 10 entries. Accept and turn-in happen at NPCs, and prerequisites form chains. Rewards: XP, money, one item choice.
- NPC markers are per-player: `!` available, `?` complete, grey `?` in progress.
- Chain (draft): *Trouble in the Woods* (kill 6 wolves) → *Pelts for the Tanner* (collect 5 wolf pelts) → *The Missing Farmhand* (talk at Millbrook) → *Marauders in the Fields* (kill 8 marauders) → *Scout the Lake* (explore) → *Mirefin Menace* (kill 6 lurkers) → *Into Redbrand Hollow* (kill 10 bandits) → *Garrick Redbrand* (named boss) → *Return to Greyhaven*.

### Player-scoped projection

- Header, then a **self** section: exact health/resource/xp/level/target/cast/GCD/cooldowns/auras.
- **Entities**: nearest relevant units within the interest radius (45 m), capped by count in `(distance, kind, id)` priority, and always including the player's current target. Relevance keeps at most 64 units; records are 21 bytes, and budget-driven packing writes as many as fit one datagram (a 1,077-byte budget: 47 records without events/sheet, 44 with a bag, 33 beside a bag and full event section, 32 beside bag/loot and full events); see [PROTOCOL.md](PROTOCOL.md#datagram-byte-budget). `game-server` now fragments oversized session frames, while a future MMO wire change must revise the projection budget for additional sections. Each record carries kind, id, template/appearance, position, velocity, facing, level, health percent and flags (dead, in combat, hostile, attackable, tapped by other, evading, targets the viewer, lootable by the viewer); later steps add quest-marker and casting flags and cast progress. Instead of a per-entity target, a target section carries the viewer's target-of-target.
- **Events**: bounded feedback such as damage/heal/miss, XP, loot, quest updates, level-up and errors ("Out of range", "Not enough mana"). They are cosmetic and may be lost.
- **Sheet**: money, inventory, equipment and quest log. It is included when changed and periodically every 10 ticks, so loss self-heals within about 330 ms.
- **Names**: player display names are sent in a periodic section. Creature and NPC names come from content by template ID.

### Presentation

Presentation never feeds back into gameplay; it reads projections and the Rust scenery export and sends intent.

- **Scenery export** (`mmorpg-wasm`, format `mmorpg.scenery` v3): the terrain grid (4 m, ±200 m), a far ring of the same relief (20 m, ±600 m), a prop kind table with compact per-prop records (kind, feet anchor on the relief, yaw, scale, body box), every structure's exact collider box, roads, water, areas and an independent presentation fingerprint for resource keys. The saved Outpost grass mask is presentation-only. `mmorpg-scenery` stays presentation-only; hosts never link it.
- **Browser world** (step 12a): terrain meshes per colour with soft biome transitions and grass tones; roads, plaza, field and shore as smooth surfaces; hazed far ranges above a 50 m snow line; procedural low-poly models for every prop kind merged per chunk, colour and cull class, with walls at body height on their colliders; a translucent lake; a CSS sky behind a transparent canvas; animated humanoids with class gear through a unit model registry keyed by entity kind, whose models read the whole projected entity and place feet by their own body height; a camera that stays above the ground, eases its zoom and turns the character on a right drag; a circular minimap; and an F3 overlay with node counts and renderer work.
- **Waiting for 3d-lab**: fog, sky, sun and lighting control (#84) replace the baked distance-haze bands and let the day cycle change lighting instead of only CSS sky colours; vertex colours, emissive materials and instancing (#82) replace per-colour batches, colour-only glows (lamps, waystone runes, the staff orb) and duplicated tree and grass geometry. `EnvironmentStyle` and the batching module are the seams; the 3d-lab pin is not bumped for this step.
- **Creature and NPC models** (step 12b, browser, `web/src/world/units/creature-models.ts`): the registry's creature and NPC entries pick a procedural low-poly body by catalog family or NPC role. Wolves, boars and rats are quadrupeds with a diagonal-pair trot (`quadrupedPoseFor`: phase from distance travelled, reach from horizontal speed, idle breathing at rest) that roll onto their side when dead; Field Marauders, Mirefin Lurkers, Redbrand Bandits, Garrick (the elite template, larger through its box and a darker cloak) and the guard, vendor, quest giver and spirit healer are compact humanoids posed by `poseFor` that lie flat when dead. Each model fits its template's collision box (NPCs the 0.6 m × 1.8 m character box) within 10% and draws at most 24 nodes with its target ring. The disposition colour stays an accent (animal eyes and collar, humanoid clothing) and corpses keep the shade; unknown templates fall back to the placeholder box.
- **Later presentation**: spell effects, selection circles beyond the target ring and nameplates, and native client parity (step 13).

## Implementation plan

Each step is one issue and one PR, validated by the full gate from `AGENTS.md`. Protocol changes update `docs/PROTOCOL.md` and the shared fixtures in the same PR.

1. **Refresh pinned foundations** (#4): game-server, physics-engine, 3d-lab crates/renderer, winit, tempfile, three, TypeScript and Vite patch releases, with compatibility evidence.
2. **Design contract**: this document, ADR 0002, roadmap section and tracking issues.
3. **Snapshot fragmentation in game-server** (#18): oversized player snapshots are split into bounded datagram fragments and reassembled by clients. This removes the connection-closing cliff before projections grow. Bump the pin here.
4. **Movement v3** (#19): facing, camera-relative movement, backpedal, jump, run speed, integer trig; command wire v2, snapshot v3 with entity kinds and facing. Native and browser decoders updated.
5. **Greyhaven Vale content** (#20): larger zone definition (content revision 2) with colliders, the player spawn grid, road corridors and named subzones; the `mmorpg-scenery` crate (props, relief, water); interest radius, projection cap and byte-budget test. Native client renders scenery, and the browser's WASM `scenery()` export maps it. Creature spawn tables and NPC placement moved to step 7 (content revision 3); steps 9 (vendor) and 10 (quest givers) give the placed NPCs their behaviour, each with a new content revision.
6. **Browser runs the shared simulation** (#21): `mmorpg-wasm` local host, build pipeline and Pages workflow. The web demo sends commands and renders decoded projections plus Rust scenery, and its duplicated illustrative rules are removed. *Landed before step 5* with a `scenery()` export that was a blockout of the hosted outpost's core colliders; step 5 mapped `mmorpg-scenery` and the vale's areas into the same versioned export without touching the browser render loop. World progress saves were removed until the composed character/world save flow (#40) exists, using the character record from step 15 (#30).
7. **Units, combat and creature AI** (#22), in two pull requests. *7a*: `ZoneContent` with content identity, creature spawn tables and NPC posts, creatures, targeting, auto-attack, death/respawn, regen, threat/leash/assist, zone RNG, events, snapshot v5, the WASM content catalog and minimal browser and native presentation (placeholder bodies, Tab/F/R, text status). *7b*: browser target frame, nameplates, combat text.
8. **Classes and abilities** (#23): resources, GCD, cooldowns, casts, auras, the ability kit above; action bar and cast bars.
9. **Progression, loot, inventory, equipment, vendor** (#24): XP/levels, loot windows, bags, character pane, vendor NPC placement and window.
10. **Quests** (#25): quest-giver NPC placement, definitions, NPC dialog, log, tracker, markers, chain and boss.
11. **Starter-zone workload evidence** (#26): deterministic multi-player combat workload with work counters and a snapshot-byte ratchet (BENCH-016).
12. **World presentation** (#27): terrain relief, instanced vegetation, water, sky, fog and day/night (3d-lab renderer extensions), procedural animated character and creature models, spell effects, selection circles, minimap. *Part A landed for the browser* with the pinned renderer (see [Presentation](#presentation)); part B adds effects and the 3d-lab #82/#84 features. *Browser creature and NPC models landed* (see Creature and NPC models above); native models remain step 13.
13. **Native client parity** (#28): units, health bars, targeting, abilities, orbit camera and HUD over the same projections.
14. **Browser online mode** (#29): a WebTransport session to a local zone host for real multiplayer from browser tabs; Pages stays offline.
15. **Durable character record** (#30): identity, class, level, XP, inventory, equipment, quests and location behind core command/query APIs. **Composed demo save/load** (#40) persists that record together with versioned zone/world checkpoints in an atomic save-slot bundle, then restores both to continue gameplay.
16. **Zone chat and emotes** (#31): bounded, rate-limited `/say`, `/yell` and emotes as zone-local events.

Steps 3 and 12 (renderer work) run in parallel with gameplay steps. Later steps may be re-sliced when evidence says so. Any foundation defect found on the way is fixed upstream and pinned here.
