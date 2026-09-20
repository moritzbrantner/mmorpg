//! In-process reference composition of zone leases and the shared session runtime.
//!
//! The caller supplies authoritative, monotonic control-plane time, independently
//! of simulation ticks. The directory borrow spans the entire operation, preventing
//! reassignment between validation and mutation in this reference model. A remote
//! implementation needs a local expiring permit and fencing at every durable sink;
//! a cached copy of `ZoneDirectory` is not a distributed authority service.

use game_server::{MatchRuntime, RuntimeError};
use mmorpg_control_plane::{ControlPlaneError, ZoneDirectory, ZoneLease};
use std::{error::Error, fmt};

use crate::ZoneGameServerAdapter;

#[derive(Debug)]
pub enum FencedRuntimeError {
    Authority(ControlPlaneError),
    TimeWentBackwards,
    Runtime(RuntimeError),
}

impl fmt::Display for FencedRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authority(error) => error.fmt(formatter),
            Self::TimeWentBackwards => formatter.write_str("control-plane time went backwards"),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl Error for FencedRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authority(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::TimeWentBackwards => None,
        }
    }
}

/// Owns a runtime that can only be accessed while its lease is current.
///
/// Keep operations synchronous and bounded. The callback is trusted server code;
/// it must not extract/replace the runtime or publish work after this scope ends.
/// Session IDs remain local to this runtime, not durable character identities.
pub struct FencedZoneRuntime {
    lease: ZoneLease,
    observed_at_tick: u64,
    runtime: MatchRuntime<ZoneGameServerAdapter>,
}

impl FencedZoneRuntime {
    pub fn new(
        lease: ZoneLease,
        directory: &ZoneDirectory,
        now_tick: u64,
        reconnect_grace_ticks: u64,
    ) -> Result<Self, FencedRuntimeError> {
        directory
            .ensure_current(&lease, now_tick)
            .map_err(FencedRuntimeError::Authority)?;
        let runtime = MatchRuntime::new(
            ZoneGameServerAdapter::new(lease.zone_id),
            reconnect_grace_ticks,
        );
        Ok(Self {
            lease,
            observed_at_tick: now_tick,
            runtime,
        })
    }

    /// Fences admission, commands, ticks, reconnects, and snapshot publication
    /// through the same interface without duplicating `game-server` semantics.
    pub fn execute<T>(
        &mut self,
        directory: &ZoneDirectory,
        now_tick: u64,
        operation: impl FnOnce(&mut MatchRuntime<ZoneGameServerAdapter>) -> Result<T, RuntimeError>,
    ) -> Result<T, FencedRuntimeError> {
        if now_tick < self.observed_at_tick {
            return Err(FencedRuntimeError::TimeWentBackwards);
        }
        self.observed_at_tick = now_tick;
        directory
            .ensure_current(&self.lease, now_tick)
            .map_err(FencedRuntimeError::Authority)?;
        operation(&mut self.runtime).map_err(FencedRuntimeError::Runtime)
    }
}
