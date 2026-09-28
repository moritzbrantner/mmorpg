//! Scripted bots against the real zone host composition, stepped in-process.
//!
//! The runner builds the same `MatchHost` as `mmorpg-zone-host`, admits bots
//! through the `game-server` session path, submits `mmorpg-protocol` command
//! bytes, advances ticks explicitly and decodes each bot's player-scoped
//! snapshot bytes. It owns no gameplay, session or visibility rules; it only
//! records what those authorities decide and compares it with expectations.

use std::collections::BTreeMap;

use game_server::{
    CommandOutcome, MatchHost, MatchId, MatchRuntime, RECONNECT_TOKEN_BYTES, ReconnectToken,
    RuntimeError, SessionLease,
};
use mmorpg_core::{
    Area, EntityKind, EntitySnapshot, MAX_PLAYERS_PER_ZONE, PlayerId, ZoneCommand, ZoneId,
    ZoneSnapshot, greyhaven_vale,
};
use mmorpg_game_server::{ZoneGameServerAdapter, build_zone_host, zone_match_id};
use mmorpg_protocol::{decode_snapshot, encode_command};
use serde::Deserialize;
use serde_json::json;

use crate::report::{Report, snake_kind, validate_name};

pub const SCHEMA: &str = "mmorpg.bot-scenario/v1";
const MAX_TICKS: u64 = 100_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotScenario {
    pub name: String,
    pub zone: u32,
    /// Number of simulation ticks to advance. Observations exist for ticks 1..=ticks.
    pub ticks: u64,
    #[serde(default = "default_grace")]
    pub reconnect_grace_ticks: u64,
    /// Print a digest every N ticks; ticks with steps, expectations or
    /// visibility changes are always printed.
    #[serde(default = "default_interval")]
    pub digest_interval: u64,
    pub bots: Vec<BotSpec>,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub expect: Vec<Expectation>,
}

fn default_grace() -> u64 {
    120
}

fn default_interval() -> u64 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotSpec {
    pub name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Join,
    Move,
    Jump,
    Disconnect,
    Reconnect,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Move => "move",
            Self::Jump => "jump",
            Self::Disconnect => "disconnect",
            Self::Reconnect => "reconnect",
        }
    }

    fn success(self) -> &'static str {
        match self {
            Self::Join => "joined",
            Self::Move | Self::Jump => "applied",
            Self::Disconnect => "disconnected",
            Self::Reconnect => "resumed",
        }
    }
}

/// One bot action, applied before the zone advances from `tick` to `tick + 1`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub tick: u64,
    pub bot: String,
    pub action: Action,
    /// Move intent, passed through unchanged; the zone accepts `-1..=1`.
    pub forward: Option<i8>,
    pub strafe: Option<i8>,
    /// Move heading: 65 536 steps per turn, 0 faces +Z, 16 384 faces +X.
    pub facing: Option<u16>,
    /// Command sequence override (move and jump); defaults to the bot's next sequence.
    pub seq: Option<u32>,
    /// Connection epoch override; defaults to the bot's current epoch.
    pub connection_epoch: Option<u32>,
    /// Expected outcome tag; defaults to the action's success tag.
    pub expect: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExpectKind {
    Sees,
    NotSees,
    Position,
    Acknowledged,
    Identity,
    VisibleCount,
    Area,
}

impl ExpectKind {
    fn name(self) -> &'static str {
        match self {
            Self::Sees => "sees",
            Self::NotSees => "not_sees",
            Self::Position => "position",
            Self::Acknowledged => "acknowledged",
            Self::Identity => "identity",
            Self::VisibleCount => "visible_count",
            Self::Area => "area",
        }
    }
}

/// A check against the decoded snapshot a bot received at an observed tick.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub kind: ExpectKind,
    pub bot: String,
    pub target: Option<String>,
    pub tick: Option<u64>,
    pub by_tick: Option<u64>,
    /// With `by_tick`: the window starts at this tick (default 1).
    pub from_tick: Option<u64>,
    pub position: Option<[i32; 3]>,
    pub sequence: Option<u32>,
    pub count: Option<usize>,
    /// Name of the core area the target stands in (`area` expectations).
    pub area: Option<String>,
}

