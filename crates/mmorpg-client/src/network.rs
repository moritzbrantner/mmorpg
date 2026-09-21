//! MMO client adapter over the pinned session protocol and WebTransport.
//! Reconnect tokens stay private to this in-memory session; server authority owns
//! their rotation, grace period, and connection epochs.

use crate::ClientError;
use game_server::{BrowserRoutePrefix, MatchId, ReconnectToken, WELCOME_BYTES, Welcome};
use mmorpg_core::{TICK_HZ, ZoneCommand, ZoneId, ZoneSnapshot};
use std::{collections::BTreeSet, error::Error, fmt, path::Path, time::Duration};
use url::Url;
use wtransport::{
    ClientConfig, Connection, Endpoint, VarInt,
    endpoint::endpoint_side::Client,
    error::{ConnectionError, SendDatagramError},
    tls::Certificate,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_SETTLE: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub enum SessionError {
    Receive(ConnectionError),
    Send(SendDatagramError),
    SnapshotTimeout,
    InvalidData(ClientError),
    InvalidMovement,
    SequenceExhausted,
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Receive(error) => error.fmt(formatter),
            Self::Send(error) => error.fmt(formatter),
            Self::SnapshotTimeout => formatter.write_str("server snapshot timed out"),
            Self::InvalidData(error) => write!(formatter, "invalid session data: {error}"),
            Self::InvalidMovement => formatter.write_str("movement axes must be in [-1, 1]"),
            Self::SequenceExhausted => formatter.write_str("command sequence exhausted"),
        }
    }
}

impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Receive(error) => Some(error),
            Self::Send(error) => Some(error),
            Self::InvalidData(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

// Parse and build routes through the same contract as the server. Never accept
// a credential-bearing reconnect URL from CLI configuration.
struct SessionRoute {
    url: Url,
    prefix: BrowserRoutePrefix,
    match_id: MatchId,
}

impl SessionRoute {
    fn parse(value: &str) -> Result<Self, ClientError> {
        let url = Url::parse(value).map_err(|_| "invalid session URL")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("session URL must be HTTPS without credentials, query, or fragment".into());
        }
        let (prefix, id) = url
            .path()
            .rsplit_once("/matches/")
            .ok_or("session URL must name a hosted match")?;
        let prefix = BrowserRoutePrefix::new(prefix)?;
        let match_id = MatchId::new(id)?;
        Ok(Self {
            url,
            prefix,
            match_id,
        })
    }

    fn reconnect_url(&self, token: [u8; 16]) -> Url {
        let mut url = self.url.clone();
        url.set_path(
            &self
                .prefix
                .reconnect_path(&self.match_id, ReconnectToken(token)),
        );
        url
    }
}

pub struct ClientSession {
    // The endpoint and its verified TLS configuration outlive every connection.
    endpoint: Endpoint<Client>,
    connection: Connection,
    route: SessionRoute,
    welcome: Welcome,
    sequence: u32,
    resume_available: bool,
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
        let route = SessionRoute::parse(url)?;
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
            let connection = endpoint.connect(route.url.as_str()).await?;
            let welcome = read_welcome(&connection).await?;
            Ok(Self {
                endpoint,
                connection,
                route,
                welcome,
                sequence: 0,
                resume_available: true,
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

    #[must_use]
    pub const fn connection_epoch(&self) -> u32 {
        self.welcome.connection_epoch
    }

    /// Interrupt transport while retaining the in-memory resume identity.
    pub fn disconnect(&self) {
        self.connection
            .close(VarInt::from_u32(0), b"client disconnected");
    }

    /// One bounded resume attempt, never a fallback to fresh admission. A failure
    /// ends the session: the server may already have rotated the one-time token.
    /// Cancellation is supported by dropping this future and its owning session.
    pub async fn reconnect(&mut self) -> Result<ZoneSnapshot, ClientError> {
        if !self.resume_available {
            return Err("session resume is unavailable after an incomplete attempt".into());
        }
        self.resume_available = false;
        let grace =
            Duration::from_secs_f64(self.welcome.reconnect_grace_ticks as f64 / f64::from(TICK_HZ));
        let bound = CONNECT_TIMEOUT.min(grace);
        self.disconnect();
        let result = tokio::time::timeout(bound, async {
            // Let the server observe connection closure before it handles resume.
            // There is no protocol-level disconnect acknowledgement in this pin.
            tokio::time::sleep(RECONNECT_SETTLE).await;
            let url = self.route.reconnect_url(self.welcome.reconnect_token);
            // Do not propagate an error containing the credential-bearing URL.
            self.connection = self
                .endpoint
                .connect(url.as_str())
                .await
                .map_err(|_| "could not reconnect to the existing session")?;
            let welcome = read_welcome(&self.connection).await?;
            if welcome.player_id != self.welcome.player_id
                || welcome.connection_epoch <= self.welcome.connection_epoch
                || welcome.reconnect_token == self.welcome.reconnect_token
            {
                return Err("server did not resume the expected player and epoch".into());
            }
            self.welcome = welcome;
            // Keep the highest *sent* sequence, even if its acknowledgement was
            // lost. Never replay old inputs. Start the resumed connection stopped.
            self.send_movement(0, 0)?;
            let stopped_sequence = self.sequence;
            let mut resend = tokio::time::interval(Duration::from_millis(50));
            resend.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    snapshot = self.receive_snapshot() => {
                        let snapshot = snapshot?;
                        if snapshot.acknowledged_sequence >= stopped_sequence {
                            return Ok(snapshot);
                        }
                    }
                    _ = resend.tick() => self.send_movement(0, 0)?,
                }
            }
        })
        .await
        .map_err(|_| ClientError::from("session reconnect timed out"))
        .and_then(|result| result);
        if result.is_err() {
            self.disconnect();
        } else {
            self.resume_available = true;
        }
        result
    }

