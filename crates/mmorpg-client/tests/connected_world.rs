use game_server::{
    BrowserRoutePrefix, MatchHostWebTransportConfig, serve_match_host_with_shutdown,
};
use mmorpg_client::{ClientError, network::ClientSession};
use mmorpg_core::{ZoneId, outpost_definition};
use std::{net::UdpSocket, time::Duration};

#[tokio::test]
async fn two_real_clients_share_server_authority_and_reject_wrong_content() {
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
        let incompatible =
            ClientSession::connect(&url, Some(&certificate), zone_id, revision + 1).await?;
        assert!(incompatible.receive_snapshot().await.is_err());
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
