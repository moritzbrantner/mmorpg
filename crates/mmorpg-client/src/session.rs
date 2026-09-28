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
/// Minimum spacing of facing-only moves: one server tick.
const FACING_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / TICK_HZ as u64);

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

/// Commands to send for one input observation, in order: a jump, then a move.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Outgoing {
    jump: bool,
    movement: Option<(i8, i8, u16)>,
}

impl Outgoing {
    fn deliver(self, session: &mut ClientSession) -> Result<(), SessionError> {
        if self.jump {
            session.send_jump()?;
        }
        if let Some((forward, strafe, facing)) = self.movement {
            session.send_move(forward, strafe, facing)?;
        }
        Ok(())
    }
}

/// Decides what to send and tracks what was last sent: intent and jump changes
/// go out immediately, facing-only changes (camera drags) at most once per
/// server tick, and the heartbeat resends the current intent. It performs no
/// I/O; a failed delivery ends in [`Outbox::resume`] or ends the session.
struct Outbox {
    sent: Option<(i8, i8, u16)>,
    sent_at: Instant,
    jumps: u32,
}

impl Outbox {
    fn new(input: MovementInput, now: Instant) -> Self {
        Self {
            sent: None,
            sent_at: now,
            jumps: input.jumps,
        }
    }

    fn movement(&mut self, input: MovementInput, now: Instant) -> (i8, i8, u16) {
        let movement = (input.forward, input.strafe, input.facing);
        self.sent = Some(movement);
        self.sent_at = now;
        movement
    }

    /// Commands for a newly observed input: one `Jump` when `jumps` advanced,
    /// however many presses it merged, then a `Move` when needed.
    fn changes(&mut self, input: MovementInput, now: Instant) -> Outgoing {
        let jump = input.jumps != self.jumps;
        self.jumps = input.jumps;
        let intent_changed = self
            .sent
            .is_none_or(|(forward, strafe, _)| (forward, strafe) != (input.forward, input.strafe));
        let facing_changed = self
            .sent
            .is_none_or(|(_, _, facing)| facing != input.facing);
        let facing_due =
            facing_changed && now.saturating_duration_since(self.sent_at) >= FACING_INTERVAL;
        let movement = (intent_changed || facing_due).then(|| self.movement(input, now));
        Outgoing { jump, movement }
    }

    /// Periodic resend of the current intent; a pending jump is left to
    /// [`Outbox::changes`], so it is never sent twice.
    fn heartbeat(&mut self, input: MovementInput, now: Instant) -> Outgoing {
        Outgoing {
            jump: false,
            movement: Some(self.movement(input, now)),
        }
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
    let mut outbox = Outbox::new(*input.borrow_and_update(), Instant::now());
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
                    outbox.heartbeat(current, Instant::now()).deliver(&mut session).err()
                }
            },
            changed = input.changed() => match changed {
                Ok(()) => {
                    let current = *input.borrow_and_update();
                    outbox.changes(current, Instant::now()).deliver(&mut session).err()
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
