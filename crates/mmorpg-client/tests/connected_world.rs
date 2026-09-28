use game_server::{
    BrowserRoutePrefix, MatchHostWebTransportConfig, serve_match_host_with_shutdown,
};
use mmorpg_client::{
    ClientError,
    network::ClientSession,
    session::{MovementInput, NetworkUpdate},
};
use mmorpg_core::{
    EntitySnapshot, PLAYER_HALF_EXTENTS_UNITS, TICK_HZ, ZoneId, ZoneSnapshot, outpost_definition,
};
use std::{net::UdpSocket, sync::Arc, time::Duration};
use tokio::sync::watch;

/// Forward-movement headings: yaw 0 faces +Z, a quarter turn faces +X and
/// half a turn faces -Z.
const NORTH: u16 = 0;
const EAST: u16 = 16_384;
const SOUTH: u16 = 32_768;

#[tokio::test]
async fn clients_share_authority_resume_identity_and_reject_wrong_content() {
    let temporary = tempfile::tempdir().unwrap();
    let certificate = temporary.path().join("cert.pem");
    let key = temporary.path().join("key.pem");
    let identity = wtransport::Identity::self_signed(["localhost", "127.0.0.1"]).unwrap();
    std::fs::write(
        &certificate,
        identity.certificate_chain().as_slice()[0].to_pem(),
    )
    .unwrap();
    let mut key_options = std::fs::OpenOptions::new();
    key_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        key_options.mode(0o600);
    }
    use std::io::Write;
    key_options
        .open(&key)
        .unwrap()
        .write_all(identity.private_key().to_secret_pem().as_bytes())
        .unwrap();
    // The shared host accepts a port rather than a pre-bound endpoint. Select an
    // OS-assigned disposable port, releasing it immediately before host startup.
    let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let zone_id = ZoneId::new(1);
    let host = mmorpg_game_server::build_zone_host([zone_id], 120).unwrap();
    let (shutdown, receiver) = tokio::sync::mpsc::channel(1);
    drop(reservation);
    let task = tokio::spawn(serve_match_host_with_shutdown(
        host,
        MatchHostWebTransportConfig {
            port,
            certificate_pem: certificate.clone(),
            private_key_pem: key,
            route_prefix: BrowserRoutePrefix::new("/game").unwrap(),
            drain_grace: Duration::from_millis(10),
        },
        receiver,
    ));
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let url = format!("https://localhost:{port}/game/matches/zone-1");
        let revision = outpost_definition().revision();
        let mut first = ClientSession::connect(&url, Some(&certificate), zone_id, revision).await?;
        let second = ClientSession::connect(&url, Some(&certificate), zone_id, revision).await?;
        assert_ne!(first.player_id(), second.player_id());
        let initial = first.receive_snapshot().await?;
        assert_eq!(initial.viewer_id, first.player_id());
        let start = initial
            .entities
            .iter()
            .find(|player| player.id == first.player_id())
            .unwrap()
            .position;
        first.send_move(1, 0, SOUTH)?;
        let mut moved = false;
        for _ in 0..30 {
            let snapshot = second.receive_snapshot().await?;
            assert_eq!(snapshot.viewer_id, second.player_id());
            if snapshot.entities.iter().any(|player| {
                player.id == first.player_id()
                    && player.position[2] < start[2]
                    && player.facing == SOUTH
            }) {
                moved = true;
                break;
            }
        }
        assert!(
            moved,
            "other clients must see the server-resolved movement and facing"
        );
        first.send_move(0, 0, SOUTH)?;
        let mut stopped = false;
        for _ in 0..30 {
            let snapshot = first.receive_snapshot().await?;
            if snapshot.acknowledged_sequence >= 2 {
                let player = snapshot
                    .entities
                    .iter()
                    .find(|player| player.id == first.player_id())
                    .unwrap();
                if player.velocity[2] == 0 {
                    stopped = true;
                    break;
                }
            }
        }
        assert!(
            stopped,
            "key release must become authoritative and acknowledged"
        );
        // Commands may have reached the server even if their ACK was lost.
        // Preserve the highest sent sequence across connection replacement.
        for _ in 0..32 {
            first.send_move(0, 0, SOUTH)?;
        }
        let player_id = first.player_id();
        let previous_epoch = first.connection_epoch();
        let resumed = first.reconnect().await?;
        assert_eq!(first.player_id(), player_id);
        assert_eq!(first.connection_epoch(), previous_epoch + 1);
        assert!(resumed.acknowledged_sequence >= 35);
        let resumed_player = resumed.entities.iter().find(|p| p.id == player_id).unwrap();
        assert!(
            resumed_player.position[2] < start[2],
            "resume must retain movement state"
        );
        assert_eq!(resumed_player.velocity, [0; 3]);
        assert_eq!(
            resumed_player.facing, SOUTH,
            "the stopped intent keeps facing"
        );
        assert_eq!(
            resumed.entities.len(),
            2,
            "resume must not admit another player"
        );
        // A second successful resume needs the newly rotated token.
        let again = first.reconnect().await?;
        assert_eq!(first.connection_epoch(), previous_epoch + 2);
        assert!(again.acknowledged_sequence > resumed.acknowledged_sequence);
        first.send_move(1, 0, NORTH)?;
        let mut resumed_movement = false;
        for _ in 0..30 {
            let snapshot = second.receive_snapshot().await?;
            if snapshot
                .entities
                .iter()
                .any(|p| p.id == player_id && p.velocity[2] > 0)
            {
                resumed_movement = true;
                break;
            }
        }
        assert!(
            resumed_movement,
            "resumed commands must reach the same authoritative player"
        );
        first.send_jump()?;
        let mut jumped = false;
        for _ in 0..30 {
            let snapshot = second.receive_snapshot().await?;
            if snapshot.entities.iter().any(|p| {
                p.id == player_id
                    && p.position[1] > PLAYER_HALF_EXTENTS_UNITS[1]
                    && p.velocity[2] > 0
            }) {
                jumped = true;
                break;
            }
        }
        assert!(
            jumped,
            "a grounded running jump must be visible to other clients"
        );
        let mut incompatible =
            ClientSession::connect(&url, Some(&certificate), zone_id, revision + 1).await?;
        let incompatible_error = incompatible.receive_snapshot().await.unwrap_err();
        assert!(!incompatible.can_reconnect(&incompatible_error).await);
        assert!(incompatible.reconnect().await.is_err());
        assert_eq!(
            incompatible.reconnect().await.unwrap_err().to_string(),
            "session resume is unavailable after an incomplete attempt"
        );
        let unrelated_identity = wtransport::Identity::self_signed(["localhost"]).unwrap();
        let wrong_certificate = temporary.path().join("wrong-cert.pem");
        std::fs::write(
            &wrong_certificate,
            unrelated_identity.certificate_chain().as_slice()[0].to_pem(),
        )
        .unwrap();
        assert!(
            ClientSession::connect(&url, Some(&wrong_certificate), zone_id, revision)
                .await
                .is_err()
        );
        let third = ClientSession::connect(&url, Some(&certificate), zone_id, revision).await?;
        verify_live_input(third).await?;
        verify_automatic_resume(first).await?;
        verify_shutdown_during_resume(second).await?;
        Ok::<(), ClientError>(())
    })
    .await;
    shutdown.send(()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    result.unwrap().unwrap();
}

