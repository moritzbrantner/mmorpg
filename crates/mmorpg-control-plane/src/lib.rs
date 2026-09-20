#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use mmorpg_core::ZoneId;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct HostId(String);

impl HostId {
    pub fn new(value: impl Into<String>) -> Result<Self, ControlPlaneError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ControlPlaneError::InvalidHostId);
        }
        if value.len() > 128
            || value.chars().any(|character| {
                !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
            })
        {
            return Err(ControlPlaneError::InvalidHostId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneLease {
    pub zone_id: ZoneId,
    pub host_id: HostId,
    pub epoch: u64,
    pub expires_at_tick: u64,
}

impl ZoneLease {
    /// Renewal changes the deadline, never the identity of an authority grant.
    #[must_use]
    pub fn same_authority(&self, other: &Self) -> bool {
        self.zone_id == other.zone_id && self.host_id == other.host_id && self.epoch == other.epoch
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlPlaneError {
    InvalidHostId,
    InvalidLeaseTtl,
    LeaseDeadlineOverflow(ZoneId),
    LeaseExpired {
        zone_id: ZoneId,
        epoch: u64,
        expires_at_tick: u64,
        observed_at_tick: u64,
    },
    ZoneAlreadyAssigned(ZoneId),
    ZoneNotAssigned(ZoneId),
    StaleLease {
        zone_id: ZoneId,
        expected_epoch: u64,
        actual_epoch: u64,
    },
    LeaseOwnerMismatch(ZoneId),
    EpochExhausted(ZoneId),
    TransferIdCollision(TransferId),
    UnknownTransfer(TransferId),
    SameZoneTransfer(ZoneId),
    TransferLeaseMismatch(TransferId),
    TransferNotAccepted(TransferId),
    EntityTransferInProgress(EntityId),
}

impl fmt::Display for ControlPlaneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHostId => write!(formatter, "host id is invalid"),
            Self::InvalidLeaseTtl => write!(formatter, "zone lease TTL must be greater than zero"),
            Self::LeaseDeadlineOverflow(zone_id) => write!(
                formatter,
                "zone {} lease deadline overflowed the control-plane tick domain",
                zone_id.get()
            ),
            Self::LeaseExpired {
                zone_id,
                epoch,
                expires_at_tick,
                observed_at_tick,
            } => write!(
                formatter,
                "zone {} lease epoch {epoch} expired at tick {expires_at_tick}; observed at tick {observed_at_tick}",
                zone_id.get()
            ),
            Self::ZoneAlreadyAssigned(zone_id) => {
                write!(formatter, "zone {} is already assigned", zone_id.get())
            }
            Self::ZoneNotAssigned(zone_id) => {
                write!(formatter, "zone {} is not assigned", zone_id.get())
            }
            Self::StaleLease {
                zone_id,
                expected_epoch,
                actual_epoch,
            } => write!(
                formatter,
                "zone {} lease epoch {expected_epoch} is stale; current epoch is {actual_epoch}",
                zone_id.get()
            ),
            Self::LeaseOwnerMismatch(zone_id) => {
                write!(
                    formatter,
                    "zone {} lease owner does not match",
                    zone_id.get()
                )
            }
            Self::EpochExhausted(zone_id) => {
                write!(formatter, "zone {} lease epoch is exhausted", zone_id.get())
            }
            Self::TransferIdCollision(transfer_id) => {
                write!(
                    formatter,
                    "transfer {} was reused with different content",
                    transfer_id.get()
                )
            }
            Self::UnknownTransfer(transfer_id) => {
                write!(formatter, "transfer {} is unknown", transfer_id.get())
            }
            Self::SameZoneTransfer(zone_id) => {
                write!(
                    formatter,
                    "zone {} cannot hand off to itself",
                    zone_id.get()
                )
            }
            Self::TransferLeaseMismatch(transfer_id) => {
                write!(
                    formatter,
                    "transfer {} lease does not match",
                    transfer_id.get()
                )
            }
            Self::EntityTransferInProgress(entity_id) => {
                write!(
                    formatter,
                    "entity {} already has an active transfer",
                    entity_id.get()
                )
            }
            Self::TransferNotAccepted(transfer_id) => {
                write!(
                    formatter,
                    "transfer {} has not been accepted",
                    transfer_id.get()
                )
            }
        }
    }
}

impl Error for ControlPlaneError {}

pub struct ZoneDirectory {
    leases: BTreeMap<ZoneId, ZoneLease>,
    last_epoch: BTreeMap<ZoneId, u64>,
    lease_ttl_ticks: u64,
}

impl ZoneDirectory {
    pub fn new(lease_ttl_ticks: u64) -> Result<Self, ControlPlaneError> {
        Self::from_epoch_floor(lease_ttl_ticks, BTreeMap::new())
    }

    pub fn from_epoch_floor(
        lease_ttl_ticks: u64,
        last_epoch: BTreeMap<ZoneId, u64>,
    ) -> Result<Self, ControlPlaneError> {
        if lease_ttl_ticks == 0 {
            return Err(ControlPlaneError::InvalidLeaseTtl);
        }
        Ok(Self {
            leases: BTreeMap::new(),
            last_epoch,
            lease_ttl_ticks,
        })
    }

    #[must_use]
    pub fn epoch_floor_snapshot(&self) -> BTreeMap<ZoneId, u64> {
        self.last_epoch.clone()
    }

    #[must_use]
    pub const fn lease_ttl_ticks(&self) -> u64 {
        self.lease_ttl_ticks
    }

    #[must_use]
    pub fn lease(&self, zone_id: ZoneId) -> Option<&ZoneLease> {
        self.leases.get(&zone_id)
    }

    pub fn assign(
        &mut self,
        zone_id: ZoneId,
        host_id: HostId,
        now_tick: u64,
    ) -> Result<ZoneLease, ControlPlaneError> {
        if self
            .leases
            .get(&zone_id)
            .is_some_and(|lease| now_tick < lease.expires_at_tick)
        {
            return Err(ControlPlaneError::ZoneAlreadyAssigned(zone_id));
        }
        let expires_at_tick = self.lease_deadline(zone_id, now_tick)?;
        let epoch = self.next_epoch(zone_id)?;
        let lease = ZoneLease {
            zone_id,
            host_id,
            epoch,
            expires_at_tick,
        };
        self.leases.insert(zone_id, lease.clone());
        Ok(lease)
    }

    pub fn reassign(
        &mut self,
        zone_id: ZoneId,
        expected_epoch: u64,
        host_id: HostId,
        now_tick: u64,
    ) -> Result<ZoneLease, ControlPlaneError> {
        let current = self
            .leases
            .get(&zone_id)
            .ok_or(ControlPlaneError::ZoneNotAssigned(zone_id))?;
        if current.epoch != expected_epoch {
            return Err(ControlPlaneError::StaleLease {
                zone_id,
                expected_epoch,
                actual_epoch: current.epoch,
            });
        }

        let expires_at_tick = self.lease_deadline(zone_id, now_tick)?;
        let epoch = self.next_epoch(zone_id)?;
        let lease = ZoneLease {
            zone_id,
            host_id,
            epoch,
            expires_at_tick,
        };
        self.leases.insert(zone_id, lease.clone());
        Ok(lease)
    }

    pub fn renew(
        &mut self,
        lease: &ZoneLease,
        now_tick: u64,
    ) -> Result<ZoneLease, ControlPlaneError> {
        self.ensure_current(lease, now_tick)?;
        let expires_at_tick = self.lease_deadline(lease.zone_id, now_tick)?;
        let current = self
            .leases
            .get_mut(&lease.zone_id)
            .expect("current lease existence checked above");
        current.expires_at_tick = expires_at_tick;
        Ok(current.clone())
    }

    pub fn release(&mut self, lease: &ZoneLease, now_tick: u64) -> Result<(), ControlPlaneError> {
        self.ensure_current(lease, now_tick)?;
        self.leases.remove(&lease.zone_id);
        Ok(())
    }

    pub fn expire(&mut self, now_tick: u64) -> Vec<ZoneLease> {
        let expired = self
            .leases
            .iter()
            .filter_map(|(zone_id, lease)| (now_tick >= lease.expires_at_tick).then_some(*zone_id))
            .collect::<Vec<_>>();
        expired
            .into_iter()
            .filter_map(|zone_id| self.leases.remove(&zone_id))
            .collect()
    }

    pub fn ensure_current(
        &self,
        lease: &ZoneLease,
        now_tick: u64,
    ) -> Result<(), ControlPlaneError> {
        let current = self
            .leases
            .get(&lease.zone_id)
            .ok_or(ControlPlaneError::ZoneNotAssigned(lease.zone_id))?;
        if current.epoch != lease.epoch {
            return Err(ControlPlaneError::StaleLease {
                zone_id: lease.zone_id,
                expected_epoch: lease.epoch,
                actual_epoch: current.epoch,
            });
        }
        if current.host_id != lease.host_id {
            return Err(ControlPlaneError::LeaseOwnerMismatch(lease.zone_id));
        }
        if now_tick >= current.expires_at_tick {
            return Err(ControlPlaneError::LeaseExpired {
                zone_id: lease.zone_id,
                epoch: lease.epoch,
                expires_at_tick: current.expires_at_tick,
                observed_at_tick: now_tick,
            });
        }
        Ok(())
    }

    fn lease_deadline(&self, zone_id: ZoneId, now_tick: u64) -> Result<u64, ControlPlaneError> {
        now_tick
            .checked_add(self.lease_ttl_ticks)
            .ok_or(ControlPlaneError::LeaseDeadlineOverflow(zone_id))
    }

    fn next_epoch(&mut self, zone_id: ZoneId) -> Result<u64, ControlPlaneError> {
        let last = self.last_epoch.get(&zone_id).copied().unwrap_or(0);
        let next = last
            .checked_add(1)
            .ok_or(ControlPlaneError::EpochExhausted(zone_id))?;
        self.last_epoch.insert(zone_id, next);
        Ok(next)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityId(u64);

impl EntityId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransferId(u64);

impl TransferId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandoffTicket {
    pub transfer_id: TransferId,
    pub entity_id: EntityId,
    pub source: ZoneLease,
    pub destination: ZoneLease,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoffPhase {
    Prepared,
    Accepted,
    Committed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandoffRecord {
    pub ticket: HandoffTicket,
    pub phase: HandoffPhase,
}

#[derive(Default)]
pub struct HandoffRegistry {
    transfers: BTreeMap<TransferId, HandoffRecord>,
    active_entities: BTreeMap<EntityId, TransferId>,
}

impl HandoffRegistry {
    #[must_use]
    pub fn record(&self, transfer_id: TransferId) -> Option<&HandoffRecord> {
        self.transfers.get(&transfer_id)
    }

    pub fn prepare(
        &mut self,
        ticket: HandoffTicket,
        directory: &ZoneDirectory,
        now_tick: u64,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        if ticket.source.zone_id == ticket.destination.zone_id {
            return Err(ControlPlaneError::SameZoneTransfer(ticket.source.zone_id));
        }
        directory.ensure_current(&ticket.source, now_tick)?;
        directory.ensure_current(&ticket.destination, now_tick)?;

        if let Some(existing) = self.transfers.get(&ticket.transfer_id) {
            if existing.ticket.entity_id != ticket.entity_id
                || !existing.ticket.source.same_authority(&ticket.source)
                || !existing
                    .ticket
                    .destination
                    .same_authority(&ticket.destination)
            {
                return Err(ControlPlaneError::TransferIdCollision(ticket.transfer_id));
            }
            return Ok(existing.phase);
        }

        if self.active_entities.contains_key(&ticket.entity_id) {
            return Err(ControlPlaneError::EntityTransferInProgress(
                ticket.entity_id,
            ));
        }
        let transfer_id = ticket.transfer_id;
        self.active_entities.insert(ticket.entity_id, transfer_id);
        self.transfers.insert(
            transfer_id,
            HandoffRecord {
                ticket,
                phase: HandoffPhase::Prepared,
            },
        );
        Ok(HandoffPhase::Prepared)
    }

    pub fn accept(
        &mut self,
        transfer_id: TransferId,
        destination: &ZoneLease,
        directory: &ZoneDirectory,
        now_tick: u64,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        directory.ensure_current(destination, now_tick)?;
        let ticket = self
            .transfers
            .get(&transfer_id)
            .ok_or(ControlPlaneError::UnknownTransfer(transfer_id))?
            .ticket
            .clone();
        if !ticket.destination.same_authority(destination) {
            return Err(ControlPlaneError::TransferLeaseMismatch(transfer_id));
        }
        directory.ensure_current(&ticket.source, now_tick)?;

        let record = self
            .transfers
            .get_mut(&transfer_id)
            .expect("transfer existence checked above");
        if record.phase == HandoffPhase::Prepared {
            record.phase = HandoffPhase::Accepted;
        }
        Ok(record.phase)
    }

    pub fn commit(
        &mut self,
        transfer_id: TransferId,
        source: &ZoneLease,
        directory: &ZoneDirectory,
        now_tick: u64,
    ) -> Result<HandoffPhase, ControlPlaneError> {
        directory.ensure_current(source, now_tick)?;
        let ticket = self
            .transfers
            .get(&transfer_id)
            .ok_or(ControlPlaneError::UnknownTransfer(transfer_id))?
            .ticket
            .clone();
        if !ticket.source.same_authority(source) {
            return Err(ControlPlaneError::TransferLeaseMismatch(transfer_id));
        }
        directory.ensure_current(&ticket.destination, now_tick)?;

        let record = self
            .transfers
            .get_mut(&transfer_id)
            .expect("transfer existence checked above");
        match record.phase {
            HandoffPhase::Prepared => {
                return Err(ControlPlaneError::TransferNotAccepted(transfer_id));
            }
            HandoffPhase::Accepted => {
                record.phase = HandoffPhase::Committed;
                self.active_entities.remove(&record.ticket.entity_id);
            }
            HandoffPhase::Committed => {}
        }
        Ok(record.phase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(value: &str) -> HostId {
        HostId::new(value).unwrap()
    }

    #[test]
    fn reassignment_fences_the_previous_zone_owner() {
        let zone_id = ZoneId::new(10);
        let mut directory = ZoneDirectory::new(10).unwrap();
        let first = directory.assign(zone_id, host("host-a"), 0).unwrap();
        let second = directory
            .reassign(zone_id, first.epoch, host("host-b"), 1)
            .unwrap();

        assert!(matches!(
            directory.ensure_current(&first, 1),
            Err(ControlPlaneError::StaleLease { .. })
        ));
        assert!(directory.ensure_current(&second, 1).is_ok());
        assert!(second.epoch > first.epoch);
    }

    #[test]
    fn handoff_phases_are_idempotent_for_the_same_transfer() {
        let mut directory = ZoneDirectory::new(10).unwrap();
        let source = directory.assign(ZoneId::new(1), host("host-a"), 0).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b"), 0).unwrap();
        let ticket = HandoffTicket {
            transfer_id: TransferId::new(77),
            entity_id: EntityId::new(9001),
            source: source.clone(),
            destination: destination.clone(),
        };
        let mut registry = HandoffRegistry::default();

        assert_eq!(
            registry.prepare(ticket.clone(), &directory, 0).unwrap(),
            HandoffPhase::Prepared
        );
        assert_eq!(
            registry.prepare(ticket, &directory, 0).unwrap(),
            HandoffPhase::Prepared
        );
        assert_eq!(
            registry
                .accept(TransferId::new(77), &destination, &directory, 0)
                .unwrap(),
            HandoffPhase::Accepted
        );
        assert_eq!(
            registry
                .accept(TransferId::new(77), &destination, &directory, 0)
                .unwrap(),
            HandoffPhase::Accepted
        );
        assert_eq!(
            registry
                .commit(TransferId::new(77), &source, &directory, 0)
                .unwrap(),
            HandoffPhase::Committed
        );
        assert_eq!(
            registry
                .commit(TransferId::new(77), &source, &directory, 0)
                .unwrap(),
            HandoffPhase::Committed
        );
    }

    #[test]
    fn stale_source_epoch_cannot_accept_a_transfer() {
        let mut directory = ZoneDirectory::new(10).unwrap();
        let source = directory.assign(ZoneId::new(1), host("host-a"), 0).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b"), 0).unwrap();
        let mut registry = HandoffRegistry::default();
        registry
            .prepare(
                HandoffTicket {
                    transfer_id: TransferId::new(3),
                    entity_id: EntityId::new(5),
                    source: source.clone(),
                    destination: destination.clone(),
                },
                &directory,
                0,
            )
            .unwrap();

        directory
            .reassign(source.zone_id, source.epoch, host("host-c"), 1)
            .unwrap();

        assert!(matches!(
            registry.accept(TransferId::new(3), &destination, &directory, 1),
            Err(ControlPlaneError::StaleLease { .. })
        ));
    }

    #[test]
    fn stale_destination_epoch_cannot_commit_a_transfer() {
        let mut directory = ZoneDirectory::new(10).unwrap();
        let source = directory.assign(ZoneId::new(1), host("host-a"), 0).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b"), 0).unwrap();
        let mut registry = HandoffRegistry::default();
        registry
            .prepare(
                HandoffTicket {
                    transfer_id: TransferId::new(4),
                    entity_id: EntityId::new(5),
                    source: source.clone(),
                    destination: destination.clone(),
                },
                &directory,
                0,
            )
            .unwrap();
        registry
            .accept(TransferId::new(4), &destination, &directory, 0)
            .unwrap();

        directory
            .reassign(destination.zone_id, destination.epoch, host("host-c"), 1)
            .unwrap();

        assert!(matches!(
            registry.commit(TransferId::new(4), &source, &directory, 1),
            Err(ControlPlaneError::StaleLease { .. })
        ));
    }

    #[test]
    fn stale_destination_epoch_cannot_accept_a_transfer() {
        let mut directory = ZoneDirectory::new(10).unwrap();
        let source = directory.assign(ZoneId::new(1), host("host-a"), 0).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b"), 0).unwrap();
        let mut registry = HandoffRegistry::default();
        registry
            .prepare(
                HandoffTicket {
                    transfer_id: TransferId::new(4),
                    entity_id: EntityId::new(5),
                    source,
                    destination: destination.clone(),
                },
                &directory,
                0,
            )
            .unwrap();

        directory
            .reassign(destination.zone_id, destination.epoch, host("host-c"), 1)
            .unwrap();

        assert!(matches!(
            registry.accept(TransferId::new(4), &destination, &directory, 1),
            Err(ControlPlaneError::StaleLease { .. })
        ));
    }

    #[test]
    fn lease_expiry_and_renewal_are_deterministic() {
        let zone_id = ZoneId::new(7);
        let mut directory = ZoneDirectory::new(10).unwrap();
        let lease = directory.assign(zone_id, host("host-a"), 5).unwrap();

        assert_eq!(lease.expires_at_tick, 15);
        assert!(directory.ensure_current(&lease, 14).is_ok());
        assert!(matches!(
            directory.ensure_current(&lease, 15),
            Err(ControlPlaneError::LeaseExpired { .. })
        ));

        let replacement = directory
            .reassign(zone_id, lease.epoch, host("host-b"), 15)
            .unwrap();
        assert_eq!(replacement.epoch, lease.epoch + 1);
        assert_eq!(replacement.expires_at_tick, 25);

        let renewed = directory.renew(&replacement, 20).unwrap();
        assert_eq!(renewed.epoch, replacement.epoch);
        assert_eq!(renewed.expires_at_tick, 30);
        assert!(directory.ensure_current(&replacement, 29).is_ok());
    }

    #[test]
    fn expired_assignment_advances_epoch_instead_of_resurrecting_it() {
        let zone_id = ZoneId::new(8);
        let mut directory = ZoneDirectory::new(10).unwrap();
        let first = directory.assign(zone_id, host("host-a"), 0).unwrap();

        let second = directory.assign(zone_id, host("host-b"), 10).unwrap();

        assert_eq!(second.epoch, first.epoch + 1);
        assert!(matches!(
            directory.ensure_current(&first, 10),
            Err(ControlPlaneError::StaleLease { .. })
        ));
    }

    #[test]
    fn epoch_floor_survives_directory_restart() {
        let zone_id = ZoneId::new(9);
        let mut directory = ZoneDirectory::new(10).unwrap();
        let first = directory.assign(zone_id, host("host-a"), 0).unwrap();
        let snapshot = directory.epoch_floor_snapshot();

        let mut restored = ZoneDirectory::from_epoch_floor(10, snapshot).unwrap();
        let second = restored.assign(zone_id, host("host-b"), 100).unwrap();

        assert_eq!(second.epoch, first.epoch + 1);
    }

    #[test]
    fn expired_handoff_cannot_advance() {
        let mut directory = ZoneDirectory::new(10).unwrap();
        let source = directory.assign(ZoneId::new(1), host("host-a"), 0).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b"), 0).unwrap();
        let mut registry = HandoffRegistry::default();
        registry
            .prepare(
                HandoffTicket {
                    transfer_id: TransferId::new(99),
                    entity_id: EntityId::new(5),
                    source: source.clone(),
                    destination: destination.clone(),
                },
                &directory,
                0,
            )
            .unwrap();

        assert!(matches!(
            registry.accept(TransferId::new(99), &destination, &directory, 10),
            Err(ControlPlaneError::LeaseExpired { .. })
        ));
    }

    #[test]
    fn transfer_ids_cannot_be_reused_for_different_content() {
        let mut directory = ZoneDirectory::new(10).unwrap();
        let source = directory.assign(ZoneId::new(1), host("host-a"), 0).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b"), 0).unwrap();
        let mut registry = HandoffRegistry::default();

        registry
            .prepare(
                HandoffTicket {
                    transfer_id: TransferId::new(1),
                    entity_id: EntityId::new(10),
                    source: source.clone(),
                    destination: destination.clone(),
                },
                &directory,
                0,
            )
            .unwrap();

        let error = registry
            .prepare(
                HandoffTicket {
                    transfer_id: TransferId::new(1),
                    entity_id: EntityId::new(11),
                    source,
                    destination,
                },
                &directory,
                0,
            )
            .unwrap_err();
        assert_eq!(
            error,
            ControlPlaneError::TransferIdCollision(TransferId::new(1))
        );
    }
}
