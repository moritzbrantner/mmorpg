# Starter inventory rules

The core inventory value introduced by #82 owns catalog validation and atomic
bag operations. It has exactly 16 ordered slots, indexed 0–15. Empty slots are
`None`; occupied slots contain a catalog item and a nonzero `u16` quantity at
or below that item's stack limit. `ItemStack::new` validates imported stacks;
private fields and a fixed-length slot array prevent unchecked or truncated
imports. Queries expose immutable slots.

The minimal immutable catalog revision is 1:

| Stable item ID | Name | Stack limit |
| --- | --- | --- |
| 1 | Torn Fur | 20 |
| 2 | Worn Dagger | 1 |

Zero and unknown IDs fail closed. IDs are never reassigned. Names and limits
are content, not client preferences. The dagger is currently an ordinary bag
item; equipment effects, prices and loot tables belong to their later slices.

Insertion fills matching stacks in ascending slot order, then empty slots in
ascending order. The entire requested quantity must fit. Moves to an empty
slot split or transfer a stack; moves to the same item merge exactly the
requested quantity, never silently clamp it. Moving a complete stack onto a
different item swaps the slots; partial swaps fail. A valid move to the same
slot is a no-op. Invalid slots, empty sources, zero/excess quantities and
insufficient capacity leave every original slot unchanged.

Public API tests exercise the stack boundaries, deterministic fill order,
full bags, atomic overflow, split/merge/swap behavior and a bounded matrix of
2,601 moves that checks item conservation and unchanged refusals independently.
There is no randomness, persistence I/O, wall clock or heap allocation in
these operations.

## Authority and recovery (#83)

Each admitted player starts with three Torn Fur in slot 0 and one Worn Dagger
in slot 1. The grant is deterministic core content; no client grant command
exists. A fresh admission gets a fresh bag. Reconnect within the session grace
preserves its bag; durable character saves remain #30/#40.

`MoveItem` uses the existing bounded sequenced intent queue. It resolves against
the sender's own bag at tick step 1. Dead players cannot move items. Invalid
moves and exhausted revisions report `InvalidInventoryMove`; an overfull target
reports `InventoryFull`. Refusals and validated same-slot no-ops leave bag revision
unchanged. A changed bag increments its nonzero `u64` revision and records the
resulting tick. Overflow refuses the entire move. Death, release and reconnect
never grant or remove items.

Canonical snapshots preserve ordered slots, revision, last-change tick and
pending moves. Recovery rejects invalid catalog stacks, zero revision or future
change ticks. Catalog revision, item names/limits, starter grant and declared RNG
seed enter content identity. Greyhaven content revision 4 deliberately preserves
the revision-3 RNG seed, preventing an economy-only change from rerolling combat.
Older snapshots/recovery bundles require explicit migration or fresh state.

Every projection repeats bag revision. It includes the complete 64-byte sheet
on admission/change ticks and every ten ticks; missing sheets mean retain prior
state. Dropped changes recover on the next periodic sheet (about 330 ms).
Projection queries never mutate authority. Snapshot v7 retains the 1,077-byte
budget and packs fewer low-priority entities when a sheet is present; see
[PROTOCOL.md](PROTOCOL.md).

Core recovery/loss fixtures, the `inventory-resume` real-session scenario, a real
native WebTransport move/resume/merge test, and the browser WASM adapter prove
ownership, sequenced moves and periodic recovery. Browser bags presentation (#84) reads these sheets; loot, money, equipment effects and vendors remain their own slices.


## Browser Bags panel (#84)

Open **Bags** or press B from the world canvas. All 16 slots show core catalog
names and received quantities. Select an occupied slot, enter a whole-number
quantity (default: the full stack), then select a destination. Selecting the same
slot cancels. This supports split, transfer, merge and full-stack swap using the
shared command path; the core decides whether each move is allowed. The slots
stay unchanged while an intent is pending, and server refusals appear in the
panel. Escape or **Close** closes it without leaving the world.

Missing sheets retain the prior bag. If a projection announces a newer revision
without its sheet, moves pause until periodic recovery supplies that revision.
Stale ticks/revisions and another viewer/zone/content identity cannot replace the
cache. Entry binds the current source identity and leave/reset removes prior
slots, selection and feedback. A fresh entry still gets a fresh starter bag;
character/world persistence belongs to #30/#40.

Catalog JSON format v2 includes `itemCatalogRevision` (decimal string) and an
ordered `items` array of `{id, name, maxStack}` from core. The browser decodes it
strictly and uses its names, without reproducing grant or bag-mutation rules.
Focused state tests and real Chromium cover commands, refusal, fresh-session
reset, keyboard close and narrow/short viewport layouts.