/// Parses and validates a scenario at the file trust boundary.
pub fn load(text: &str) -> Result<BotScenario, String> {
    let scenario: BotScenario = toml::from_str(text).map_err(|error| error.to_string())?;
    validate(&scenario)?;
    Ok(scenario)
}

fn validate(scenario: &BotScenario) -> Result<(), String> {
    validate_name("scenario name", &scenario.name)?;
    if scenario.ticks == 0 || scenario.ticks > MAX_TICKS {
        return Err(format!("ticks must be in 1..={MAX_TICKS}"));
    }
    if scenario.digest_interval == 0 {
        return Err("digest_interval must be positive".into());
    }
    if scenario.bots.is_empty() || scenario.bots.len() > MAX_PLAYERS_PER_ZONE {
        return Err(format!(
            "bots must contain 1..={MAX_PLAYERS_PER_ZONE} entries"
        ));
    }
    let mut names = BTreeMap::new();
    for bot in &scenario.bots {
        validate_name("bot name", &bot.name)?;
        if names.insert(bot.name.as_str(), ()).is_some() {
            return Err(format!("bot {} is declared twice", bot.name));
        }
    }
    let known = |name: &str| {
        names
            .contains_key(name)
            .then_some(())
            .ok_or_else(|| format!("unknown bot {name}"))
    };
    let mut previous_tick = 0;
    for (index, step) in scenario.steps.iter().enumerate() {
        let at = format!("steps[{index}]");
        known(&step.bot).map_err(|error| format!("{at}: {error}"))?;
        if step.tick >= scenario.ticks {
            return Err(format!("{at}: tick must be below ticks"));
        }
        if step.tick < previous_tick {
            return Err(format!("{at}: steps must be ordered by tick"));
        }
        previous_tick = step.tick;
        let is_move = step.action == Action::Move;
        let intent = [
            step.forward.is_some(),
            step.strafe.is_some(),
            step.facing.is_some(),
        ];
        if intent.iter().any(|&present| present != is_move) {
            return Err(format!(
                "{at}: forward, strafe and facing are required for move and only for move"
            ));
        }
        let is_command = matches!(step.action, Action::Move | Action::Jump);
        if !is_command && (step.seq.is_some() || step.connection_epoch.is_some()) {
            return Err(format!(
                "{at}: seq/connection_epoch apply only to move and jump"
            ));
        }
    }
    for (index, expectation) in scenario.expect.iter().enumerate() {
        let at = format!("expect[{index}]");
        known(&expectation.bot).map_err(|error| format!("{at}: {error}"))?;
        if let Some(target) = &expectation.target {
            known(target).map_err(|error| format!("{at}: {error}"))?;
        }
        let tick = match (expectation.tick, expectation.by_tick) {
            (Some(tick), None) => tick,
            (None, Some(tick)) if expectation.kind == ExpectKind::Sees => tick,
            _ => {
                return Err(format!(
                    "{at}: exactly one of tick/by_tick is required (by_tick only for sees)"
                ));
            }
        };
        if tick == 0 || tick > scenario.ticks {
            return Err(format!("{at}: tick must be in 1..=ticks"));
        }
        if let Some(from) = expectation.from_tick
            && (expectation.by_tick.is_none() || from == 0 || from > tick)
        {
            return Err(format!(
                "{at}: from_tick needs by_tick and must be in 1..=by_tick"
            ));
        }
        let required = match expectation.kind {
            ExpectKind::Sees | ExpectKind::NotSees => expectation.target.is_some(),
            ExpectKind::Position => expectation.position.is_some(),
            ExpectKind::Acknowledged => expectation.sequence.is_some(),
            ExpectKind::Identity => true,
            ExpectKind::VisibleCount => expectation.count.is_some(),
            ExpectKind::Area => expectation.area.is_some(),
        };
        if !required {
            return Err(format!(
                "{at}: {} is missing its required field",
                expectation.kind.name()
            ));
        }
    }
    Ok(())
}

