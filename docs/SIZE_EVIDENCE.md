# Native library size evidence

The opt-in native pilot measures `mmorpg-core`, the maintained gameplay library used by the host and both clients. It counts the raw bytes of `target/x86_64-unknown-linux-gnu/release/libmmorpg_core.rlib`, including Rust archive metadata. This is neither linked executable size nor a runtime, memory, download-size or per-symbol measurement.

From the repository root with Linux x64, Rust from `rust-toolchain.toml` and Bun 1.4.2:

```sh
bun install --cwd web --frozen-lockfile
bun run --cwd web typecheck:size
bun run --cwd web size:budget
bun run --cwd web test:size-evidence
```

`size:budget` builds the library with Cargo's locked release profile, no default features, the explicit GNU Linux x64 target and the repository-local target directory. It remaps the checkout prefix to `/mmorpg` so checkout path length does not inflate the archive. Ambient Rust flags, release-profile overrides and `RUSTUP_TOOLCHAIN` overrides are unavailable, rather than silently changing the declared build. Other host platforms are currently unavailable for this pilot.

`.performance/size.json` selects that one archive and records the target, profile, features, Rust/Cargo versions, Cargo manifests and lockfile, toolchain declaration, producer script and source-package dependency graph. The committed baseline in `.performance/baselines/mmorpg-core.json` retains the exact bytes and SHA-256. Source edits are candidates rather than comparability inputs. Compiler, target, feature or declared build-input differences are incomparable, not improvements or regressions; missing tools, baselines and artifacts are unavailable.

The initial archive budget is 2 MiB, with at most 64 KiB growth against a compatible reviewed baseline. These limits provide headroom over the initial approximately 1.65 MiB archive while catching accidental growth. They are an explicit pilot decision, not an existing runtime or fleet-wide performance guarantee. The shared collector comes from the public `coding-tooling/size-evidence` source export at revision `122ad55cbc5fd82233f2aa4af17278554db5cc78`; package publication is unnecessary.

Ordinary gates never update the baseline. To propose an intentional update, capture to a disposable file, require a passed result, review its build identity, bytes and hashes, and then replace the committed baseline in the same reviewed change:

```sh
bun run --cwd web size:baseline:capture > /tmp/mmorpg-core-size-candidate.json
```

A capture explicitly has no requested comparison; it proves only the absolute budget. Changing build flags requires matching declaration and producer changes before capture. The acceptance command exercises the real built archive and public collector through compatible comparisons, target/feature mismatches, missing inputs/tools, a growth failure and preservation of the committed baseline. It also proves the producer refuses ambient flags. CI repeats the build, budget comparison and acceptance command and retains `artifacts/native-size.json`.

This completes one native-library adoption for coding-tooling#160. It does not roll the capability out to the native client, zone-host executable, WASM bundle or other repositories.

Resolved shared policy sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
