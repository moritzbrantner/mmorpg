# Greyhaven combat workload budgets

Issue #26 measures complete zone composition with fixed commands, horizons and
content. It complements the player-only [interest workloads](INTEREST_WORKLOADS.md)
and the [physics adoption comparisons](PHYSICS_WORKLOADS.md). It introduces no new
combat rules, capacity claim or wall-clock correctness gate.

```sh
cargo run -p mmorpg-protocol --example combat_workload --locked
cargo test -p mmorpg-core --test workload_stats --locked
cargo test -p mmorpg-protocol --test combat_workload --locked
```

The example emits four JSON lines under `mmorpg.combat-workload/v1`, including core
and wire versions, the content revision/fingerprint, operation counts, encoded
bytes and a diagnostic trace checksum. The example and regression gate use the
same fixture/measurement module under protocol test support, outside production.

Every fixture runs Greyhaven's 59 creatures, nine NPCs and static colliders for
360 completed ticks (12 seconds of simulation at 30 Hz). Initialization is outside
the counts. `vale-idle-16` admits 16 stationary players on the plaza; creature AI
and wandering remain active, so this is an idle-player control rather than an idle
physics world. The 8/32-player fighting fixtures distribute players across the
authored creature spawn table. The 64-player control concentrates them beside the
six farm rats to exercise crowded discovery and budget packing. Initial player
positions come from a validated canonical fixture, not a new gameplay teleport.

Fighting bots select the nearest living creature every 60 ticks in stable ID
order, start attacking, or release spirit when dead. Core decides whether those
intents are valid. At ticks 31 and 36 they jump/run briefly and stop. The fixed
horizon includes successful blows, refused intents, deaths and continued AI. The
tests require actual damage and death feedback, so silently dropping combat or
shortening the script cannot produce an apparently cheaper passing workload.

Each measured zone has an independently constructed replay with the same content,
zone seed and ordered commands. At tick 180 the replay is destroyed and restored
through canonical recovery. Canonical bytes and every packed player projection
must match directly after every tick, including all 180 ticks after restoration.
Decoding also checks the tick, viewer, events and packed entity prefix. A second
cold run must reproduce every count and the trace checksum. The checksum is
FNV-1a over length-prefixed canonical/projection records; direct byte comparisons
establish parity, not checksum equality alone.

## Counters and cost model

`ZoneSimulation::tick_work()` describes the latest attempted tick. It counts each
living creature AI decision, including idle decisions, and retains the successful
engine step's `StepStats` without duplicating physics instrumentation. A failed
physics step supplies no successful report. Counters start at zero on construction
or recovery and never enter canonical state, RNG, commands or projections.

The harness aggregates AI evaluations and engine-reported admitted body counts,
dynamic bodies, staged bodies, broad-phase pair checks, TOI tests and contact
resolutions. Body visits are the sum of admitted bodies per step, not an allocation
or exact-test metric. Interest maintenance inspections come from core's existing
counter. One projection is produced for every player every tick; candidate tests,
packed entity counts, event records, total bytes and largest payload are measured
at that publication boundary. Damage/death records count player-scoped feedback,
including copies observed by different recipients; they are not unique world events.

AI decisions and controller/maintenance scans grow with living creatures and
players. Physics work also depends on motion and nearby contact pairs; the
translational engine currently stages all admitted bodies on these active worlds.
Projection work grows with recipients and nearby candidates, becoming quadratic
in crowded populations despite the output cap. Encoding grows with packed records
and feedback, bounded per projection by 1,077 bytes. No throughput or memory result
is inferred from those distinct counters.

## Initial v1 ceilings

The deterministic gate allows counts to decrease. Increasing a ceiling requires
reviewing the changed workload, mechanics or performance cost, alongside replay
and recovery acceptance. The complete ceilings live in the protocol test.

| Workload | AI decisions | Physics body visits | Pair checks | Projection candidates | Total projection bytes | Largest bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vale-idle-16 | 21,240 | 120,960 | 170,837 | 190,080 | 3,204,720 | 559 |
| vale-fights-8 | 21,240 | 118,080 | 238,275 | 70,336 | 981,271 | 482 |
| vale-fights-32 | 21,240 | 126,720 | 362,835 | 364,710 | 5,406,543 | 678 |
| vale-crowded-fights-64 | 20,106 | 137,106 | 462,299 | 1,938,628 | 24,499,402 | 1,077 |

The crowded case reaches the full byte budget and publishes 23,040 projections;
its lower AI total reflects killed creatures, not reduced evaluation frequency.
Exact completion/projection counts and workload-specific combat controls accompany
the ceilings. Engine staging, TOI and contact-resolution ceilings are also checked.
Existing scenario/protocol golden fixtures remain unchanged.

Evidence baseline: consumer `676fab2632c209bdaffc9ac0cdce376acb32f484`, physics-engine
`0baf3411419fc250273caec24d64654cb30c28ec`, Rust 1.98.1, native debug profile,
Greyhaven revision 3, core/wire version 5, committed `Cargo.lock` unchanged. The
workload has no clock, network or GPU dependency. It runs in the ordinary workspace
test gate; native/WASM builds continue to use the same Rust authority. Counts are
integer operations and byte lengths, not timings. Resolved shared convention
sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.

Snapshot v6 adds eight self bytes for durable XP. The same 1,077-byte budget
now has a largest attainable 1,071-byte payload. Recovery/replay and workload
ceilings still cover complete ticks; historical v5 byte observations above
remain identified as the original #26 baseline. Physics semantics are unchanged.

The v6 byte ceiling adds at most eight bytes per completed recipient projection
to the reviewed v5 ceiling. Every AI, physics, maintenance and candidate counter
remains identical on these four fixtures. Measured v5 → v6 publication totals:
idle-16 3,204,720 → 3,250,800; fights-8 981,271 → 1,004,311; fights-32
5,406,543 → 5,498,703; crowded-64 24,499,402 → 24,671,752. The crowded
fixture packs 570 fewer lower-priority entity records under the unchanged budget;
its events and work counts are retained. No physics or gameplay-work ceiling rises.
