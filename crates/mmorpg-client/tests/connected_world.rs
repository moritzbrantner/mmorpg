use game_server::{
    BrowserRoutePrefix, MatchHostWebTransportConfig, serve_match_host_with_shutdown,
};
use mmorpg_client::{ClientError, network::ClientSession};
use mmorpg_core::{ZoneId, outpost_definition};
use std::{net::UdpSocket, time::Duration};

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
        let start = initial
            .players
            .iter()
            .find(|player| player.player_id == first.player_id())
            .unwrap()
            .position;
        first.send_movement(0, -1)?;
        let mut moved = false;
        for _ in 0..30 {
            let snapshot = second.receive_snapshot().await?;
            if snapshot.players.iter().any(|player| {
                player.player_id == first.player_id() && player.position[2] < start[2]
            }) {
                moved = true;
                break;
            }
        }
        assert!(moved, "other clients must see the server-resolved movement");
        first.send_movement(0, 0)?;
        let mut stopped = false;
        for _ in 0..30 {
            let snapshot = first.receive_snapshot().await?;
            if snapshot.acknowledged_sequence >= 2 {
                let player = snapshot
                    .players
                    .iter()
                    .find(|player| player.player_id == first.player_id())
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
            first.send_movement(0, 0)?;
        }
        let player_id = first.player_id();
        let previous_epoch = first.connection_epoch();
        let resumed = first.reconnect().await?;
        assert_eq!(first.player_id(), player_id);
        assert_eq!(first.connection_epoch(), previous_epoch + 1);
        assert!(resumed.acknowledged_sequence >= 35);
        let resumed_player = resumed
            .players
            .iter()
            .find(|p| p.player_id == player_id)
            .unwrap();
        assert!(
            resumed_player.position[2] < start[2],
            "resume must retain movement state"
        );
        assert_eq!(resumed_player.velocity, [0; 3]);
        assert_eq!(
            resumed.players.len(),
            2,
            "resume must not admit another player"
        );
        // A second successful resume needs the newly rotated token.
        let again = first.reconnect().await?;
        assert_eq!(first.connection_epoch(), previous_epoch + 2);
        assert!(again.acknowledged_sequence > resumed.acknowledged_sequence);
        first.send_movement(0, 1)?;
        let mut resumed_movement = false;
        for _ in 0..30 {
            let snapshot = second.receive_snapshot().await?;
            if snapshot
                .players
                .iter()
                .any(|p| p.player_id == player_id && p.velocity[2] > 0)
            {
                resumed_movement = true;
                break;
            }
        }
        assert!(
            resumed_movement,
            "resumed commands must reach the same authoritative player"
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
    use mmorpg_client::session::{NetworkUpdate, run_session};
    use tokio::sync::{oneshot, watch};
    let player_id = session.player_id();
    let epoch = session.connection_epoch();
    session.disconnect();
    let (_input, input) = watch::channel([0, 0]);
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
                assert!(snapshot.players.iter().any(|p| p.player_id == player_id));
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
    use mmorpg_client::session::{NetworkUpdate, run_session};
    use tokio::sync::{oneshot, watch};
    session.disconnect();
    let (_input, input) = watch::channel([0, 0]);
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
