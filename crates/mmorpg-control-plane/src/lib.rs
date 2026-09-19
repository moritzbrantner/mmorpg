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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlPlaneError {
    InvalidHostId,
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
}

impl fmt::Display for ControlPlaneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHostId => write!(formatter, "host id is invalid"),
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

#[derive(Default)]
pub struct ZoneDirectory {
    leases: BTreeMap<ZoneId, ZoneLease>,
    last_epoch: BTreeMap<ZoneId, u64>,
}

impl ZoneDirectory {
    #[must_use]
    pub fn lease(&self, zone_id: ZoneId) -> Option<&ZoneLease> {
        self.leases.get(&zone_id)
    }

    pub fn assign(
        &mut self,
        zone_id: ZoneId,
        host_id: HostId,
    ) -> Result<ZoneLease, ControlPlaneError> {
        if self.leases.contains_key(&zone_id) {
            return Err(ControlPlaneError::ZoneAlreadyAssigned(zone_id));
        }
        let epoch = self.next_epoch(zone_id)?;
        let lease = ZoneLease {
            zone_id,
            host_id,
            epoch,
        };
        self.leases.insert(zone_id, lease.clone());
        Ok(lease)
    }

    pub fn reassign(
        &mut self,
        zone_id: ZoneId,
        expected_epoch: u64,
        host_id: HostId,
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

        let epoch = self.next_epoch(zone_id)?;
        let lease = ZoneLease {
            zone_id,
            host_id,
            epoch,
        };
        self.leases.insert(zone_id, lease.clone());
        Ok(lease)
    }

    pub fn release(&mut self, lease: &ZoneLease) -> Result<(), ControlPlaneError> {
        self.ensure_current(lease)?;
        self.leases.remove(&lease.zone_id);
        Ok(())
    }

