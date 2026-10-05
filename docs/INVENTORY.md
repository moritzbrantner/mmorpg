# Starter inventory rules

The core inventory value introduced by #82 owns catalog validation and atomic
bag operations. It has exactly 16 ordered slots, indexed 0–15. Empty slots are
`None`; occupied slots contain a catalog item and a nonzero `u16` quantity at
or below that item's stack limit. `ItemStack::new` validates imported stacks;
private fields and a fixed-length slot array prevent unchecked or truncated
imports. Queries expose immutable slots.

The immutable catalog revision is 2 (revision 1 held only items 1 and 2, with
the same names and limits):

| Stable item ID | Name | Stack limit | Equipment slot | Sta | Str | Agi | Int |
| ---: | --- | ---: | --- | ---: | ---: | ---: | ---: |
| 1 | Torn Fur | 20 | — | | | | |
| 2 | Worn Dagger | 1 | main hand | 0 | 2 | 2 | 0 |
| 3 | Militia Shortsword | 1 | main hand | 1 | 3 | 0 | 0 |
| 4 | Apprentice Wand | 1 | main hand | 0 | 0 | 0 | 4 |
| 5 | Pine Buckler | 1 | off hand | 2 | 0 | 0 | 0 |
| 6 | Cloth Hood | 1 | head | 1 | 0 | 0 | 2 |
| 7 | Padded Tunic | 1 | chest | 2 | 0 | 0 | 0 |
| 8 | Padded Trousers | 1 | legs | 1 | 0 | 0 | 0 |
| 9 | Worn Boots | 1 | feet | 1 | 0 | 2 | 0 |

Zero and unknown IDs fail closed. IDs are never reassigned. Names, limits,
slots and stats are content, not client preferences. Equippable items stack to
one; prices belong to the vendor slice.

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
ownership, sequenced moves and periodic recovery. Browser bags presentation (#84) reads these sheets; loot and money are in [LOOT.md](LOOT.md), equipment below, and vendors remain their own slice.

## Equipment

A player has six equipment slots, each empty or holding one catalog item made
for it: 0 main hand, 1 off hand, 2 head, 3 chest, 4 legs, 5 feet (wire and
canonical index). There are no two-handed weapons, rings, armor, or class or
level requirements. `Equipment` validates imported slots, so an unknown item or
an item in the wrong slot cannot be restored.

Each item adds `u8` stamina, strength, agility and intellect; a player's totals
are the sums over its equipped items, with no base attributes.

- Maximum health is `player_max_health(level) + 5 × stamina`. Every rule that
  reads a player's maximum (regeneration, release, heal over time, level-up
  growth, projection, entity health percent and recovery bounds) uses it.
- The class damage bonus is half the primary stat, rounded down: strength for
  Wardens and players without a class, agility for Rangers, intellect for
  Arcanists. It is added to both ends of the player melee range (auto-attack
  and the weapon roll of weapon strikes and Cleave) and to each direct Firebolt
  and Frost Nova hit. Damage and healing over time, Blizzard pulses, shields,
  mana and other resources ignore gear.

`EquipItem { bag_slot }` and `UnequipItem { equipment_slot }` are sequenced
intents resolved in tick step 1 like `MoveItem`:

- Equipping needs an existing, occupied bag slot (else `InvalidInventoryMove`)
  whose item has an equipment slot (else `NotEquippable`). The item leaves the
  bag, and an item already in that equipment slot takes its place in the same
  bag slot, so a full bag never blocks equipping.
- Unequipping needs an existing, occupied equipment slot (else
  `InvalidInventoryMove`) and moves the item to the lowest empty bag slot
  (none: `InventoryFull`).
- Dead players are refused with `YouAreDead`; equipping in combat or while
  casting is allowed.
- Every refusal and revision exhaustion leaves bag, equipment, health and
  revision unchanged. A success increments the bag's revision and sets its
  change tick: bag and equipment share one revision and one sheet.
- Current health follows the maximum: a gain raises it by the same amount, a
  loss lowers it by the same amount but never below 1.

Canonical player records hold the six item IDs after the bag; the self sheet
carries the bag, the equipment and the four stat totals (84 bytes), and every
projection carries the viewer's melee damage range (see
[PROTOCOL.md](PROTOCOL.md)). Starter admission equips nothing; the gear drops
from humanoids ([LOOT.md](LOOT.md)). The browser decodes the equipment and its
stats with the bag and keeps them under the same revision rules; the character
pane below shows them.


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

## Browser Character pane (#129)

Open **Character** or press C from the world canvas. The pane lists the six
equipment slots in wire order (main hand, off hand, head, chest, legs, feet)
with catalog names and stats (for example `+2 Strength, +2 Agility`), empty
slots as Empty, then the Stamina, Strength, Agility and Intellect totals of the
self sheet and the viewer's projected Health `current / max` and Damage
`min–max`. In Bags, selecting a stack whose catalog item has an equipment slot
offers **Equip** (`EquipItem { bag_slot }`); an occupied pane slot offers
**Unequip** (`UnequipItem { equipment_slot }`). Bag slot tooltips show item stats.

The pane reads the Bags panel's received cache: bag and equipment share one
revision, so the same retain/pause rules apply, and slots, totals, health and
damage stay unchanged while an intent is pending. The client never predicts a
swap or a health change. `NotEquippable`, `InventoryFull` on unequip, dead
players and the existing inventory refusals appear as readable feedback in both
panels. Escape or **Close** closes the pane; Escape closes Loot first, then the
pane, then Bags. Opening Loot closes Bags and the pane.

Catalog JSON format v2 includes `itemCatalogRevision` (decimal string) and an
ordered `items` array of `{id, name, maxStack}` from core; format v4 adds each
item's `slot` (`null` or the camel-case slot name) and `stats`. The browser decodes it
strictly and uses its names, without reproducing grant or bag-mutation rules.
Focused state tests and real Chromium cover commands, refusal, fresh-session
reset, keyboard close and narrow/short viewport layouts.
