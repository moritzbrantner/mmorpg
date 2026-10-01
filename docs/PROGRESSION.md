# Starter progression rules

Issue #74 defines the immutable rule queries in `mmorpg-core`. XP is not yet
awarded by the zone: #75 owns authoritative tapping/eligibility, mutable XP,
level growth, canonical persistence and exact self projection; #76 owns the
browser XP bar and feedback. These queries do not grant XP or change combat.

Player progression supports levels 1–10. The next-level thresholds are
current-level XP, not cumulative totals:

| Current level | XP to next level |
| --- | ---: |
| 1 | 100 |
| 2 | 200 |
| 3 | 300 |
| 4 | 400 |
| 5 | 500 |
| 6 | 600 |
| 7 | 700 |
| 8 | 800 |
| 9 | 900 |
| 10 | 0 (cap) |

The total from level 1 to level 10 is 4,500 XP. The future authority consumes
each threshold on level-up and carries remaining current-level XP forward.
At the cap, XP and its next-level threshold are zero. The existing health and
damage growth rules remain the stat foundation; resources belong to #23.

`kill_experience(player_level, creature_level)` returns the nominal kill
reward, before authoritative eligibility. Creature levels remain valid through
60, independently of the starter player cap. Base XP is `45 + 5 × creature
level`. A creature one/two/three/four levels below pays 80/60/40/20% of base;
five or more below pays zero. Equal level pays 100%. Each level above adds 10%
to a maximum of 150%. Round the final integer reward down. At player level 10
the reward is zero.

Examples: a level-1 player gets 50 XP from a level-1 creature, so two eligible
kills reach level 2. A level-5 player gets 70 from level 5, 52 from level 4,
10 from level 1, or 82 from level 6. A level-6 player gets zero from level 1.
Quest XP and other rewards are outside this kill rule.

Both public queries return `None` for unsupported levels. The capped threshold
is `Some(0)`, distinct from invalid input. No client command can call these
queries to mutate XP. Applying these rules and rejecting invalid persisted
progression belong to #75; existing snapshots are unchanged in #74.

Exact public-API fixtures cover the curve, rounding, gray rewards, bonus cap,
player cap and invalid inputs. Conventions sourceRevision:
`46d8793bb3034326561f876dcc67dbaa5aa1e432`.
