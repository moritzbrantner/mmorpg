//! MMO client adapter over the shared session protocol and WebTransport.
//! No client simulation or alternate server-side session implementation lives here.

use crate::ClientError;
use game_server::{WELCOME_BYTES, Welcome};
use mmorpg_core::{TICK_HZ, ZoneCommand, ZoneId, ZoneSnapshot};
use std::{path::Path, time::Duration};
use wtransport::{
    ClientConfig, Connection, Endpoint, VarInt, endpoint::endpoint_side::Client, tls::Certificate,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ClientSession {
    // The endpoint must outlive the connection.
    _endpoint: Endpoint<Client>,
    connection: Connection,
    welcome: Welcome,
    sequence: u32,
    zone_id: ZoneId,
    content_revision: u64,
}

impl ClientSession {
    pub async fn connect(
        url: &str,
        certificate: Option<&Path>,
        zone_id: ZoneId,
        content_revision: u64,
    ) -> Result<Self, ClientError> {
        tokio::time::timeout(CONNECT_TIMEOUT, async {
            let builder = ClientConfig::builder().with_bind_default();
            let config = match certificate {
                Some(path) => {
                    builder.with_server_certificate_hashes([Certificate::load_pemfile(path)
                        .await?
                        .hash()])
                }
                None => builder.with_native_certs(),
            }
            .keep_alive_interval(Some(Duration::from_secs(1)))
            .build();
            let endpoint = Endpoint::client(config)?;
            let connection = endpoint.connect(url).await?;
            let mut stream = connection.accept_uni().await?;
            let mut bytes = [0; WELCOME_BYTES];
            stream.read_exact(&mut bytes).await?;
            let mut trailing = [0];
            if stream.read(&mut trailing).await?.is_some() {
                return Err("welcome contains trailing bytes".into());
            }
            let welcome = game_server::decode_welcome(&bytes)?;
            if welcome.tick_hz != TICK_HZ {
                return Err("server tick rate is incompatible with this client".into());
            }
            Ok(Self {
                _endpoint: endpoint,
                connection,
                welcome,
                sequence: 0,
                zone_id,
                content_revision,
            })
        })
        .await
        .map_err(|_| "connection or welcome timed out")?
    }

    #[must_use]
    pub const fn player_id(&self) -> u32 {
        self.welcome.player_id
    }

    /// Resend current movement regularly: losing a key-release datagram must not
    /// leave the server indefinitely holding the last delivered movement intent.
    pub fn send_movement(&mut self, x: i8, z: i8) -> Result<(), ClientError> {
        if !(-1..=1).contains(&x) || !(-1..=1).contains(&z) {
            return Err("movement axes must be in [-1, 1]".into());
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("command sequence exhausted")?;
        let payload = mmorpg_protocol::encode_command(ZoneCommand::SetMovement { x, z });
        self.connection
            .send_datagram(game_server::encode_command(sequence, &payload)?)?;
        self.sequence = sequence;
        Ok(())
    }

    pub async fn receive_snapshot(&self) -> Result<ZoneSnapshot, ClientError> {
        let datagram = tokio::time::timeout(SNAPSHOT_TIMEOUT, self.connection.receive_datagram())
            .await
            .map_err(|_| "server snapshot timed out")??;
        let frame = game_server::decode_snapshot(&datagram)?;
        let snapshot = mmorpg_protocol::decode_snapshot(&frame.payload)?;
        if snapshot.zone_id != self.zone_id || snapshot.content_revision != self.content_revision {
            return Err("server zone or world content does not match this client".into());
        }
        if frame.tick != snapshot.tick {
            return Err("session and zone snapshot ticks disagree".into());
        }
        if !snapshot
            .players
            .iter()
            .any(|player| player.player_id == self.player_id())
        {
            return Err("player projection does not contain the local player".into());
        }
        Ok(snapshot)
    }
}

impl Drop for ClientSession {
    fn drop(&mut self) {
        self.connection.close(VarInt::from_u32(0), b"client closed");
    }
}
