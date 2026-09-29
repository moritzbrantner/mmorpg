//! Owns network progress, bounded resume, input delivery and shutdown for a window.
use crate::{
    ClientError,
    network::{ClientSession, SessionError},
};
use mmorpg_core::{EntityRef, TICK_HZ, ZoneSnapshot};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, watch};

const RESEND_INTERVAL: Duration = Duration::from_millis(50);
/// Minimum spacing of facing-only moves: one server tick.
const FACING_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / TICK_HZ as u64);

/// Latest local input published by the window: held movement plus counted
/// discrete presses. Each counter (`jumps`, `selections`, `attack_requests`,
/// `releases`) makes the session send one command when it advances, so
/// coalesced watch updates can merge presses but never replay them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlayerInput {
    pub forward: i8,
    pub strafe: i8,
    pub facing: u16,
    pub jumps: u32,
    /// The latest requested selection (`None` clears it).
    pub target: Option<EntityRef>,
    pub selections: u32,
    /// Whether the latest attack request starts or stops auto-attack.
    pub attack: bool,
    pub attack_requests: u32,
    pub releases: u32,
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

/// Commands to send for one input observation, in order: a selection, an
/// attack start or stop, a spirit release, a jump, then a move.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Outgoing {
    select: Option<Option<EntityRef>>,
    attack: Option<bool>,
    release: bool,
    jump: bool,
    movement: Option<(i8, i8, u16)>,
}

impl Outgoing {
    fn deliver(self, session: &mut ClientSession) -> Result<(), SessionError> {
        if let Some(target) = self.select {
            session.send_select_target(target)?;
        }
        if let Some(start) = self.attack {
            session.send_attack(start)?;
        }
        if self.release {
            session.send_release_spirit()?;
        }
        if self.jump {
            session.send_jump()?;
        }
        if let Some((forward, strafe, facing)) = self.movement {
            session.send_move(forward, strafe, facing)?;
        }
        Ok(())
    }
}

/// Press counters already handled; a counter that moves on means one more
/// command, however many presses it merged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Presses {
    jumps: u32,
    selections: u32,
    attack_requests: u32,
    releases: u32,
}

impl Presses {
    const fn of(input: PlayerInput) -> Self {
        Self {
            jumps: input.jumps,
            selections: input.selections,
            attack_requests: input.attack_requests,
            releases: input.releases,
        }
    }
}

/// Decides what to send and tracks what was last sent: intent and jump changes
/// go out immediately, facing-only changes (camera drags) at most once per
/// server tick, and the heartbeat resends the current intent. It performs no
/// I/O; a failed delivery ends in [`Outbox::resume`] or ends the session.
struct Outbox {
    sent: Option<(i8, i8, u16)>,
    sent_at: Instant,
    presses: Presses,
}

impl Outbox {
    fn new(input: PlayerInput, now: Instant) -> Self {
        Self {
            sent: None,
            sent_at: now,
            presses: Presses::of(input),
        }
    }

    fn movement(&mut self, input: PlayerInput, now: Instant) -> (i8, i8, u16) {
        let movement = (input.forward, input.strafe, input.facing);
        self.sent = Some(movement);
        self.sent_at = now;
        movement
    }

    /// Commands for a newly observed input: one command per advanced press
    /// counter, however many presses it merged, then a `Move` when needed.
    fn changes(&mut self, input: PlayerInput, now: Instant) -> Outgoing {
        let handled = self.presses;
        let pressed = Presses::of(input);
        self.presses = pressed;
        let intent_changed = self
            .sent
            .is_none_or(|(forward, strafe, _)| (forward, strafe) != (input.forward, input.strafe));
        let facing_changed = self
            .sent
            .is_none_or(|(_, _, facing)| facing != input.facing);
        let facing_due =
            facing_changed && now.saturating_duration_since(self.sent_at) >= FACING_INTERVAL;
        let movement = (intent_changed || facing_due).then(|| self.movement(input, now));
        Outgoing {
            select: (pressed.selections != handled.selections).then_some(input.target),
            attack: (pressed.attack_requests != handled.attack_requests).then_some(input.attack),
            release: pressed.releases != handled.releases,
            jump: pressed.jumps != handled.jumps,
            movement,
        }
    }

