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

Player-owned inventories, queued movement intents, canonical recovery and the
loss-recoverable self sheet remain #83; browser bags remain #84. This pure-rule
slice changes no current zone state, content fingerprint, command or snapshot
version and grants no items through a client command. The authority integration
must bind the catalog revision/content to recovery before inventories persist.