    /// Only interruption/timeouts are candidates for resume. Server application
    /// rejection, malformed snapshots and protocol errors terminate the session.
    pub async fn can_reconnect(&self, error: &SessionError) -> bool {
        if !self.resume_available {
            return false;
        }
        match error {
            SessionError::SnapshotTimeout => true,
            SessionError::Receive(error) => transient_connection_error(error),
            SessionError::Send(SendDatagramError::NotConnected) => {
                tokio::time::timeout(Duration::from_secs(1), self.connection.closed())
                    .await
                    .is_ok_and(|error| transient_connection_error(&error))
            }
            _ => false,
        }
    }

    pub fn send_movement(&mut self, x: i8, z: i8) -> Result<(), SessionError> {
        if !(-1..=1).contains(&x) || !(-1..=1).contains(&z) {
            return Err(SessionError::InvalidMovement);
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(SessionError::SequenceExhausted)?;
        let payload = mmorpg_protocol::encode_command(ZoneCommand::SetMovement { x, z });
        let frame = game_server::encode_command(sequence, &payload)
            .map_err(|error| SessionError::InvalidData(error.into()))?;
        self.connection
            .send_datagram(frame)
            .map_err(SessionError::Send)?;
        self.sequence = sequence;
        Ok(())
    }

    pub async fn receive_snapshot(&self) -> Result<ZoneSnapshot, SessionError> {
        let datagram = tokio::time::timeout(SNAPSHOT_TIMEOUT, self.connection.receive_datagram())
            .await
            .map_err(|_| SessionError::SnapshotTimeout)?
            .map_err(SessionError::Receive)?;
        self.decode_snapshot(&datagram)
            .map_err(SessionError::InvalidData)
    }

    fn decode_snapshot(&self, bytes: &[u8]) -> Result<ZoneSnapshot, ClientError> {
        let frame = game_server::decode_snapshot(bytes)?;
        let snapshot = mmorpg_protocol::decode_snapshot(&frame.payload)?;
        if snapshot.zone_id != self.zone_id || snapshot.content_revision != self.content_revision {
            return Err("server zone or world content does not match this client".into());
        }
        if frame.tick != snapshot.tick {
            return Err("session and zone snapshot ticks disagree".into());
        }
        let mut ids = BTreeSet::new();
        if snapshot
            .players
            .iter()
            .any(|player| !ids.insert(player.player_id))
        {
            return Err("duplicate player in snapshot".into());
        }
        if !ids.contains(&self.player_id()) {
            return Err("player projection does not contain the local player".into());
        }
        Ok(snapshot)
    }
}

async fn read_welcome(connection: &Connection) -> Result<Welcome, ClientError> {
    let mut stream = connection.accept_uni().await?;
    let mut bytes = [0; WELCOME_BYTES];
    stream.read_exact(&mut bytes).await?;
    let mut trailing = [0];
    if stream.read(&mut trailing).await?.is_some() {
        return Err("welcome contains trailing bytes".into());
    }
    let welcome = game_server::decode_welcome(&bytes)?;
    if welcome.tick_hz != TICK_HZ || welcome.player_id == 0 || welcome.connection_epoch == 0 {
        return Err("server welcome is incompatible with this client".into());
    }
    Ok(welcome)
}

fn transient_connection_error(error: &ConnectionError) -> bool {
    matches!(
        error,
        ConnectionError::TimedOut | ConnectionError::LocallyClosed
    )
}

impl Drop for ClientSession {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_route_preserves_origin_and_uses_shared_route_contract() {
        let route =
            SessionRoute::parse("https://localhost:4433/custom/game/matches/zone-2").unwrap();
        let token = [0xab; 16];
        let resumed = route.reconnect_url(token);
        assert_eq!(resumed.origin(), route.url.origin());
        let parsed = route.prefix.parse(resumed.path()).unwrap().unwrap();
        assert_eq!(parsed.match_id, route.match_id);
        assert_eq!(
            parsed.admission,
            game_server::BrowserAdmission::Reconnect(ReconnectToken(token))
        );
        for invalid in [
            "http://localhost/game/matches/zone-1",
            "https://user:secret@localhost/game/matches/zone-1",
            "https://localhost/game/matches/zone-1?token=secret",
            "https://localhost/game/matches/zone-1#fragment",
            "https://localhost/game/matches/zone-1/reconnect/secret",
            "https://localhost/game/matches/zone-1/",
        ] {
            assert!(SessionRoute::parse(invalid).is_err());
        }
    }
}
