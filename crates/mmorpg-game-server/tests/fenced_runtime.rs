use game_server::{MatchRuntime, RECONNECT_TOKEN_BYTES, ReconnectToken};
use mmorpg_control_plane::{ControlPlaneError, HostId, HostRegistry, ZoneDirectory};
use mmorpg_core::{ZoneCommand, ZoneId};
use mmorpg_game_server::{FencedRuntimeError, FencedZoneRuntime};
use mmorpg_protocol::{decode_snapshot, encode_command};

fn registered_hosts() -> HostRegistry {
    let mut hosts = HostRegistry::new(100).unwrap();
    for name in ["a", "b"] {
        hosts.register(HostId::new(name).unwrap(), 0).unwrap();
    }
    hosts
}

#[test]
fn reassignment_stops_old_host_commands_ticks_admission_and_publication() {
    let mut directory = ZoneDirectory::new(10).unwrap();
    let hosts = registered_hosts();
    let zone = ZoneId::new(1);
    let lease = directory
        .assign(zone, HostId::new("a").unwrap(), &hosts, 0)
        .unwrap();
    let mut old = FencedZoneRuntime::new(lease.clone(), &directory, 0, 120).unwrap();
    let player = old
        .execute(&directory, 0, |runtime| {
            runtime.admit(ReconnectToken([1; RECONNECT_TOKEN_BYTES]))
        })
        .unwrap();
    let replacement = directory
        .reassign(zone, lease.epoch, HostId::new("b").unwrap(), &hosts, 1)
        .unwrap();
    let mut new = FencedZoneRuntime::new(replacement, &directory, 1, 120).unwrap();
    let command = encode_command(ZoneCommand::SetMovement { x: 1, z: 0 });
    assert!(
        old.execute(&directory, 1, |runtime| runtime.submit_command(
            player.player_id,
            player.connection_epoch,
            1,
            &command
        ))
        .is_err()
    );
    assert!(
        old.execute(&directory, 1, MatchRuntime::advance_tick)
            .is_err()
    );
    assert!(
        old.execute(&directory, 1, |runtime| runtime
            .admit(ReconnectToken([2; RECONNECT_TOKEN_BYTES])))
            .is_err()
    );
    assert!(
        old.execute(&directory, 1, |runtime| runtime
            .snapshot_for(player.player_id))
            .is_err()
    );
    assert!(
        new.execute(&directory, 1, MatchRuntime::advance_tick)
            .is_ok()
    );
}

#[test]
fn renewal_keeps_runtime_alive_but_expiry_and_time_regression_fail_closed() {
    let mut directory = ZoneDirectory::new(10).unwrap();
    let hosts = registered_hosts();
    let lease = directory
        .assign(ZoneId::new(1), HostId::new("a").unwrap(), &hosts, 0)
        .unwrap();
    let mut host = FencedZoneRuntime::new(lease.clone(), &directory, 0, 120).unwrap();
    let player = host
        .execute(&directory, 0, |runtime| {
            runtime.admit(ReconnectToken([1; RECONNECT_TOKEN_BYTES]))
        })
        .unwrap();
    directory.renew(&lease, 5).unwrap();
    host.execute(&directory, 10, MatchRuntime::advance_tick)
        .unwrap();
    let snapshot = host
        .execute(&directory, 10, |runtime| {
            runtime.snapshot_for(player.player_id)
        })
        .unwrap();
    assert_eq!(decode_snapshot(&snapshot.payload).unwrap().tick, 1);
    assert!(matches!(
        host.execute(&directory, 15, MatchRuntime::advance_tick),
        Err(FencedRuntimeError::Authority(
            ControlPlaneError::LeaseExpired { .. }
        ))
    ));
    assert!(matches!(
        host.execute(&directory, 10, MatchRuntime::advance_tick),
        Err(FencedRuntimeError::TimeWentBackwards)
    ));
}