struct Bot {
    name: String,
    index: u32,
    tokens_minted: u32,
    lease: Option<SessionLease>,
    first_player: Option<PlayerId>,
    connected: bool,
    next_sequence: u32,
    view: Option<ZoneSnapshot>,
    previous_sees: Vec<String>,
}

impl Bot {
    /// Deterministic, per-bot unique tokens. They are test credentials only.
    fn mint_token(&mut self) -> ReconnectToken {
        self.tokens_minted += 1;
        let mut bytes = [0; RECONNECT_TOKEN_BYTES];
        bytes[..4].copy_from_slice(&(self.index + 1).to_be_bytes());
        bytes[4..8].copy_from_slice(&self.tokens_minted.to_be_bytes());
        ReconnectToken(bytes)
    }
}

struct Runner<'a> {
    scenario: &'a BotScenario,
    host: MatchHost<ZoneGameServerAdapter>,
    match_id: MatchId,
    bots: Vec<Bot>,
    report: Report,
    satisfied: Vec<bool>,
}

/// Runs a scenario. `Err` means the harness itself could not be built or
/// driven; scenario failures are recorded in the returned report instead.
pub fn run(scenario: &BotScenario) -> Result<Report, String> {
    let zone_id = ZoneId::new(scenario.zone);
    let host = build_zone_host([zone_id], scenario.reconnect_grace_ticks)
        .map_err(|error| error.to_string())?;
    let match_id = zone_match_id(zone_id).map_err(|error| error.to_string())?;
    let bots = (0_u32..)
        .zip(&scenario.bots)
        .map(|(index, spec)| Bot {
            name: spec.name.clone(),
            index,
            tokens_minted: 0,
            lease: None,
            first_player: None,
            connected: false,
            next_sequence: 1,
            view: None,
            previous_sees: Vec::new(),
        })
        .collect();
    let mut runner = Runner {
        scenario,
        host,
        match_id,
        bots,
        report: Report::new(SCHEMA),
        satisfied: vec![false; scenario.expect.len()],
    };
    runner.run()?;
    Ok(runner.report)
}