async fn verify_automatic_resume(session: ClientSession) -> Result<(), ClientError> {
    use mmorpg_client::session::{MovementInput, NetworkUpdate, run_session};
    use tokio::sync::{oneshot, watch};
    let player_id = session.player_id();
    let epoch = session.connection_epoch();
    session.disconnect();
    let (_input, input) = watch::channel(MovementInput::default());
    let (updates, mut receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown, stopped) = oneshot::channel();
    // JoinSet aborts its owned task on every early-return/panic path.
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move { run_session(session, input, &updates, stopped).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            receiver.changed().await?;
            if let NetworkUpdate::Snapshot {
                connection_epoch,
                snapshot,
            } = &*receiver.borrow_and_update()
            {
                assert_eq!(*connection_epoch, epoch + 1);
                assert!(snapshot.entities.iter().any(|p| p.id == player_id));
                break;
            }
        }
        Ok::<(), ClientError>(())
    })
    .await??;
    shutdown
        .send(())
        .map_err(|_| "worker exited before shutdown")?;
    tokio::time::timeout(Duration::from_secs(1), tasks.join_next())
        .await?
        .ok_or("missing worker")???;
    Ok(())
}

async fn verify_shutdown_during_resume(session: ClientSession) -> Result<(), ClientError> {
    use mmorpg_client::session::{MovementInput, NetworkUpdate, run_session};
    use tokio::sync::{oneshot, watch};
    session.disconnect();
    let (_input, input) = watch::channel(MovementInput::default());
    let (updates, mut receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown, stopped) = oneshot::channel();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move { run_session(session, input, &updates, stopped).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            receiver.changed().await?;
            if matches!(*receiver.borrow_and_update(), NetworkUpdate::Reconnecting) {
                break;
            }
        }
        Ok::<(), ClientError>(())
    })
    .await??;
    shutdown
        .send(())
        .map_err(|_| "worker exited before shutdown")?;
    tokio::time::timeout(Duration::from_secs(1), tasks.join_next())
        .await?
        .ok_or("missing worker")???;
    Ok(())
}

