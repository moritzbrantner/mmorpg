//! Canonical recovery validation through the public zone API: restored state
//! must match its content exactly.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureSpawn, CreatureTemplate, ZoneContent, ZoneId, ZoneSimulation,
};
use support::arena::{self, WOLF};

const HOME: [i32; 2] = [5_000, 5_000];
const WANDER_RADIUS: i32 = 600;

fn wandering_wolf(template: CreatureTemplate) -> Arc<ZoneContent> {
    arena::arena(
        vec![template],
        vec![CreatureSpawn {
            wander_radius: WANDER_RADIUS,
            ..arena::spawn(1, WOLF, HOME)
        }],
        vec![],
    )
}

/// A zone with one admitted player, far from the wolf, and its checkpoint.
fn checkpoint(content: &Arc<ZoneContent>) -> CanonicalZoneSnapshot {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), Arc::clone(content)).unwrap();
    zone.add_player(1).unwrap();
    zone.snapshot().unwrap()
}

fn restore(
    snapshot: CanonicalZoneSnapshot,
    content: &Arc<ZoneContent>,
) -> Result<ZoneSimulation, String> {
    ZoneSimulation::from_snapshot(snapshot, Arc::clone(content))
        .map_err(|error| error.message().to_owned())
}

#[test]
fn recovery_refuses_changed_content_of_the_same_revision() {
    let content = wandering_wolf(arena::wolf(30, [1, 2]));
    let retuned = wandering_wolf(CreatureTemplate {
        swing_ticks: 61,
        ..arena::wolf(30, [1, 2])
    });
    assert_eq!(content.revision(), retuned.revision());
    assert_ne!(content.fingerprint(), retuned.fingerprint());

    let state = checkpoint(&content);
    assert!(restore(state.clone(), &content).is_ok());
    assert_eq!(
        restore(state, &retuned).err().unwrap(),
        "snapshot content identity does not match the supplied zone content"
    );
}