impl Runner<'_> {
    fn runtime<R>(
        &mut self,
        operation: impl FnOnce(&mut MatchRuntime<ZoneGameServerAdapter>) -> R,
    ) -> Result<R, String> {
        self.host
            .with_runtime_mut(&self.match_id, operation)
            .ok_or_else(|| format!("match {} is not hosted", self.match_id.as_str()))
    }

    fn bot_index(&self, name: &str) -> Result<usize, String> {
        self.bots
            .iter()
            .position(|bot| bot.name == name)
            .ok_or_else(|| format!("unknown bot {name}"))
    }

    fn run(&mut self) -> Result<(), String> {
        let scenario = self.scenario;
        self.report.line(
            "scenario",
            format!(
                "scenario {} zone={} bots={} ticks={} grace={}",
                scenario.name,
                scenario.zone,
                scenario.bots.len(),
                scenario.ticks,
                scenario.reconnect_grace_ticks
            ),
            json!({
                "scenario": scenario.name,
                "zone": scenario.zone,
                "bots": scenario.bots.iter().map(|bot| bot.name.as_str()).collect::<Vec<_>>(),
                "ticks": scenario.ticks,
                "reconnect_grace_ticks": scenario.reconnect_grace_ticks,
            }),
        );
        let mut next_step = 0;
        for tick in 0..scenario.ticks {
            let mut stepped = false;
            while let Some(step) = scenario.steps.get(next_step) {
                if step.tick != tick {
                    break;
                }
                self.apply(step)?;
                next_step += 1;
                stepped = true;
            }
            let observed = tick + 1;
            if let Err(error) = self.runtime(MatchRuntime::advance_tick)? {
                self.report.fail();
                self.report.line(
                    "error",
                    format!("t={observed} advance_tick FAIL {error}"),
                    json!({ "tick": observed, "error": error.to_string() }),
                );
                break;
            }
            let changed = self.observe(observed)?;
            let checked = self.evaluate(observed);
            if stepped
                || changed
                || checked
                || observed % scenario.digest_interval == 0
                || observed == scenario.ticks
            {
                self.digest(observed);
            }
        }
        let failures = self.report.failures();
        let verdict = if failures == 0 { "PASS" } else { "FAIL" };
        self.report.line(
            "result",
            format!(
                "{verdict} {} ticks={} steps={} expectations={} failures={failures}",
                scenario.name,
                scenario.ticks,
                scenario.steps.len(),
                scenario.expect.len()
            ),
            json!({
                "scenario": scenario.name,
                "passed": failures == 0,
                "ticks": scenario.ticks,
                "steps": scenario.steps.len(),
                "expectations": scenario.expect.len(),
                "failures": failures,
            }),
        );
        Ok(())
    }

    fn apply(&mut self, step: &Step) -> Result<(), String> {
        let index = self.bot_index(&step.bot)?;
        let (tag, detail) = match step.action {
            Action::Join => self.join(index)?,
            Action::Move | Action::Jump => self.submit(index, step)?,
            Action::Disconnect => self.disconnect(index)?,
            Action::Reconnect => self.reconnect(index)?,
        };
        let expected = step
            .expect
            .as_deref()
            .unwrap_or_else(|| step.action.success());
        let ok = tag == expected;
        let mut text = format!(
            "t={} {} {} -> {tag}",
            step.tick,
            step.bot,
            step.action.name()
        );
        if !detail.is_empty() {
            text.push(' ');
            text.push_str(&detail);
        }
        if !ok {
            self.report.fail();
            text.push_str(&format!(" FAIL expected {expected}"));
        }
        self.report.line(
            "step",
            text,
            json!({
                "tick": step.tick,
                "bot": step.bot,
                "action": step.action.name(),
                "outcome": tag,
                "detail": detail,
                "expected": expected,
                "ok": ok,
            }),
        );
        Ok(())
    }

    fn join(&mut self, index: usize) -> Result<(String, String), String> {
        let token = self.bots[index].mint_token();
        Ok(match self.runtime(|runtime| runtime.admit(token))? {
            Ok(lease) => {
                let bot = &mut self.bots[index];
                bot.first_player.get_or_insert(lease.player_id);
                bot.lease = Some(lease);
                bot.connected = true;
                (
                    "joined".into(),
                    format!(
                        "player={} epoch={}",
                        lease.player_id, lease.connection_epoch
                    ),
                )
            }
            Err(error) => (rejected(&error), String::new()),
        })
    }

    fn submit(&mut self, index: usize, step: &Step) -> Result<(String, String), String> {
        let bot = &mut self.bots[index];
        let Some(lease) = bot.lease else {
            return Ok(("rejected:no_session".into(), String::new()));
        };
        let sequence = step.seq.unwrap_or(bot.next_sequence);
        let epoch = step.connection_epoch.unwrap_or(lease.connection_epoch);
        bot.next_sequence = bot.next_sequence.max(sequence.saturating_add(1));
        let (payload, detail) = if step.action == Action::Jump {
            (
                encode_command(ZoneCommand::Jump),
                format!("seq={sequence} epoch={epoch}"),
            )
        } else {
            let (forward, strafe, facing) = (
                step.forward.unwrap_or_default(),
                step.strafe.unwrap_or_default(),
                step.facing.unwrap_or_default(),
            );
            (
                encode_command(ZoneCommand::Move {
                    forward,
                    strafe,
                    facing,
                }),
                format!(
                    "seq={sequence} epoch={epoch} forward={forward} strafe={strafe} facing={facing}"
                ),
            )
        };
        let outcome = self.runtime(|runtime| {
            runtime.submit_command(lease.player_id, epoch, sequence, &payload)
        })?;
        let tag = match outcome {
            Ok(CommandOutcome::Applied) => "applied".into(),
            Ok(CommandOutcome::IgnoredStale) => "ignored_stale".into(),
            Err(error) => rejected(&error),
        };
        Ok((tag, detail))
    }

    fn disconnect(&mut self, index: usize) -> Result<(String, String), String> {
        let Some(lease) = self.bots[index].lease else {
            return Ok(("rejected:no_session".into(), String::new()));
        };
        let disconnected =
            self.runtime(|runtime| runtime.disconnect(lease.player_id, lease.connection_epoch))?;
        if !disconnected {
            return Ok(("rejected:not_connected".into(), String::new()));
        }
        let bot = &mut self.bots[index];
        bot.connected = false;
        bot.view = None;
        Ok(("disconnected".into(), format!("player={}", lease.player_id)))
    }

    fn reconnect(&mut self, index: usize) -> Result<(String, String), String> {
        let Some(previous) = self.bots[index].lease else {
            return Ok(("rejected:no_session".into(), String::new()));
        };
        let replacement = self.bots[index].mint_token();
        Ok(
            match self
                .runtime(|runtime| runtime.reconnect(previous.reconnect_token, replacement))?
            {
                Ok(lease) => {
                    let bot = &mut self.bots[index];
                    bot.lease = Some(lease);
                    bot.connected = true;
                    let identity = if bot.first_player == Some(lease.player_id) {
                        "kept"
                    } else {
                        "changed"
                    };
                    (
                        "resumed".into(),
                        format!(
                            "player={} epoch={} identity={identity}",
                            lease.player_id, lease.connection_epoch
                        ),
                    )
                }
                Err(error) => (rejected(&error), String::new()),
            },
        )
    }

    /// Decodes every connected bot's snapshot bytes. Returns whether any
    /// bot's visible set changed.
    fn observe(&mut self, tick: u64) -> Result<bool, String> {
        let zone = ZoneId::new(self.scenario.zone);
        let mut changed = false;
        for index in 0..self.bots.len() {
            let bot = &self.bots[index];
            let Some(lease) = bot.lease.filter(|_| bot.connected) else {
                continue;
            };
            let bytes = self.runtime(|runtime| runtime.snapshot_for(lease.player_id))?;
            let decoded = bytes
                .map_err(|error| error.to_string())
                .and_then(|snapshot| {
                    decode_snapshot(&snapshot.payload).map_err(|error| error.to_string())
                })
                .and_then(|snapshot| {
                    if snapshot.zone_id != zone
                        || snapshot.tick != tick
                        || snapshot.viewer_id != lease.player_id
                    {
                        Err(format!(
                            "snapshot for zone {} tick {} viewer {} does not match zone {} tick {tick} viewer {}",
                            snapshot.zone_id.get(),
                            snapshot.tick,
                            snapshot.viewer_id,
                            zone.get(),
                            lease.player_id
                        ))
                    } else {
                        Ok(snapshot)
                    }
                });
            match decoded {
                Ok(snapshot) => {
                    let sees = self.sees(&snapshot);
                    let bot = &mut self.bots[index];
                    changed |= sees != bot.previous_sees;
                    bot.previous_sees = sees;
                    bot.view = Some(snapshot);
                }
                Err(error) => {
                    self.report.fail();
                    let name = self.bots[index].name.clone();
                    self.report.line(
                        "error",
                        format!("t={tick} {name} snapshot FAIL {error}"),
                        json!({ "tick": tick, "bot": name, "error": error }),
                    );
                    self.bots[index].view = None;
                }
            }
        }
        Ok(changed)
    }

    fn player_name(&self, player_id: PlayerId) -> String {
        self.bots
            .iter()
            .find(|bot| bot.lease.is_some_and(|lease| lease.player_id == player_id))
            .map_or_else(|| format!("p{player_id}"), |bot| bot.name.clone())
    }

    fn sees(&self, snapshot: &ZoneSnapshot) -> Vec<String> {
        snapshot
            .entities
            .iter()
            .filter_map(player_of_entity)
            .filter(|&player_id| player_id != snapshot.viewer_id)
            .map(|player_id| self.player_name(player_id))
            .collect()
    }

    fn player_of(&self, name: &str) -> Option<PlayerId> {
        self.bots
            .iter()
            .find(|bot| bot.name == name)
            .and_then(|bot| bot.lease)
            .map(|lease| lease.player_id)
    }

    fn digest(&mut self, tick: u64) {
        let mut parts = Vec::new();
        let mut objects = Vec::new();
        for bot in &self.bots {
            let Some(lease) = bot.lease else { continue };
            let view = bot.view.as_ref().filter(|_| bot.connected);
            let Some(view) = view else {
                parts.push(format!("{} p{} offline", bot.name, lease.player_id));
                objects.push(json!({
                    "bot": bot.name, "player": lease.player_id, "connected": false,
                }));
                continue;
            };
            let position = visible_player(view, lease.player_id).map(|entity| entity.position);
            let position_text =
                position.map_or_else(|| "(absent)".into(), |[x, y, z]| format!("({x},{y},{z})"));
            parts.push(format!(
                "{} p{} e{} ack{} {position_text} sees[{}]",
                bot.name,
                lease.player_id,
                lease.connection_epoch,
                view.acknowledged_sequence,
                bot.previous_sees.join(",")
            ));
            objects.push(json!({
                "bot": bot.name,
                "player": lease.player_id,
                "connected": true,
                "epoch": lease.connection_epoch,
                "ack": view.acknowledged_sequence,
                "position": position,
                "sees": bot.previous_sees,
            }));
        }
        self.report.line(
            "tick",
            format!("t={tick} {}", parts.join(" | ")),
            json!({ "tick": tick, "bots": objects }),
        );
    }

    /// Evaluates expectations that are due at `tick`. Returns whether any ran.
    fn evaluate(&mut self, tick: u64) -> bool {
        let scenario = self.scenario;
        let mut any = false;
        for (index, expectation) in scenario.expect.iter().enumerate() {
            let result = if expectation.tick == Some(tick) {
                self.check(expectation)
            } else if let Some(by_tick) = expectation.by_tick {
                let from = expectation.from_tick.unwrap_or(1);
                if self.satisfied[index] || tick < from || tick > by_tick {
                    continue;
                }
                match self.check(expectation) {
                    Ok(detail) => {
                        self.satisfied[index] = true;
                        Ok(detail)
                    }
                    Err(_) if tick < by_tick => continue,
                    Err(reason) => Err(reason),
                }
            } else {
                continue;
            };
            any = true;
            let label = describe(expectation);
            let (ok, detail) = match result {
                Ok(detail) => (true, detail),
                Err(reason) => (false, reason),
            };
            if !ok {
                self.report.fail();
            }
            let verdict = if ok { "ok" } else { "FAIL" };
            self.report.line(
                "expect",
                format!("t={tick} expect {label} {verdict} {detail}"),
                json!({
                    "tick": tick,
                    "index": index,
                    "kind": expectation.kind.name(),
                    "expectation": label,
                    "ok": ok,
                    "detail": detail,
                }),
            );
        }
        any
    }

    fn check(&self, expectation: &Expectation) -> Result<String, String> {
        let bot = self
            .bots
            .iter()
            .find(|bot| bot.name == expectation.bot)
            .ok_or("unknown bot")?;
        let view = bot
            .view
            .as_ref()
            .filter(|_| bot.connected)
            .ok_or_else(|| format!("{} received no snapshot", bot.name))?;
        let target_name = expectation.target.as_deref().unwrap_or(&bot.name);
        let target = self.player_of(target_name);
        let visible = |player: Option<PlayerId>| player.and_then(|id| visible_player(view, id));
        match expectation.kind {
            ExpectKind::Sees => visible(target)
                .map(|record| format!("{target_name} at {:?}", record.position))
                .ok_or_else(|| format!("{target_name} not visible")),
            ExpectKind::NotSees => match visible(target) {
                None => Ok(format!("{target_name} not visible")),
                Some(record) => Err(format!("{target_name} visible at {:?}", record.position)),
            },
            ExpectKind::Position => {
                let expected = expectation.position.unwrap_or_default();
                let record = visible(target).ok_or_else(|| format!("{target_name} not visible"))?;
                if record.position == expected {
                    Ok(format!("{:?}", record.position))
                } else {
                    Err(format!("got {:?}", record.position))
                }
            }
            ExpectKind::Acknowledged => {
                let expected = expectation.sequence.unwrap_or_default();
                if view.acknowledged_sequence == expected {
                    Ok(format!("ack={expected}"))
                } else {
                    Err(format!("got ack={}", view.acknowledged_sequence))
                }
            }
            ExpectKind::Identity => {
                let lease = bot.lease.ok_or("no session")?;
                if bot.first_player != Some(lease.player_id) {
                    return Err(format!(
                        "player changed from {:?} to {}",
                        bot.first_player, lease.player_id
                    ));
                }
                visible(Some(lease.player_id))
                    .map(|_| {
                        format!(
                            "player={} epoch={}",
                            lease.player_id, lease.connection_epoch
                        )
                    })
                    .ok_or_else(|| "own player missing from snapshot".into())
            }
            ExpectKind::VisibleCount => {
                let expected = expectation.count.unwrap_or_default();
                let count = view.entities.iter().filter_map(player_of_entity).count();
                if count == expected {
                    Ok(format!("count={expected}"))
                } else {
                    Err(format!("got count={count}"))
                }
            }
            ExpectKind::Area => {
                let expected = expectation.area.as_deref().unwrap_or_default();
                let [x, _, z] = visible(target)
                    .ok_or_else(|| format!("{target_name} not visible"))?
                    .position;
                // The areas of the vale content that `build_zone_host` installs.
                match greyhaven_vale::area_at(x, z).map(Area::name) {
                    Some(name) if name == expected => Ok(format!("{target_name} in {name}")),
                    Some(name) => Err(format!("{target_name} in {name}")),
                    None => Err(format!("{target_name} in no named area")),
                }
            }
        }
    }
}

