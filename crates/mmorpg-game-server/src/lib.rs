#![forbid(unsafe_code)]

use game_server::{
    GameSimulation, HostError, MatchHost, MatchId, MatchIdError, MatchRuntime, SimulationError,
    SimulationSnapshot, SnapshotScope,
};
use mmorpg_core::{MAX_PLAYERS_PER_ZONE, TICK_HZ, ZoneId, ZoneSimulation};
use mmorpg_protocol::{decode_command, encode_canonical_snapshot, encode_snapshot};

pub const PINNED_GAME_SERVER_REVISION: &str = "769de47005cc37891011fc76ae183c18b7c5e0ae";
pub const PINNED_PHYSICS_ENGINE_REVISION: &str = "c796ea382bdcb0276b9309e8a3cca34c8c28313b";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ZoneHostBuildError {
    Empty,
    DuplicateZone(ZoneId),
    MatchId(MatchIdError),
    Host(HostError),
}

impl std::fmt::Display for ZoneHostBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("zone host must contain at least one zone"),
            Self::DuplicateZone(zone_id) => {
                write!(
                    formatter,
                    "zone {} is configured more than once",
                    zone_id.get()
                )
            }
            Self::MatchId(error) => error.fmt(formatter),
            Self::Host(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ZoneHostBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MatchId(error) => Some(error),
            Self::Host(error) => Some(error),
            Self::Empty | Self::DuplicateZone(_) => None,
        }
    }
}

pub struct ZoneGameServerAdapter {
    zone: ZoneSimulation,
}

impl ZoneGameServerAdapter {
    #[must_use]
    pub fn new(zone_id: ZoneId) -> Self {
        Self {
            zone: ZoneSimulation::new(zone_id),
        }
    }

    #[must_use]
    pub fn zone(&self) -> &ZoneSimulation {
        &self.zone
    }

    pub fn zone_mut(&mut self) -> &mut ZoneSimulation {
        &mut self.zone
    }

    #[must_use]
    pub fn into_zone(self) -> ZoneSimulation {
        self.zone
    }
}

pub fn zone_match_id(zone_id: ZoneId) -> Result<MatchId, MatchIdError> {
    MatchId::new(format!("zone-{}", zone_id.get()))
}

pub fn build_zone_host(
    zone_ids: impl IntoIterator<Item = ZoneId>,
    reconnect_grace_ticks: u64,
) -> Result<MatchHost<ZoneGameServerAdapter>, ZoneHostBuildError> {
    let zone_ids = zone_ids.into_iter().collect::<Vec<_>>();
    if zone_ids.is_empty() {
        return Err(ZoneHostBuildError::Empty);
    }

    let mut ordered = zone_ids.clone();
    ordered.sort_unstable();
    if let Some(duplicate) = ordered.windows(2).find_map(|pair| {
        let [left, right] = pair else {
            return None;
        };
        (left == right).then_some(*left)
    }) {
        return Err(ZoneHostBuildError::DuplicateZone(duplicate));
    }

    let mut host = MatchHost::new(zone_ids.len()).map_err(ZoneHostBuildError::Host)?;
    for zone_id in zone_ids {
        let match_id = zone_match_id(zone_id).map_err(ZoneHostBuildError::MatchId)?;
        let runtime = MatchRuntime::new(ZoneGameServerAdapter::new(zone_id), reconnect_grace_ticks);
        host.insert(match_id, runtime).map_err(|failure| {
            let (error, _, _) = failure.into_parts();
            ZoneHostBuildError::Host(error)
        })?;
    }
    Ok(host)
}

impl GameSimulation for ZoneGameServerAdapter {
    fn tick_hz(&self) -> u16 {
        TICK_HZ
    }

    fn max_players(&self) -> usize {
        MAX_PLAYERS_PER_ZONE
    }

    fn current_tick(&self) -> u64 {
        self.zone.current_tick()
    }

    fn add_player(&mut self, player_id: game_server::PlayerId) -> Result<(), SimulationError> {
        self.zone.add_player(player_id).map_err(map_zone_error)
    }

    fn remove_player(&mut self, player_id: game_server::PlayerId) -> bool {
        self.zone.remove_player(player_id)
    }

    fn apply_command(
        &mut self,
        player_id: game_server::PlayerId,
        sequence: u32,
        payload: &[u8],
    ) -> Result<(), SimulationError> {
        let command = decode_command(payload).map_err(map_protocol_error)?;
        self.zone
            .apply_command(player_id, sequence, command)
            .map_err(map_zone_error)
    }

    fn advance_tick(&mut self) -> Result<(), SimulationError> {
        self.zone.advance_tick().map_err(map_zone_error)
    }

    fn snapshot_scope(&self) -> SnapshotScope {
        SnapshotScope::PlayerScoped
    }

    fn snapshot(&self) -> Result<SimulationSnapshot, SimulationError> {
        let snapshot = self.zone.snapshot().map_err(map_zone_error)?;
        let payload = encode_canonical_snapshot(&snapshot).map_err(map_protocol_error)?;
        Ok(SimulationSnapshot::new(snapshot.tick, payload))
    }

    fn snapshot_for(
        &self,
        player_id: game_server::PlayerId,
    ) -> Result<SimulationSnapshot, SimulationError> {
        let snapshot = self
            .zone
            .snapshot_for_player(player_id)
            .map_err(map_zone_error)?;
        let payload = encode_snapshot(&snapshot).map_err(map_protocol_error)?;
        Ok(SimulationSnapshot::new(snapshot.tick, payload))
    }
}

fn map_zone_error(error: mmorpg_core::ZoneError) -> SimulationError {
    SimulationError::new(error.to_string())
}