    pub fn ensure_current(&self, lease: &ZoneLease) -> Result<(), ControlPlaneError> {
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
        Ok(())
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
    ) -> Result<HandoffPhase, ControlPlaneError> {
        if ticket.source.zone_id == ticket.destination.zone_id {
            return Err(ControlPlaneError::SameZoneTransfer(ticket.source.zone_id));
        }
        directory.ensure_current(&ticket.source)?;
        directory.ensure_current(&ticket.destination)?;

        if let Some(existing) = self.transfers.get(&ticket.transfer_id) {
            if existing.ticket != ticket {
                return Err(ControlPlaneError::TransferIdCollision(ticket.transfer_id));
            }
            return Ok(existing.phase);
        }

        let transfer_id = ticket.transfer_id;
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
    ) -> Result<HandoffPhase, ControlPlaneError> {
        directory.ensure_current(destination)?;
        let ticket = self
            .transfers
            .get(&transfer_id)
            .ok_or(ControlPlaneError::UnknownTransfer(transfer_id))?
            .ticket
            .clone();
        if ticket.destination != *destination {
            return Err(ControlPlaneError::TransferLeaseMismatch(transfer_id));
        }
        directory.ensure_current(&ticket.source)?;

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
    ) -> Result<HandoffPhase, ControlPlaneError> {
        directory.ensure_current(source)?;
        let ticket = self
            .transfers
            .get(&transfer_id)
            .ok_or(ControlPlaneError::UnknownTransfer(transfer_id))?
            .ticket
            .clone();
        if ticket.source != *source {
            return Err(ControlPlaneError::TransferLeaseMismatch(transfer_id));
        }
        directory.ensure_current(&ticket.destination)?;

        let record = self
            .transfers
            .get_mut(&transfer_id)
            .expect("transfer existence checked above");
        match record.phase {
            HandoffPhase::Prepared => {
                return Err(ControlPlaneError::TransferNotAccepted(transfer_id));
            }
            HandoffPhase::Accepted => record.phase = HandoffPhase::Committed,
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
        let mut directory = ZoneDirectory::default();
        let first = directory.assign(zone_id, host("host-a")).unwrap();
        let second = directory
            .reassign(zone_id, first.epoch, host("host-b"))
            .unwrap();

        assert!(matches!(
            directory.ensure_current(&first),
            Err(ControlPlaneError::StaleLease { .. })
        ));
        assert!(directory.ensure_current(&second).is_ok());
        assert!(second.epoch > first.epoch);
    }

    #[test]
    fn handoff_phases_are_idempotent_for_the_same_transfer() {
        let mut directory = ZoneDirectory::default();
        let source = directory.assign(ZoneId::new(1), host("host-a")).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b")).unwrap();
        let ticket = HandoffTicket {
            transfer_id: TransferId::new(77),
            entity_id: EntityId::new(9001),
            source: source.clone(),
            destination: destination.clone(),
        };
        let mut registry = HandoffRegistry::default();

        assert_eq!(
            registry.prepare(ticket.clone(), &directory).unwrap(),
            HandoffPhase::Prepared
        );
        assert_eq!(
            registry.prepare(ticket, &directory).unwrap(),
            HandoffPhase::Prepared
        );
        assert_eq!(
            registry
                .accept(TransferId::new(77), &destination, &directory)
                .unwrap(),
            HandoffPhase::Accepted
        );
        assert_eq!(
            registry
                .accept(TransferId::new(77), &destination, &directory)
                .unwrap(),
            HandoffPhase::Accepted
        );
        assert_eq!(
            registry
                .commit(TransferId::new(77), &source, &directory)
                .unwrap(),
            HandoffPhase::Committed
        );
        assert_eq!(
            registry
                .commit(TransferId::new(77), &source, &directory)
                .unwrap(),
            HandoffPhase::Committed
        );
    }

    #[test]
    fn stale_source_epoch_cannot_accept_a_transfer() {
        let mut directory = ZoneDirectory::default();
        let source = directory.assign(ZoneId::new(1), host("host-a")).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b")).unwrap();
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
            )
            .unwrap();

        directory
            .reassign(source.zone_id, source.epoch, host("host-c"))
            .unwrap();

        assert!(matches!(
            registry.accept(TransferId::new(3), &destination, &directory),
            Err(ControlPlaneError::StaleLease { .. })
        ));
    }

    #[test]
    fn stale_destination_epoch_cannot_commit_a_transfer() {
        let mut directory = ZoneDirectory::default();
        let source = directory.assign(ZoneId::new(1), host("host-a")).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b")).unwrap();
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
            )
            .unwrap();
        registry
            .accept(TransferId::new(4), &destination, &directory)
            .unwrap();

        directory
            .reassign(destination.zone_id, destination.epoch, host("host-c"))
            .unwrap();

        assert!(matches!(
            registry.commit(TransferId::new(4), &source, &directory),
            Err(ControlPlaneError::StaleLease { .. })
        ));
    }

    #[test]
    fn stale_destination_epoch_cannot_accept_a_transfer() {
        let mut directory = ZoneDirectory::default();
        let source = directory.assign(ZoneId::new(1), host("host-a")).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b")).unwrap();
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
            )
            .unwrap();

        directory
            .reassign(destination.zone_id, destination.epoch, host("host-c"))
            .unwrap();

        assert!(matches!(
            registry.accept(TransferId::new(4), &destination, &directory),
            Err(ControlPlaneError::StaleLease { .. })
        ));
    }

    #[test]
    fn transfer_ids_cannot_be_reused_for_different_content() {
        let mut directory = ZoneDirectory::default();
        let source = directory.assign(ZoneId::new(1), host("host-a")).unwrap();
        let destination = directory.assign(ZoneId::new(2), host("host-b")).unwrap();
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
            )
            .unwrap_err();
        assert_eq!(
            error,
            ControlPlaneError::TransferIdCollision(TransferId::new(1))
        );
    }
}