/// The player behind a visible entity. Bots name, count and position players
/// only; creatures and NPCs have their own expectations.
fn player_of_entity(entity: &EntitySnapshot) -> Option<PlayerId> {
    match entity.kind {
        EntityKind::Player => Some(entity.id),
        EntityKind::Creature | EntityKind::Npc => None,
    }
}

fn visible_player(view: &ZoneSnapshot, player_id: PlayerId) -> Option<&EntitySnapshot> {
    view.entities
        .iter()
        .find(|entity| player_of_entity(entity) == Some(player_id))
}

fn describe(expectation: &Expectation) -> String {
    let bot = &expectation.bot;
    let target = expectation.target.as_deref().unwrap_or(bot);
    let by = match (expectation.from_tick, expectation.by_tick) {
        (Some(from), Some(by)) => format!(" within t={from}..={by}"),
        (None, Some(by)) => format!(" by t={by}"),
        _ => String::new(),
    };
    match expectation.kind {
        ExpectKind::Sees => format!("{bot} sees {target}{by}"),
        ExpectKind::NotSees => format!("{bot} not_sees {target}"),
        ExpectKind::Position => format!("{bot} position {target}"),
        ExpectKind::Acknowledged => format!("{bot} acknowledged"),
        ExpectKind::Identity => format!("{bot} identity"),
        ExpectKind::VisibleCount => format!("{bot} visible_count"),
        ExpectKind::Area => format!("{bot} area {target}"),
    }
}

fn rejected(error: &RuntimeError) -> String {
    let kind = match error {
        RuntimeError::Session(session) => snake_kind(&format!("{session:?}")),
        RuntimeError::Simulation(_) => "simulation".into(),
        other => snake_kind(&format!("{other:?}")),
    };
    format!("rejected:{kind}")
}