/// Drives `run_session` through its input channel, as the window does: a Jump
/// pressed while the connection is down is dropped on resume, each later press
/// sends one Jump, and a facing-only change while running turns the player.
async fn verify_live_input(session: ClientSession) -> Result<(), ClientError> {
    use mmorpg_client::session::run_session;
    use tokio::sync::oneshot;
    let player_id = session.player_id();
    let epoch = session.connection_epoch();
    // A freshly admitted player stands still on flat ground.
    let spawned = session.receive_snapshot().await?;
    assert!(is_at_rest(own(&spawned, player_id)));
    session.disconnect();
    let (input, watched) = watch::channel(MovementInput::default());
    let (updates, mut receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown, stopped) = oneshot::channel();
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move { run_session(session, watched, &updates, stopped).await });
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            receiver.changed().await?;
            if matches!(*receiver.borrow_and_update(), NetworkUpdate::Reconnecting) {
                break;
            }
        }
        // Resume publishes its snapshot before it rereads input, so the press
        // lands in the outage while `Reconnecting` is still the latest update.
        input.send_modify(|input| input.jumps += 1);
        assert!(
            matches!(*receiver.borrow(), NetworkUpdate::Reconnecting),
            "the press must happen before the session resumes"
        );
        let resumed = next_snapshot(&mut receiver).await?;
        assert_eq!(resumed.0, epoch + 1);
        // Any input change reaches the outbox; a replayed press would ride along.
        input.send_modify(|input| input.facing = EAST);
        let turned = wait_for(&mut receiver, player_id, |player| player.facing == EAST).await?;
        assert!(
            is_at_rest(own(&turned, player_id)),
            "the outage press must not be sent with the first change after resume"
        );
        assert_stays_at_rest(&mut receiver, player_id, turned.tick).await?;

        input.send_modify(|input| input.jumps += 1);
        wait_for(&mut receiver, player_id, |player| {
            player.position[1] > PLAYER_HALF_EXTENTS_UNITS[1]
        })
        .await?;
        let landed = wait_for(&mut receiver, player_id, is_at_rest).await?;
        // A repeated Jump would lift the player again as soon as it lands.
        assert_stays_at_rest(&mut receiver, player_id, landed.tick).await?;

        input.send_modify(|input| {
            input.forward = 1;
            input.facing = NORTH;
        });
        wait_for(&mut receiver, player_id, |player| {
            player.facing == NORTH && player.velocity[2] > 0
        })
        .await?;
        input.send_modify(|input| input.facing = EAST);
        wait_for(&mut receiver, player_id, |player| {
            player.facing == EAST && player.velocity[0] > 0 && player.velocity[2] == 0
        })
        .await?;
        Ok::<(), ClientError>(())
    })
    .await??;
    shutdown
        .send(())
        .map_err(|_| "worker exited before shutdown")?;
    tokio::time::timeout(Duration::from_secs(1), tasks.join_next())
        .await?
        .ok_or("missing worker")???;
    Ok(())
}

fn own(snapshot: &ZoneSnapshot, player_id: u32) -> &EntitySnapshot {
    snapshot
        .entities
        .iter()
        .find(|player| player.id == player_id)
        .expect("a player always sees itself")
}

/// Feet on the ground with no vertical motion.
fn is_at_rest(player: &EntitySnapshot) -> bool {
    player.position[1] == PLAYER_HALF_EXTENTS_UNITS[1] && player.velocity[1] == 0
}

async fn next_snapshot(
    receiver: &mut watch::Receiver<NetworkUpdate>,
) -> Result<(u32, Arc<ZoneSnapshot>), ClientError> {
    loop {
        receiver.changed().await?;
        match &*receiver.borrow_and_update() {
            NetworkUpdate::Snapshot {
                connection_epoch,
                snapshot,
            } => return Ok((*connection_epoch, Arc::clone(snapshot))),
            NetworkUpdate::Failed(error) => return Err(error.clone().into()),
            NetworkUpdate::Waiting | NetworkUpdate::Reconnecting => {}
        }
    }
}

/// The first published snapshot, within two seconds of server ticks, in which
/// the player satisfies `condition`. A full jump lands within about 33 ticks.
async fn wait_for(
    receiver: &mut watch::Receiver<NetworkUpdate>,
    player_id: u32,
    condition: impl Fn(&EntitySnapshot) -> bool,
) -> Result<Arc<ZoneSnapshot>, ClientError> {
    let mut snapshot = next_snapshot(receiver).await?.1;
    let deadline = snapshot.tick + 2 * u64::from(TICK_HZ);
    while !condition(own(&snapshot, player_id)) {
        if snapshot.tick > deadline {
            return Err(format!("player never reached the expected state: {snapshot:?}").into());
        }
        snapshot = next_snapshot(receiver).await?.1;
    }
    Ok(snapshot)
}

/// Every snapshot for half a second of server ticks after `since` shows the
/// player standing on the ground.
async fn assert_stays_at_rest(
    receiver: &mut watch::Receiver<NetworkUpdate>,
    player_id: u32,
    since: u64,
) -> Result<(), ClientError> {
    loop {
        let (_, snapshot) = next_snapshot(receiver).await?;
        let player = own(&snapshot, player_id);
        assert!(
            is_at_rest(player),
            "no Jump may be sent here, but the player left the ground: {player:?}"
        );
        if snapshot.tick >= since + u64::from(TICK_HZ / 2) {
            return Ok(());
        }
    }
}
