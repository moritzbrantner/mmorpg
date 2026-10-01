# Starter loot rules

The immutable rule catalog has revision 1 and one table per hosted creature
template. Each roll yields u32 copper and at most one ordinary item stack.
These minimal rewards use the existing item catalog; equipment effects, prices
and quest-conditional drops are separate integrations.

| Template ID | Creature | Inclusive copper range | Weighted ordinary item outcome |
| --- | --- | --- | --- |
| 1 | Timber Wolf | 0–2 | Torn Fur 1–2: weight 3; nothing: weight 1 |
| 2 | Young Boar | 0–3 | Torn Fur 1: weight 1; nothing: weight 1 |
| 3 | Grain Rat | 0–1 | Torn Fur 1: weight 1; nothing: weight 3 |
| 4 | Field Marauder | 2–6 | Worn Dagger 1: weight 1; nothing: weight 3 |
| 5 | Mirefin Lurker | 1–4 | Nothing: weight 1 |
| 6 | Redbrand Bandit | 4–9 | Worn Dagger 1: weight 1; nothing: weight 1 |
| 7 | Garrick Redbrand | 25–35 | Worn Dagger 1: weight 1 |

`LootTable::new` validates authored money bounds and one to four ordered
outcomes. Every weight is positive; item IDs and inclusive quantity ranges must
fit the item catalog. Private fields retain those invariants. The table exposes
immutable queries and a pure `roll(LootRolls)` operation. Construction copies
at most four outcomes into fixed capacity; neither construction nor rolling
allocates. Unknown template lookup returns `None`, never
another template's fallback reward.

The caller supplies three independent u32 values for money, outcome and quantity.
Money and quantity rolls map by modulo to their inclusive ranges. The outcome
roll maps by modulo to the total weight, then selects the first ascending
cumulative-weight bucket. This is an exact deterministic mapping, not a claim
of unbiased sampling from an arbitrary caller distribution. Even no-drop outcomes
use the same supplied roll tuple; the table never draws from or advances an RNG.
Money arithmetic widens before computing the inclusive range, supporting
0..=u32::MAX without overflow.

Public-API tests cover malformed authoring, exact bucket/range boundaries,
maximum values/weights, every hosted template and reproducibility. Greyhaven
revision 5 activates all seven tables; #90 owns the browser window.

`settle_loot` (#102) credits one validated `LootRewards` value into an inventory
and u32 copper balance. It checks copper overflow first, then uses the existing
allocation-free, ordered, atomic bag insertion. Either both credits succeed or
both inputs remain unchanged. Money-only rewards work with a full bag; an empty
reward is a successful no-op; item-only rewards work at the maximum copper
balance. Overflow refuses rather than saturating. Typed errors retain bag refusal
details, and overflow wins when both limits would be exceeded.

The caller retains the supplied reward and consumes its authoritative claim
only after success. Settlement itself owns no player identity, eligibility,
range or duplicate-claim fencing. The corpse authority integrates those
preconditions with generation, revisions, canonical state and wire recovery.

`ZoneContent::with_loot_tables` (#105) binds an explicit nonzero loot revision
and a bounded table of already validated `LootTable` values. Template IDs must
exist in the zone and be unique; binding sorts them, while weighted outcomes
retain their authored order. Missing template lookup returns `None`. An empty
but explicitly revisioned catalog is valid and differs from unbound content.
Immutable queries expose the revision and ordered rules without allowing mutation.

Bound content uses fingerprint domain `mmorpg.zone-content/v3`: the existing v2
content fingerprint, loot revision and every ordered rule field enter identity.
Thus changing any money bound, outcome kind, item, quantity bound or weight
refuses recovery against the old identity. Binding retains the declared RNG seed. Unbound content keeps
its existing v2 fingerprint exactly. Greyhaven revision 5 binds catalog revision 1
and has fingerprint `5dcb5d3b46dc5451`, while preserving its physical definition,
units and revision-3 AI/combat seed `3cbc808bbe89b29c`.

## Corpse authority

On creature death, core draws exactly three values from a separate `ZoneRng`
initialized with `content.rng_seed() XOR 0x6c6f6f742f763031`. Each value is the
upper 32 bits of the next 64-bit draw, in money/outcome/quantity order. A bound
table rolls once; the tapper's corpse retains that result, including an empty
roll, until successful settlement or expiry. Queries and refused claims draw
nothing. This stream's state is canonical; it never advances the AI/combat RNG.

`Loot { creature, died_at }` is sequenced and queued like other discrete intents.
The next tick requires a living sender, that exact unexpired corpse death,
ownership by its tapper, inclusive 300-unit (3 m) Euclidean distance between
authoritative centres in XYZ, and nonempty remaining rewards. Ownership does
not depend on the client's selection. The corpse's projected lootable flag
means owned rewards exist; range and player life are separate eligibility checks.
The selected eligible corpse's complete sheet is repeated on every projection.

Settlement stages the fixed bag and u32 copper balance, preflights any bag revision
increment, then commits both credits and consumes remaining loot together. Full
bags, copper overflow or revision exhaustion leave player and corpse unchanged.
Money-only rewards work with a full bag and do not bump its revision. Item credit
updates the existing bag revision/change tick; periodic complete sheets recover
a lost update. Duplicate claims cannot credit again, and a respawn's new death
tick fences claims for an earlier life. Looting preserves the existing corpse
despawn and respawn schedule. Removing a player clears its tap and rewards, so
a reused session-local ID cannot inherit them.

Refusals use existing `YouAreDead`, `OutOfRange`, `InventoryFull` and
`InvalidInventoryMove` (revision exhaustion), plus `InvalidLoot`, `NotLootOwner`,
`EmptyLoot` and `MoneyOverflow`. Feedback is cosmetic; money, bag revision and
complete loot presence/state are durable projection facts.

Canonical recovery retains copper, both RNG states, remaining rewards, tapper,
death tick and pending claims. It validates rewards against the bound table and
requires an existing owner and unexpired corpse. Raw wire continuation tests
cover pending, refused and consumed claims; a native two-client transport test
covers ownership, resume and duplicate credit. `corpse-loot-resume` reuses the
deterministic hosted wolf hunt and checks money, bags and stale claims through
connection resume. A WASM integration test repeats the actual hunt and recovers
an intentionally missed bag sheet. See [PROTOCOL.md](PROTOCOL.md) for v8/v3
compatibility and [SCENARIOS.md](SCENARIOS.md) for the scenario vocabulary.

## Browser projection state (#108)

`LootState` copies only received copper and the complete optional selected-corpse
sheet. Increasing ticks within the joined zone/content/viewer identity replace
that state; absence clears loot, while a dropped publication recovers from the
next complete sheet. Target/death changes clear old feedback. Reset invalidates
queued actions even when a session-local ID is reused.

`claimIntent()` queues one existing `Loot` command, then rechecks the freshest
projection's identity, tick, living viewer, selected creature and death fence at
dispatch. It never changes rewards, copper or bags. Refused claims retain the
received sheet; a later acknowledged projection restores the control even when
cosmetic refusal feedback was lost. Bag recovery stays in the existing bag cache.
The DOM window and real Chromium interaction are the separate #109 child of #90.
