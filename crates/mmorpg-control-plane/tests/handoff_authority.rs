use mmorpg_control_plane::{
    EntityId, HandoffPhase, HandoffRegistry, HandoffTicket, HostId, TransferId, ZoneDirectory,
};
use mmorpg_core::ZoneId;

fn host(name: &str) -> HostId {
    HostId::new(name).unwrap()
}

#[test]
fn lease_renewal_preserves_handoff_identity_and_retries() {
    let mut directory = ZoneDirectory::new(10).unwrap();
    let source = directory.assign(ZoneId::new(1), host("a"), 0).unwrap();
    let destination = directory.assign(ZoneId::new(2), host("b"), 0).unwrap();
    let mut ticket = HandoffTicket {
        transfer_id: TransferId::new(7),
        entity_id: EntityId::new(90),
        source,
        destination,
    };
    let mut registry = HandoffRegistry::default();
    registry.prepare(ticket.clone(), &directory, 0).unwrap();
    ticket.source = directory.renew(&ticket.source, 5).unwrap();
    ticket.destination = directory.renew(&ticket.destination, 5).unwrap();
    assert_eq!(
        registry.prepare(ticket.clone(), &directory, 5).unwrap(),
        HandoffPhase::Prepared
    );
    assert_eq!(
        registry
            .accept(ticket.transfer_id, &ticket.destination, &directory, 5)
            .unwrap(),
        HandoffPhase::Accepted
    );
    assert_eq!(
        registry
            .commit(ticket.transfer_id, &ticket.source, &directory, 5)
            .unwrap(),
        HandoffPhase::Committed
    );
}

#[test]
fn an_entity_cannot_prepare_two_concurrent_transfers() {
    let mut directory = ZoneDirectory::new(10).unwrap();
    let source = directory.assign(ZoneId::new(1), host("a"), 0).unwrap();
    let destination = directory.assign(ZoneId::new(2), host("b"), 0).unwrap();
    let mut ticket = HandoffTicket {
        transfer_id: TransferId::new(7),
        entity_id: EntityId::new(90),
        source,
        destination,
    };
    let mut registry = HandoffRegistry::default();
    registry.prepare(ticket.clone(), &directory, 0).unwrap();
    ticket.transfer_id = TransferId::new(8);
    assert!(registry.prepare(ticket.clone(), &directory, 0).is_err());
    assert!(registry.record(ticket.transfer_id).is_none());
}

#[test]
fn failed_assignment_does_not_consume_an_epoch_or_remove_a_lease() {
    let mut directory = ZoneDirectory::new(10).unwrap();
    let zone = ZoneId::new(1);
    let lease = directory.assign(zone, host("a"), 0).unwrap();
    let floor = directory.epoch_floor_snapshot();
    assert!(directory.assign(zone, host("b"), u64::MAX).is_err());
    assert_eq!(directory.epoch_floor_snapshot(), floor);
    assert_eq!(directory.lease(zone), Some(&lease));
    assert!(
        directory
            .reassign(zone, lease.epoch, host("b"), u64::MAX)
            .is_err()
    );
    assert_eq!(directory.epoch_floor_snapshot(), floor);
    assert_eq!(directory.lease(zone), Some(&lease));
}

#[test]
fn retrying_a_committed_transfer_does_not_release_a_newer_reservation() {
    let mut directory = ZoneDirectory::new(10).unwrap();
    let source = directory.assign(ZoneId::new(1), host("a"), 0).unwrap();
    let destination = directory.assign(ZoneId::new(2), host("b"), 0).unwrap();
    let first = HandoffTicket {
        transfer_id: TransferId::new(7),
        entity_id: EntityId::new(90),
        source,
        destination,
    };
    let mut registry = HandoffRegistry::default();
    registry.prepare(first.clone(), &directory, 0).unwrap();
    registry
        .accept(first.transfer_id, &first.destination, &directory, 0)
        .unwrap();
    registry
        .commit(first.transfer_id, &first.source, &directory, 0)
        .unwrap();
    let mut next = HandoffTicket {
        transfer_id: TransferId::new(8),
        entity_id: first.entity_id,
        source: first.destination.clone(),
        destination: first.source.clone(),
    };
    registry.prepare(next.clone(), &directory, 1).unwrap();
    registry
        .commit(first.transfer_id, &first.source, &directory, 1)
        .unwrap();
    next.transfer_id = TransferId::new(9);
    assert!(registry.prepare(next, &directory, 1).is_err());
}