fn map_protocol_error(error: mmorpg_protocol::ProtocolError) -> SimulationError {
    SimulationError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_server::{MatchRuntime, RECONNECT_TOKEN_BYTES, ReconnectToken};
    use mmorpg_core::ZoneCommand;
    use mmorpg_protocol::{decode_canonical_snapshot, decode_snapshot, encode_command};

    #[test]
    fn zones_map_to_stable_route_safe_match_ids() {
        assert_eq!(zone_match_id(ZoneId::new(42)).unwrap().as_str(), "zone-42");
    }

    #[test]
    fn adapter_keeps_canonical_and_player_scoped_snapshots_separate() {
        let mut adapter = ZoneGameServerAdapter::new(ZoneId::new(7));
        for player_id in 1..=12 {
            adapter.add_player(player_id).unwrap();
        }
        adapter
            .zone_mut()
            .apply_command(1, 9, ZoneCommand::SetMovement { x: 1, z: 0 })
            .unwrap();

        let canonical = decode_canonical_snapshot(&adapter.snapshot().unwrap().payload).unwrap();
        let projected = decode_snapshot(&adapter.snapshot_for(1).unwrap().payload).unwrap();

        assert_eq!(adapter.snapshot_scope(), SnapshotScope::PlayerScoped);
        assert_eq!(canonical.players.len(), 12);
        assert_eq!(canonical.players[0].movement_x, 1);
        assert_eq!(canonical.players[0].last_sequence, 9);
        assert_eq!(projected.players.len(), 11);
        assert_eq!(projected.players[0].player_id, 1);
        assert!(
            projected
                .players
                .iter()
                .all(|player| player.player_id != 12)
        );
    }

    #[test]
    fn shared_game_server_runtime_drives_the_same_zone_authority() {
        let adapter = ZoneGameServerAdapter::new(ZoneId::new(9));
        let mut runtime = MatchRuntime::new(adapter, 120);
        let lease = runtime
            .admit(ReconnectToken([7; RECONNECT_TOKEN_BYTES]))
            .unwrap();
        let payload = encode_command(ZoneCommand::SetMovement { x: 1, z: 0 });

        runtime
            .submit_command(lease.player_id, lease.connection_epoch, 1, &payload)
            .unwrap();
        for _ in 0..5 {
            runtime.advance_tick().unwrap();
        }

        let snapshot =
            decode_snapshot(&runtime.snapshot_for(lease.player_id).unwrap().payload).unwrap();
        assert_eq!(snapshot.zone_id, ZoneId::new(9));
        assert_eq!(snapshot.tick, 5);
        assert_eq!(snapshot.players.len(), 1);
        assert!(snapshot.players[0].position[0] > 0);
    }

    #[test]
    fn host_rejects_empty_and_duplicate_zone_sets() {
        assert_eq!(
            build_zone_host([], 120).err(),
            Some(ZoneHostBuildError::Empty)
        );
        assert_eq!(
            build_zone_host([ZoneId::new(4), ZoneId::new(4)], 120).err(),
            Some(ZoneHostBuildError::DuplicateZone(ZoneId::new(4)))
        );
    }

    #[test]
    fn one_host_advances_zones_independently() {
        let first_zone = ZoneId::new(10);
        let second_zone = ZoneId::new(11);
        let first_match = zone_match_id(first_zone).unwrap();
        let second_match = zone_match_id(second_zone).unwrap();
        let mut host = build_zone_host([first_zone, second_zone], 120).unwrap();

        let first_lease = host
            .with_runtime_mut(&first_match, |runtime| {
                runtime.admit(ReconnectToken([1; RECONNECT_TOKEN_BYTES]))
            })
            .unwrap()
            .unwrap();
        let second_lease = host
            .with_runtime_mut(&second_match, |runtime| {
                runtime.admit(ReconnectToken([2; RECONNECT_TOKEN_BYTES]))
            })
            .unwrap()
            .unwrap();

        let first_command = encode_command(ZoneCommand::SetMovement { x: 1, z: 0 });
        let second_command = encode_command(ZoneCommand::SetMovement { x: 0, z: -1 });
        host.with_runtime_mut(&first_match, |runtime| {
            runtime.submit_command(
                first_lease.player_id,
                first_lease.connection_epoch,
                1,
                &first_command,
            )
        })
        .unwrap()
        .unwrap();
        host.with_runtime_mut(&second_match, |runtime| {
            runtime.submit_command(
                second_lease.player_id,
                second_lease.connection_epoch,
                1,
                &second_command,
            )
        })
        .unwrap()
        .unwrap();

        for _ in 0..3 {
            host.with_runtime_mut(&first_match, MatchRuntime::advance_tick)
                .unwrap()
                .unwrap();
        }
        host.with_runtime_mut(&second_match, MatchRuntime::advance_tick)
            .unwrap()
            .unwrap();

        let first_snapshot = host
            .with_runtime_mut(&first_match, |runtime| {
                runtime.snapshot_for(first_lease.player_id)
            })
            .unwrap()
            .unwrap();
        let second_snapshot = host
            .with_runtime_mut(&second_match, |runtime| {
                runtime.snapshot_for(second_lease.player_id)
            })
            .unwrap()
            .unwrap();
        let first_snapshot = decode_snapshot(&first_snapshot.payload).unwrap();
        let second_snapshot = decode_snapshot(&second_snapshot.payload).unwrap();

        assert_eq!(first_snapshot.zone_id, first_zone);
        assert_eq!(first_snapshot.tick, 3);
        assert!(first_snapshot.players[0].position[0] > 0);
        assert_eq!(second_snapshot.zone_id, second_zone);
        assert_eq!(second_snapshot.tick, 1);
        assert!(second_snapshot.players[0].position[2] < 0);
    }
}
