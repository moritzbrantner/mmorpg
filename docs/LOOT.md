# Starter loot rules (#88)

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
this pure-rule slice leaves content identity, combat RNG and existing bytes intact.
