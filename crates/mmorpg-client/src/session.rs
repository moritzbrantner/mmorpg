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

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = FACING_INTERVAL;
    const EAST: u16 = 16_384;

    fn held(forward: i8, strafe: i8, facing: u16, jumps: u32) -> MovementInput {
        MovementInput {
            forward,
            strafe,
            facing,
            jumps,
        }
    }

    fn only_move(forward: i8, strafe: i8, facing: u16) -> Outgoing {
        Outgoing {
            jump: false,
            movement: Some((forward, strafe, facing)),
        }
    }

    /// An outbox that has already sent `input` at `now`.
    fn sent(input: MovementInput, now: Instant) -> Outbox {
        let mut outbox = Outbox::new(input, now);
        assert_eq!(
            outbox.changes(input, now),
            only_move(input.forward, input.strafe, input.facing),
            "the first observation sends the current intent without a jump"
        );
        outbox
    }

    #[test]
    fn each_advance_of_the_press_counter_sends_exactly_one_jump() {
        let now = Instant::now();
        let mut outbox = sent(MovementInput::default(), now);
        let pressed = held(0, 0, 0, 1);
        assert_eq!(
            outbox.changes(pressed, now),
            Outgoing {
                jump: true,
                movement: None,
            }
        );
        assert_eq!(
            outbox.changes(pressed, now),
            Outgoing::default(),
            "observing the same counter again never repeats the jump"
        );
        let merged = held(0, 0, 0, 4);
        assert_eq!(
            outbox.changes(merged, now),
            Outgoing {
                jump: true,
                movement: None,
            },
            "coalesced presses merge into one jump"
        );
        let running = held(1, 0, 0, 5);
        assert_eq!(
            outbox.changes(running, now),
            Outgoing {
                jump: true,
                movement: Some((1, 0, 0)),
            },
            "a press with an intent change sends the jump, then the move"
        );
    }

    #[test]
    fn the_heartbeat_resends_intent_but_never_a_pending_jump() {
        let now = Instant::now();
        let mut outbox = sent(held(1, 0, 0, 0), now);
        // The heartbeat peeks at input the change branch has not handled yet.
        let pressed = held(1, 0, 0, 1);
        assert_eq!(outbox.heartbeat(pressed, now + TICK), only_move(1, 0, 0));
        assert_eq!(
            outbox.heartbeat(pressed, now + 2 * TICK),
            only_move(1, 0, 0)
        );
        assert_eq!(
            outbox.changes(pressed, now + 2 * TICK),
            Outgoing {
                jump: true,
                movement: None,
            },
            "the change branch still sends the pending jump exactly once"
        );
        assert_eq!(
            outbox.heartbeat(pressed, now + 3 * TICK),
            only_move(1, 0, 0)
        );
    }

    #[test]
    fn presses_made_during_an_outage_are_dropped_on_resume() {
        let now = Instant::now();
        let mut outbox = sent(held(1, 0, 0, 2), now);
        // The connection drops; the player presses Space three times and
        // turns before the session resumes.
        outbox.resume(held(1, 0, EAST, 5));
        assert_eq!(
            outbox.changes(held(1, 0, EAST, 5), now),
            only_move(1, 0, EAST),
            "resume resends only the current intent"
        );
        assert_eq!(
            outbox.changes(held(1, 0, EAST, 6), now),
            Outgoing {
                jump: true,
                movement: None,
            },
            "a press after resume is sent"
        );
    }

    #[test]
    fn intent_changes_are_sent_immediately() {
        let now = Instant::now();
        let mut outbox = sent(MovementInput::default(), now);
        assert_eq!(outbox.changes(held(1, 0, 0, 0), now), only_move(1, 0, 0));
        assert_eq!(outbox.changes(held(1, -1, 0, 0), now), only_move(1, -1, 0));
        assert_eq!(
            outbox.changes(held(0, -1, EAST, 0), now),
            only_move(0, -1, EAST),
            "an intent change carries the latest facing without waiting"
        );
        assert_eq!(
            outbox.changes(held(0, 0, EAST, 0), now),
            only_move(0, 0, EAST),
            "a key release is never throttled"
        );
    }

    #[test]
    fn facing_only_changes_are_sent_at_most_once_per_tick() {
        let start = Instant::now();
        let mut outbox = sent(held(1, 0, 0, 0), start);
        let half_tick = TICK / 2;
        assert_eq!(
            outbox.changes(held(1, 0, 100, 0), start + half_tick),
            Outgoing::default()
        );
        assert_eq!(
            outbox.changes(held(1, 0, 200, 0), start + TICK - Duration::from_nanos(1)),
            Outgoing::default()
        );
        assert_eq!(
            outbox.changes(held(1, 0, 300, 0), start + TICK),
            only_move(1, 0, 300),
            "a facing change is sent once a tick has passed since the last move"
        );
        assert_eq!(
            outbox.changes(held(1, 0, 400, 0), start + TICK + half_tick),
            Outgoing::default(),
            "the window restarts at the last sent move"
        );
        assert_eq!(
            outbox.changes(held(1, 0, 300, 0), start + 3 * TICK),
            Outgoing::default(),
            "facing back at the last sent value is not a change"
        );
        // A heartbeat move also restarts the window.
        assert_eq!(
            outbox.heartbeat(held(1, 0, 500, 0), start + 4 * TICK),
            only_move(1, 0, 500)
        );
        assert_eq!(
            outbox.changes(held(1, 0, 600, 0), start + 4 * TICK + half_tick),
            Outgoing::default()
        );
        assert_eq!(
            outbox.changes(held(1, 0, 600, 0), start + 5 * TICK),
            only_move(1, 0, 600)
        );
    }
}