    /// Periodic resend of the current intent; pending presses are left to
    /// [`Outbox::changes`], so none is ever sent twice.
    fn heartbeat(&mut self, input: PlayerInput, now: Instant) -> Outgoing {
        Outgoing {
            movement: Some(self.movement(input, now)),
            ..Outgoing::default()
        }
    }

    /// After resume only current input counts; presses during the outage are dropped.
    fn resume(&mut self, input: PlayerInput) {
        self.presses = Presses::of(input);
        self.sent = None;
    }
}

pub async fn run_session(
    mut session: ClientSession,
    mut input: watch::Receiver<PlayerInput>,
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

    fn held(forward: i8, strafe: i8, facing: u16, jumps: u32) -> PlayerInput {
        PlayerInput {
            forward,
            strafe,
            facing,
            jumps,
            ..PlayerInput::default()
        }
    }

    fn only_move(forward: i8, strafe: i8, facing: u16) -> Outgoing {
        Outgoing {
            movement: Some((forward, strafe, facing)),
            ..Outgoing::default()
        }
    }

    /// An outbox that has already sent `input` at `now`.
    fn sent(input: PlayerInput, now: Instant) -> Outbox {
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
        let mut outbox = sent(PlayerInput::default(), now);
        let pressed = held(0, 0, 0, 1);
        assert_eq!(
            outbox.changes(pressed, now),
            Outgoing {
                jump: true,
                ..Outgoing::default()
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
                ..Outgoing::default()
            },
            "coalesced presses merge into one jump"
        );
        let running = held(1, 0, 0, 5);
        assert_eq!(
            outbox.changes(running, now),
            Outgoing {
                jump: true,
                movement: Some((1, 0, 0)),
                ..Outgoing::default()
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
                ..Outgoing::default()
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
                ..Outgoing::default()
            },
            "a press after resume is sent"
        );
    }

    #[test]
    fn intent_changes_are_sent_immediately() {
        let now = Instant::now();
        let mut outbox = sent(PlayerInput::default(), now);
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

    #[test]
    fn selections_attacks_and_releases_are_sent_once_per_press_in_order() {
        use mmorpg_core::CreatureId;
        let now = Instant::now();
        let mut outbox = sent(PlayerInput::default(), now);
        let wolf = Some(EntityRef::Creature(CreatureId::new(108)));
        let input = PlayerInput {
            target: wolf,
            selections: 2,
            attack: true,
            attack_requests: 1,
            ..PlayerInput::default()
        };
        assert_eq!(
            outbox.changes(input, now),
            Outgoing {
                select: Some(wolf),
                attack: Some(true),
                ..Outgoing::default()
            },
            "merged selections send the latest target once, then the attack"
        );
        assert_eq!(outbox.changes(input, now), Outgoing::default());
        assert_eq!(
            outbox.heartbeat(input, now + TICK),
            only_move(0, 0, 0),
            "the heartbeat never repeats a press"
        );
        let released = PlayerInput {
            releases: 1,
            ..input
        };
        assert_eq!(
            outbox.changes(released, now),
            Outgoing {
                release: true,
                ..Outgoing::default()
            }
        );
        outbox.resume(PlayerInput {
            selections: 9,
            attack_requests: 9,
            releases: 9,
            ..released
        });
        assert_eq!(
            outbox.changes(
                PlayerInput {
                    selections: 9,
                    attack_requests: 9,
                    releases: 9,
                    ..released
                },
                now
            ),
            only_move(0, 0, 0),
            "presses during an outage are dropped on resume"
        );
    }
}
