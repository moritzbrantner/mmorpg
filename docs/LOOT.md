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
maximum values/weights, every hosted template, reproducibility and unchanged
live zone snapshots after independent rolls. The catalog is not activated in
zone gameplay yet: #89 owns death generation, tapper/range/claim fencing,
remaining corpse rewards, money/bag transactions, canonical state, wire and
scenario acceptance. #90 owns the browser window. Activation must bind the
catalog to revisioned content identity and decide the authoritative roll source;
these standalone rules leave content identity, combat RNG and existing bytes intact.

`settle_loot` (#102) credits one validated `LootRewards` value into an inventory
and u32 copper balance. It checks copper overflow first, then uses the existing
allocation-free, ordered, atomic bag insertion. Either both credits succeed or
both inputs remain unchanged. Money-only rewards work with a full bag; an empty
reward is a successful no-op; item-only rewards work at the maximum copper
balance. Overflow refuses rather than saturating. Typed errors retain bag refusal
details, and overflow wins when both limits would be exceeded.

The caller retains the supplied reward and consumes its authoritative claim
only after success. Settlement itself owns no player identity, eligibility,
range or duplicate-claim fencing. #103 integrates those preconditions with
corpse generation, revisions, canonical state and wire recovery under #89.

`ZoneContent::with_loot_tables` (#105) binds an explicit nonzero loot revision
and a bounded table of already validated `LootTable` values. Template IDs must
exist in the zone and be unique; binding sorts them, while weighted outcomes
retain their authored order. Missing template lookup returns `None`. An empty
but explicitly revisioned catalog is valid and differs from unbound content.
Immutable queries expose the revision and ordered rules without allowing mutation.

Bound content uses fingerprint domain `mmorpg.zone-content/v3`: the existing v2
content fingerprint, loot revision and every ordered rule field enter identity.
Thus changing any money bound, outcome kind, item, quantity bound or weight
refuses recovery against the old identity. Binding retains the declared RNG seed
and introduces no reward generation or simulation reads. Unbound content keeps
its existing v2 fingerprint exactly; Greyhaven remains revision 4 and unbound
until #103 activates the catalog with corpse authority.
