use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use mmorpg_control_plane::{
    EntityId, EpochFloorStore, EpochFloorStoreError, EpochPersistedDirectoryError,
    EpochPersistedZoneDirectory, HandoffPhase, HandoffRegistry, HandoffTicket, HostId,
    HostRegistry, TransferId, ZoneDirectory,
};
use mmorpg_core::ZoneId;

fn host(name: &str) -> HostId {
    HostId::new(name).unwrap()
}

fn registered_hosts() -> HostRegistry {
    let mut hosts = HostRegistry::new(u64::MAX).unwrap();
    for name in ["a", "b"] {
        hosts.register(host(name), 0).unwrap();
    }
    hosts
}

#[derive(Clone, Default)]
struct SharedEpochStore {
    floors: Arc<Mutex<BTreeMap<ZoneId, u64>>>,
    reject_next: Arc<AtomicBool>,
}

impl SharedEpochStore {
    fn reject_next(&self) {
        self.reject_next.store(true, Ordering::SeqCst);
    }

    fn floor(&self, zone_id: ZoneId) -> Option<u64> {
        self.floors.lock().unwrap().get(&zone_id).copied()
    }
}

impl EpochFloorStore for SharedEpochStore {
    fn load_epoch_floor(&self) -> Result<BTreeMap<ZoneId, u64>, EpochFloorStoreError> {
        self.floors
            .lock()
            .map(|floors| floors.clone())
            .map_err(|_| EpochFloorStoreError::Unavailable("test store lock poisoned".into()))
    }

    fn compare_and_advance(
        &mut self,
        zone_id: ZoneId,
        expected_epoch: u64,
        next_epoch: u64,
    ) -> Result<(), EpochFloorStoreError> {
        if self.reject_next.swap(false, Ordering::SeqCst) {
            return Err(EpochFloorStoreError::Unavailable(
                "injected persistence failure".into(),
            ));
        }
        let mut floors = self
            .floors
            .lock()
            .map_err(|_| EpochFloorStoreError::Unavailable("test store lock poisoned".into()))?;
        let actual_epoch = floors.get(&zone_id).copied().unwrap_or(0);
        if actual_epoch != expected_epoch {
            return Err(EpochFloorStoreError::Conflict {
                zone_id,
                expected_epoch,
                actual_epoch,
            });
        }
        if expected_epoch.checked_add(1) != Some(next_epoch) {
            return Err(EpochFloorStoreError::Unavailable(
                "non-monotonic epoch advance".into(),
            ));
        }
        floors.insert(zone_id, next_epoch);
        Ok(())
    }
}

#[test]
fn lease_renewal_preserves_handoff_identity_and_retries() {
    let mut directory = ZoneDirectory::new(10).unwrap();
    let hosts = registered_hosts();
    let source = directory
        .assign(ZoneId::new(1), host("a"), &hosts, 0)
        .unwrap();
    let destination = directory
        .assign(ZoneId::new(2), host("b"), &hosts, 0)
        .unwrap();
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
    let hosts = registered_hosts();
    let source = directory
        .assign(ZoneId::new(1), host("a"), &hosts, 0)
        .unwrap();
    let destination = directory
        .assign(ZoneId::new(2), host("b"), &hosts, 0)
        .unwrap();
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
    let hosts = registered_hosts();
    let zone = ZoneId::new(1);
    let lease = directory.assign(zone, host("a"), &hosts, 0).unwrap();
    let floor = directory.epoch_floor_snapshot();
    assert!(
        directory
            .assign(zone, host("b"), &hosts, u64::MAX - 1)
            .is_err()
    );
    assert_eq!(directory.epoch_floor_snapshot(), floor);
    assert_eq!(directory.lease(zone), Some(&lease));
    assert!(
        directory
            .reassign(zone, lease.epoch, host("b"), &hosts, u64::MAX - 1)
            .is_err()
    );
    assert_eq!(directory.epoch_floor_snapshot(), floor);
    assert_eq!(directory.lease(zone), Some(&lease));
}

#[test]
fn retrying_a_committed_transfer_does_not_release_a_newer_reservation() {
    let mut directory = ZoneDirectory::new(10).unwrap();
    let hosts = registered_hosts();
    let source = directory
        .assign(ZoneId::new(1), host("a"), &hosts, 0)
        .unwrap();
    let destination = directory
        .assign(ZoneId::new(2), host("b"), &hosts, 0)
        .unwrap();
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

#[test]
fn persisted_epoch_floor_advances_across_directory_restart() {
    let store = SharedEpochStore::default();
    let hosts = registered_hosts();
    let zone = ZoneId::new(41);

    let first = {
        let mut directory = EpochPersistedZoneDirectory::from_store(10, store.clone()).unwrap();
        directory.assign(zone, host("a"), &hosts, 0).unwrap()
    };
    assert_eq!(first.epoch, 1);
    assert_eq!(store.floor(zone), Some(1));

    let mut restarted = EpochPersistedZoneDirectory::from_store(10, store.clone()).unwrap();
    let second = restarted.assign(zone, host("b"), &hosts, 10).unwrap();
    assert_eq!(second.epoch, 2);
    assert_eq!(store.floor(zone), Some(2));
}

#[test]
fn concurrent_epoch_allocator_conflict_fails_closed() {
    let store = SharedEpochStore::default();
    let hosts = registered_hosts();
    let zone = ZoneId::new(42);
    let mut first = EpochPersistedZoneDirectory::from_store(10, store.clone()).unwrap();
    let mut stale = EpochPersistedZoneDirectory::from_store(10, store.clone()).unwrap();

    first.assign(zone, host("a"), &hosts, 0).unwrap();
    let error = stale.assign(zone, host("b"), &hosts, 0).unwrap_err();

    assert!(matches!(
        error,
        EpochPersistedDirectoryError::Store(EpochFloorStoreError::Conflict {
            zone_id,
            expected_epoch: 0,
            actual_epoch: 1,
        }) if zone_id == zone
    ));
    assert!(stale.lease(zone).is_none());
    assert!(stale.epoch_floor_snapshot().is_empty());
    assert_eq!(store.floor(zone), Some(1));
}

#[test]
fn epoch_store_failure_never_acknowledges_a_lease() {
    let store = SharedEpochStore::default();
    store.reject_next();
    let hosts = registered_hosts();
    let zone = ZoneId::new(43);
    let mut directory = EpochPersistedZoneDirectory::from_store(10, store.clone()).unwrap();

    let error = directory.assign(zone, host("a"), &hosts, 0).unwrap_err();

    assert!(matches!(
        error,
        EpochPersistedDirectoryError::Store(EpochFloorStoreError::Unavailable(_))
    ));
    assert!(directory.lease(zone).is_none());
    assert!(directory.epoch_floor_snapshot().is_empty());
    assert_eq!(store.floor(zone), None);
}
