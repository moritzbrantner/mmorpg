# Actual zone physics adoption workloads

This is the semantics-preserving first adoption slice of MMORPG #46 and physics-engine #190. The zone continues to import the translational integer-state `physics_engine::World`; its units, tick order, CCD, game rules, save schema and player projection policy retain their existing contracts. This dependency update does not select the persistent-f64 solver. That migration still requires failed-step/checkpoint, character/query and applicable CCD prerequisites.

## Reproduce and compare

```sh
cargo run -p mmorpg-protocol --example physics_workload --release --locked
cargo run -p mmorpg-protocol --example physics_workload --release --locked -- --trace artifacts/physics-traces
```

The versioned `mmorpg.physics-workload/v1` example drives the real zone API for 120 completed ticks in three independent trials per workload. Construction is excluded; bootstrap is included. Layouts cover 128/512 stationary players, one/eight locally moving controllers among 512, 512 players on the vale's gravity-supported spawn plaza with a jump, a 64-player overlapping spawn control, negative-coordinate movement plus admission/removal/same-ID reuse and teleport recovery, and the actual vale's 59 creatures/nine NPCs with 16 players. These are workload fixtures, not supported capacity claims.

Every workload captures canonical state at tick 60, reconstructs a second zone and applies identical subsequent commands. Subsequent canonical state must match on every tick. The mutation workload also reconstructs after a deliberate teleport. Interest maintenance counts accumulate around each tick so reconstruction cannot erase prior measured work.

Measure `advance_tick` separately from recipient projection, projection encoding and canonical capture/encoding. Tick time includes gameplay/AI/controllers, physics and interest maintenance; it is not labeled physics-only time. Commands, exhaustive-reference validation, shadow recovery and diagnostic trace writes are outside those measured regions. Project eight recipients each tick and all recipients at the completed horizon; player-only fixtures also require the existing exhaustive projection oracle's bytes. The vale unit fixture uses the ordinary unit/recovery/protocol acceptance rather than pretending the player-only oracle covers creatures.

Optional bounded traces contain length-prefixed canonical and selected player-projection payloads at every tick. Comparing the files byte for byte across the old and new dependency establishes exact observable compatibility; the FNV-1a diagnostic checksum is convenient for inspecting repeated runs, not a replacement for the full comparison or a security hash. Trace capture uses no network and is disabled by default. No trace data, timings or derived engine cache enter production recovery or wire state.

## Evidence

Baseline consumer revision: `20d34fb` with physics-engine `1b98f84d409796b2a15b84f3fa4ed7f03a11f8bd`. Candidate engine: `0baf3411419fc250273caec24d64654cb30c28ec` (upstream PR #206). All 24 complete canonical/projection binary traces are byte-identical. Existing scenario/protocol goldens are retained.

Advisory median `advance_tick` totals for 120 ticks:

| Workload | Baseline ms | Candidate ms |
| --- | ---: | ---: |
| quiet-128 | 3.280 | 1.116 |
| quiet-512 | 26.921 | 8.662 |
| sparse-512-k1 | 27.349 | 22.956 |
| sparse-512-k8 | 27.138 | 22.900 |
| supported-512 | 56.271 | 45.759 |
| crowded-64 | 3.771 | 2.790 |
| mutation-recovery-32 | 0.704 | 0.701 |
| vale-units-16 | 14.148 | 14.913 |

Quiet and supported cases improve in this local comparison. Sparse and crowded cases remain active-path controls. The actual vale unit case is about 5.4% slower, and mutation/recovery shows little change; neither is reported as a speedup. Shared-machine timings are advisory and do not define a throughput gate. Full-zone work still includes controller and interest population scans. Sparse physics still stages/sorts its admitted bodies, so #190 remains open for further measured maintenance improvements.

Upstream `docs/translational-maintenance.md` records physics-call phase/work/capacity evidence and its independent exhaustive rebuild oracle. Upstream work fences require zero staging, bounds, queries and map reconstruction on unchanged quiet ticks, and zero body-map reconstruction on active successful ticks. Vector capacity is not process RSS or allocator overhead; the consumer does not claim a measured memory reduction from this dependency update.

Fingerprint: Rust 1.98.1, `x86_64-unknown-linux-gnu`, release profile, empty `RUSTFLAGS`, workload v1, three trials, identical inputs and horizons, committed dependency locks on each side; Linux, AMD Ryzen 7 5700X. Local workspace debug verification disables debug symbols/incremental caches to limit disposable disk use; timing comparisons use the same release configuration. Resolved conventions sourceRevision: `e6acb5310afaf15c0cba24f87108f5f4ad1bedc3`.
