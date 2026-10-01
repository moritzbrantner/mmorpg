# Native library size evidence

The opt-in native pilot measures `mmorpg-core`, the maintained gameplay library used by the host and both clients. It counts the raw bytes of `target/x86_64-unknown-linux-gnu/release/libmmorpg_core.rlib`, including Rust archive metadata. This is neither linked executable size nor a runtime, memory, download-size or per-symbol measurement.

From the repository root with Linux x64, Rust from `rust-toolchain.toml` and Bun 1.4.2:

```sh
bun install --cwd web --frozen-lockfile
bun run --cwd web typecheck:size
bun run --cwd web size:budget
bun run --cwd web test:size-evidence
```

`size:budget` builds the library with Cargo's locked release profile, no default features, the explicit GNU Linux x64 target and the repository-local target directory. It remaps the checkout prefix to `/mmorpg` so checkout path length does not inflate the archive. Ambient compiler controls (`RUSTC` and `RUSTC_*`, including bootstrap), wrapper, incremental, Rust-flag, release-profile and `RUSTUP_TOOLCHAIN` overrides are unavailable, rather than silently changing the declared build. Inherited Cargo configuration is unsupported unless it contains only dev/test profile settings, which cannot affect this release build. Cargo and rustc must pass bounded availability probes before compilation. Other host platforms are currently unavailable for this pilot.

`.performance/size.json` selects that one archive and records the target, profile, features, Rust/Cargo versions, Cargo manifests and lockfile, toolchain declaration, producer script and source-package dependency graph. The committed baseline in `.performance/baselines/mmorpg-core.json` retains the exact bytes and SHA-256. Source edits are candidates rather than comparability inputs. Compiler, target, feature or declared build-input differences are incomparable, not improvements or regressions; missing tools, baselines and artifacts are unavailable.

The initial archive budget is 2 MiB, with at most 64 KiB growth against a compatible reviewed baseline. These limits provide headroom over the initial approximately 1.65 MiB archive while catching accidental growth. They are an explicit pilot decision, not an existing runtime or fleet-wide performance guarantee. The shared collector comes from the public `coding-tooling/size-evidence` source export at revision `122ad55cbc5fd82233f2aa4af17278554db5cc78`; package publication is unnecessary.

The script typecheck uses Bun's native `bun-types` provider for TOML parsing and declares its Undici type import explicitly. It preserves package symlinks so the isolated Bun dependency graph resolves through this consumer, with every declaration still checked.

Ordinary gates never update the baseline. To propose an intentional update, capture to a disposable file, require a passed result, review its build identity, bytes and hashes, and then replace the committed baseline in the same reviewed change:

```sh
bun run --cwd web size:baseline:capture > /tmp/mmorpg-core-size-candidate.json
```

A capture explicitly has no requested comparison; it proves only the absolute budget. Changing build flags requires matching declaration and producer changes before capture. The acceptance command exercises the real built archive and public collector through compatible comparisons, target/feature mismatches, missing inputs/tools, a growth failure and preservation of the committed baseline. It also proves the producer refuses ambient flags. CI repeats the build, budget comparison and acceptance command and retains `artifacts/native-size.json`.

This completes one native-library adoption for coding-tooling#160. It does not roll the capability out to the native client, zone-host executable, WASM bundle or other repositories.

Resolved shared policy sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.


The #83 inventory/schema-v7 change intentionally advances the reviewed baseline
to **1,818,308 bytes**, SHA-256
`ffc794c54ef39595ae7e11be36ad441aeeee520cdeb38de0e1b98dd29529bbee`.
Its build identity is identical to the initial 1,736,620-byte baseline. The
81,688-byte cumulative increase exceeded the 65,536-byte growth gate; the failed
comparison is retained in
[the raw observation](../.performance/observations/inventory-v7-growth.json).
The preceding #82 pure catalog/bag archive measured 1,784,858 bytes, so #83 adds
33,450 bytes for authoritative bags, queued moves, snapshots and declared RNG
seed identity. The new APIs and stored records explain expected feature growth;
this archive includes Rust metadata and is not a linked-code cost estimate.
The absolute 2 MiB and per-baseline 64 KiB budgets remain unchanged. The explicit
capture passed the absolute budget, and the ordinary gate never updated evidence.

The #102 atomic loot settlement change advances the reviewed baseline to
**1,893,188 bytes**, SHA-256
`1111e9b8119b7f18b6f2895326e74d420ff2a454b062a58888d05eba75b1b0c5`.
Its complete build identity matches the inventory baseline. The cumulative
74,880-byte increase exceeded the unchanged 65,536-byte growth gate; the
[failed observation](../.performance/observations/loot-settlement-growth.json)
retains the exact comparison. The preceding #88 loot catalog measured
1,880,316 bytes, so this public settlement function and typed error add 12,872
archive bytes. These are raw archive/metadata bytes, not a linked-code or runtime
cost estimate. Explicit capture passed the unchanged 2 MiB absolute limit;
the ordinary gate and its adversarial acceptance checks do not update baselines.

The #103 corpse authority/schema-v8 change advances the reviewed baseline to
**1,971,554 bytes**, SHA-256
`e02b7da4676de49a783d3959fa50793b3cab882847d5922daf452d1f3b791bc2`.
Its complete build identity and inputs match the #102 baseline. The cumulative
78,366-byte increase exceeds the unchanged 65,536-byte growth gate;
[the failed comparison](../.performance/observations/corpse-loot-v8-growth.json)
retains the observation. The preceding #105 content-binding archive measured
1,925,682 bytes, so corpse generation, claim authority and canonical public
records add 45,872 archive bytes. Raw archives include Rust metadata;
this is not a linked-code or runtime cost estimate. Explicit capture passed the
unchanged 2 MiB absolute limit. The ordinary comparison and adversarial acceptance
checks retain both budgets and never update evidence automatically.
