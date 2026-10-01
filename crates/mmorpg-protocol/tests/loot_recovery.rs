//! Continue pending, refused and consumed claims through real canonical bytes.
#[path = "../../mmorpg-core/tests/support/arena.rs"]
mod arena;

use std::sync::Arc;

use mmorpg_core::{
    CreatureId, EntityRef, INVENTORY_SLOTS, Inventory, ItemId, ItemStack, LootOutcome, LootTable,
    ZoneCommand, ZoneId, ZoneSimulation,
};
use mmorpg_protocol::{decode_canonical_snapshot, encode_canonical_snapshot, pack_snapshot};

fn recovered(zone: &ZoneSimulation) -> ZoneSimulation {
    let bytes = encode_canonical_snapshot(&zone.snapshot().unwrap()).unwrap();
    ZoneSimulation::from_snapshot(
        decode_canonical_snapshot(&bytes).unwrap(),
        Arc::clone(zone.content()),
    )
    .unwrap()
}

fn corpse() -> ZoneSimulation {
    let original = arena::arena(
        vec![arena::wolf(1, [1, 1])],
        vec![arena::spawn(1, arena::WOLF, [-180, 0])],
        vec![],
    );
    let table = LootTable::new(
        [2, 2],
        &[LootOutcome::Item {
            weight: 1,
            item: ItemId::new(1),
            quantity: [2, 2],
        }],
    )
    .unwrap();
    let content = Arc::new(
        original
            .as_ref()
            .clone()
            .with_loot_tables(1, vec![(arena::WOLF, table)])
            .unwrap(),
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(
        1,
        1,
        ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(1)))),
    )
    .unwrap();
    zone.apply_command(1, 2, ZoneCommand::StartAttack).unwrap();
    for _ in 0..180 {
        zone.advance_tick().unwrap();
        if zone.snapshot_for_player(1).unwrap().loot.is_some() {
            return zone;
        }
    }
    panic!("corpse was not generated");
}

fn continue_equally(left: &mut ZoneSimulation, right: &mut ZoneSimulation, ticks: usize) {
    for _ in 0..ticks {
        left.advance_tick().unwrap();
        right.advance_tick().unwrap();
        assert_eq!(
            encode_canonical_snapshot(&left.snapshot().unwrap()).unwrap(),
            encode_canonical_snapshot(&right.snapshot().unwrap()).unwrap()
        );
        assert_eq!(
            pack_snapshot(&left.snapshot_for_player(1).unwrap()).unwrap(),
            pack_snapshot(&right.snapshot_for_player(1).unwrap()).unwrap()
        );
    }
}

#[test]
fn pending_and_consumed_claims_survive_wire_recovery_without_double_credit() {
    let mut zone = corpse();
    let claim = zone.snapshot_for_player(1).unwrap().loot.unwrap().claim;
    zone.apply_command(1, 3, ZoneCommand::Loot(claim)).unwrap();
    let mut copy = recovered(&zone);
    continue_equally(&mut zone, &mut copy, 10);
    let state = zone.snapshot().unwrap();
    assert_eq!(state.players[0].copper, 2);
    assert_eq!(state.players[0].inventory.slots()[0].unwrap().quantity(), 5);
    assert!(state.creatures[0].loot.is_none());
    let mut copy = recovered(&zone);
    for simulation in [&mut zone, &mut copy] {
        simulation
            .apply_command(1, 4, ZoneCommand::Loot(claim))
            .unwrap();
    }
    continue_equally(&mut zone, &mut copy, 10);
    assert_eq!(zone.snapshot().unwrap().players[0].copper, 2);
}

#[test]
fn refusal_checkpoints_retain_the_exact_reward_and_dedicated_rng_state() {
    let mut zone = corpse();
    let claim = zone.snapshot_for_player(1).unwrap().loot.unwrap().claim;
    let mut state = zone.snapshot().unwrap();
    state.players[0].inventory =
        Inventory::from_slots([Some(ItemStack::new(ItemId::new(2), 1).unwrap()); INVENTORY_SLOTS]);
    let rewards = state.creatures[0].loot;
    let rng = state.loot_rng_state;
    zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.apply_command(1, 3, ZoneCommand::Loot(claim)).unwrap();
    let mut copy = recovered(&zone);
    continue_equally(&mut zone, &mut copy, 1);
    let mut copy = recovered(&zone);
    continue_equally(&mut zone, &mut copy, 10);
    let state = zone.snapshot().unwrap();
    assert_eq!(state.players[0].copper, 0);
    assert_eq!(state.creatures[0].loot, rewards);
    assert_eq!(state.loot_rng_state, rng);
}
