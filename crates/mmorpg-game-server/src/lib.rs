#![forbid(unsafe_code)]

use game_server::{
    GameSimulation, MatchId, MatchIdError, SimulationError, SimulationSnapshot, SnapshotScope,
};
use mmorpg_core::{MAX_PLAYERS_PER_ZONE, TICK_HZ, ZoneId, ZoneSimulation};
use mmorpg_protocol::{decode_command, encode_canonical_snapshot, encode_snapshot};

pub const PINNED_GAME_SERVER_REVISION: &str = "769de47005cc37891011fc76ae183c18b7c5e0ae";
pub const PINNED_PHYSICS_ENGINE_REVISION: &str = "c796ea382bdcb0276b9309e8a3cca34c8c28313b";

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
        adapter.add_player(1).unwrap();
        adapter.add_player(15).unwrap();
        adapter
            .zone_mut()
            .apply_command(1, 9, ZoneCommand::SetMovement { x: 1, z: 0 })
            .unwrap();

        let canonical = decode_canonical_snapshot(&adapter.snapshot().unwrap().payload).unwrap();
        let projected = decode_snapshot(&adapter.snapshot_for(1).unwrap().payload).unwrap();

        assert_eq!(adapter.snapshot_scope(), SnapshotScope::PlayerScoped);
        assert_eq!(canonical.players.len(), 2);
        assert_eq!(canonical.players[0].movement_x, 1);
        assert_eq!(canonical.players[0].last_sequence, 9);
        assert_eq!(projected.players.len(), 1);
        assert_eq!(projected.players[0].player_id, 1);
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
}
