//! Owns network progress, bounded resume, input delivery and shutdown for a window.
use crate::{
    ClientError,
    network::{ClientSession, SessionError},
};
use mmorpg_core::{TICK_HZ, ZoneSnapshot};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, watch};

const RESEND_INTERVAL: Duration = Duration::from_millis(50);

/// Latest local intent published by the window. `jumps` counts Space presses:
/// the session sends one `Jump` when it advances, so coalesced watch updates
/// can merge presses but never replay them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MovementInput {
    pub forward: i8,
    pub strafe: i8,
    pub facing: u16,
    pub jumps: u32,
}

#[derive(Clone)]
pub enum NetworkUpdate {
    Waiting,
    Reconnecting,
    // Epoch accompanies every snapshot: watch coalescing may skip intermediate
    // status updates, but must never skip the presentation reset on resume.
    Snapshot {
        connection_epoch: u32,
        snapshot: Arc<ZoneSnapshot>,
    },
    Failed(String),
}

/// Tracks what was last sent: intent and jump changes go out immediately,
/// facing-only changes (camera drags) at most once per server tick, and the
/// heartbeat resends the current intent.
struct Outbox {
    sent: Option<(i8, i8, u16)>,
    sent_at: Instant,
    jumps: u32,
}

impl Outbox {
    fn new(input: MovementInput) -> Self {
        Self {
            sent: None,
            sent_at: Instant::now(),
            jumps: input.jumps,
        }
    }

    fn send_move(
        &mut self,
        session: &mut ClientSession,
        input: MovementInput,
    ) -> Result<(), SessionError> {
        session.send_move(input.forward, input.strafe, input.facing)?;
        self.sent = Some((input.forward, input.strafe, input.facing));
        self.sent_at = Instant::now();
        Ok(())
    }

    fn send_changes(
        &mut self,
        session: &mut ClientSession,
        input: MovementInput,
    ) -> Result<(), SessionError> {
        if input.jumps != self.jumps {
            self.jumps = input.jumps;
            session.send_jump()?;
        }
        let intent_changed = self
            .sent
            .is_none_or(|(forward, strafe, _)| (forward, strafe) != (input.forward, input.strafe));
        let facing_changed = self
            .sent
            .is_none_or(|(_, _, facing)| facing != input.facing);
        let tick = Duration::from_secs(1) / u32::from(TICK_HZ);
        if intent_changed || (facing_changed && self.sent_at.elapsed() >= tick) {
            self.send_move(session, input)?;
        }
        Ok(())
    }

    /// After resume only current input counts; presses during the outage are dropped.
    fn resume(&mut self, input: MovementInput) {
        self.jumps = input.jumps;
        self.sent = None;
    }
}

pub async fn run_session(
    mut session: ClientSession,
    mut input: watch::Receiver<MovementInput>,
    updates: &watch::Sender<NetworkUpdate>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), ClientError> {
    let mut heartbeat = tokio::time::interval(RESEND_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut outbox = Outbox::new(*input.borrow_and_update());
    let mut last_snapshot = Instant::now();
    let mut last_tick = None;
    loop {
        let failure = tokio::select! {
            biased;
            _ = &mut shutdown => return Ok(()),
            _ = heartbeat.tick() => {
                if last_snapshot.elapsed() > Duration::from_secs(5) {
                    Some(SessionError::SnapshotTimeout)
                } else {
                    // Periodic resend corrects a lost datagram, including a key
                    // release. Peek without marking the value seen, so a pending
                    // jump still reaches the change branch.
                    let current = *input.borrow();
                    outbox.send_move(&mut session, current).err()
                }
            },
            changed = input.changed() => match changed {
                Ok(()) => {
                    let current = *input.borrow_and_update();
                    outbox.send_changes(&mut session, current).err()
                }
                // The owning window has closed; it also requests shutdown.
                Err(_) => return Ok(()),
            },
            snapshot = session.receive_snapshot() => match snapshot {
                Ok(snapshot) => {
                    if last_tick.is_none_or(|tick| snapshot.tick > tick) {
                        last_tick = Some(snapshot.tick);
                        last_snapshot = Instant::now();
                        publish(updates, &session, snapshot);
                    }
                    None
                }
                Err(error) => Some(error),
            },
        };
        if let Some(error) = failure {
            let recoverable = tokio::select! {
                biased;
                _ = &mut shutdown => return Ok(()),
                recoverable = session.can_reconnect(&error) => recoverable,
            };
            if !recoverable {
                return Err(error.into());
            }
            updates.send_replace(NetworkUpdate::Reconnecting);
            let snapshot = tokio::select! {
                biased;
                _ = &mut shutdown => return Ok(()),
                snapshot = session.reconnect() => snapshot?,
            };
            last_tick = Some(snapshot.tick);
            last_snapshot = Instant::now();
            publish(updates, &session, snapshot);
            // Read only the current input after recovery; no buffered commands.
            outbox.resume(*input.borrow_and_update());
            heartbeat.reset();
        }
    }
}

fn publish(
    updates: &watch::Sender<NetworkUpdate>,
    session: &ClientSession,
    snapshot: ZoneSnapshot,
) {
    updates.send_replace(NetworkUpdate::Snapshot {
        connection_epoch: session.connection_epoch(),
        snapshot: Arc::new(snapshot),
    });
}
