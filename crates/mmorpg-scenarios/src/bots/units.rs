//! Scenario vocabulary for units and combat feedback: which unit a step or
//! expectation names, which event it looks for and which visible state it
//! checks. Parsed once at the file boundary; matching reads only decoded
//! projections, so the runner still owns no gameplay rule.

use std::fmt;

use mmorpg_core::{CreatureId, EntityRef, EntitySnapshot, ErrorCode, NpcId, ZoneEvent};
use serde::Deserialize;

/// A unit named in a scenario: `none`, `bot:<name>`, `creature:<id>` or `npc:<id>`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "String")]
pub enum UnitSpec {
    None,
    Bot(String),
    Creature(u32),
    Npc(u32),
}

impl TryFrom<String> for UnitSpec {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text == "none" {
            return Ok(Self::None);
        }
        let invalid =
            || format!("unit {text:?} must be none, bot:<name>, creature:<id> or npc:<id>");
        let (kind, value) = text.split_once(':').ok_or_else(invalid)?;
        let id = || value.parse::<u32>().map_err(|_| invalid());
        match kind {
            "bot" if !value.is_empty() => Ok(Self::Bot(value.to_owned())),
            "creature" => Ok(Self::Creature(id()?)),
            "npc" => Ok(Self::Npc(id()?)),
            _ => Err(invalid()),
        }
    }
}

impl fmt::Display for UnitSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Bot(name) => write!(formatter, "bot:{name}"),
            Self::Creature(id) => write!(formatter, "creature:{id}"),
            Self::Npc(id) => write!(formatter, "npc:{id}"),
        }
    }
}

impl UnitSpec {
    /// The unit this names; `player_of` maps a bot to its current player.
    /// `Err` carries the bot name when that bot has no session.
    pub fn resolve(
        &self,
        player_of: impl Fn(&str) -> Option<u32>,
    ) -> Result<Option<EntityRef>, String> {
        Ok(match self {
            Self::None => None,
            Self::Bot(name) => Some(EntityRef::Player(
                player_of(name).ok_or_else(|| name.clone())?,
            )),
            Self::Creature(id) => Some(EntityRef::Creature(CreatureId::new(*id))),
            Self::Npc(id) => Some(EntityRef::Npc(NpcId::new(*id))),
        })
    }
}

/// A feedback event kind: `damage_dealt`, `damage_taken`, `miss`, `died`,
/// `evade` or `error:<code>`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "String")]
pub enum EventSpec {
    DamageDealt,
    DamageTaken,
    Miss,
    Died,
    Evade,
    Error(ErrorCode),
}

impl TryFrom<String> for EventSpec {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        Ok(match text.as_str() {
            "damage_dealt" => Self::DamageDealt,
            "damage_taken" => Self::DamageTaken,
            "miss" => Self::Miss,
            "died" => Self::Died,
            "evade" => Self::Evade,
            _ => match text.strip_prefix("error:").and_then(parse_error_code) {
                Some(code) => Self::Error(code),
                None => {
                    return Err(format!(
                        "event {text:?} must be damage_dealt, damage_taken, miss, died, evade or error:<code>"
                    ));
                }
            },
        })
    }
}

impl fmt::Display for EventSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DamageDealt => formatter.write_str("damage_dealt"),
            Self::DamageTaken => formatter.write_str("damage_taken"),
            Self::Miss => formatter.write_str("miss"),
            Self::Died => formatter.write_str("died"),
            Self::Evade => formatter.write_str("evade"),
            Self::Error(code) => write!(formatter, "error:{}", error_code_name(*code)),
        }
    }
}

impl EventSpec {
    /// Whether `event`, received by `viewer`, is of this kind and, when
    /// `unit` is given, concerns that unit.
    pub fn matches(self, event: &ZoneEvent, viewer: EntityRef, unit: Option<EntityRef>) -> bool {
        let kind = match (self, event) {
            (Self::DamageDealt, ZoneEvent::DamageDealt { .. })
            | (Self::DamageTaken, ZoneEvent::DamageTaken { .. })
            | (Self::Miss, ZoneEvent::Miss { .. })
            | (Self::Died, ZoneEvent::Died { .. })
            | (Self::Evade, ZoneEvent::Evade { .. }) => true,
            (Self::Error(expected), ZoneEvent::Error { code, .. }) => expected == *code,
            _ => false,
        };
        kind && unit.is_none_or(|unit| concerned_unit(event, viewer) == Some(unit))
    }
}

/// The unit an event is about, seen from its recipient: whom it hit or who
/// hit it, who died, who evaded, or the target an error concerned.
pub fn concerned_unit(event: &ZoneEvent, viewer: EntityRef) -> Option<EntityRef> {
    match *event {
        ZoneEvent::DamageDealt { target, .. } | ZoneEvent::Evade { target, .. } => Some(target),
        ZoneEvent::DamageTaken { source, .. } => Some(source),
        ZoneEvent::Miss { source, target } => Some(if source == viewer { target } else { source }),
        ZoneEvent::Died { entity, .. } => Some(entity),
        ZoneEvent::Error { target, .. } => target,
    }
}

/// A presentation fact about a unit in a bot's projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UnitState {
    /// In the projection and not dead.
    Alive,
    /// In the projection and dead: a corpse or a dead player.
    Dead,
    /// Not in the projection.
    Absent,
    InCombat,
    Evading,
    TargetsViewer,
    TappedByOther,
}

impl UnitState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Dead => "dead",
            Self::Absent => "absent",
            Self::InCombat => "in_combat",
            Self::Evading => "evading",
            Self::TargetsViewer => "targets_viewer",
            Self::TappedByOther => "tapped_by_other",
        }
    }

    /// Whether a unit's projected record (`None` when absent) shows this state.
    pub fn holds(self, record: Option<&EntitySnapshot>) -> bool {
        let Some(record) = record else {
            return matches!(self, Self::Absent);
        };
        let flags = record.flags;
        match self {
            Self::Alive => !flags.dead,
            Self::Dead => flags.dead,
            Self::Absent => false,
            Self::InCombat => flags.in_combat,
            Self::Evading => flags.evading,
            Self::TargetsViewer => flags.targets_viewer,
            Self::TappedByOther => flags.tapped_by_other,
        }
    }
}

/// Stable snake-case name of an error code.
pub const fn error_code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::NoTarget => "no_target",
        ErrorCode::OutOfRange => "out_of_range",
        ErrorCode::TargetDead => "target_dead",
        ErrorCode::NotAttackable => "not_attackable",
        ErrorCode::YouAreDead => "you_are_dead",
        ErrorCode::NotDead => "not_dead",
        ErrorCode::InvalidTarget => "invalid_target",
    }
}

fn parse_error_code(name: &str) -> Option<ErrorCode> {
    Some(match name {
        "no_target" => ErrorCode::NoTarget,
        "out_of_range" => ErrorCode::OutOfRange,
        "target_dead" => ErrorCode::TargetDead,
        "not_attackable" => ErrorCode::NotAttackable,
        "you_are_dead" => ErrorCode::YouAreDead,
        "not_dead" => ErrorCode::NotDead,
        "invalid_target" => ErrorCode::InvalidTarget,
        _ => return None,
    })
}

/// A compact digest token for one event; `name` renders units.
pub fn event_token(event: &ZoneEvent, name: impl Fn(EntityRef) -> String) -> String {
    let critical = |critical: bool| if critical { "!" } else { "" };
    match *event {
        ZoneEvent::DamageDealt {
            target,
            amount,
            critical: crit,
            ..
        } => format!("dealt:{}:{amount}{}", name(target), critical(crit)),
        ZoneEvent::DamageTaken {
            source,
            amount,
            critical: crit,
            ..
        } => format!("taken:{}:{amount}{}", name(source), critical(crit)),
        ZoneEvent::Miss { source, target } => format!("miss:{}>{}", name(source), name(target)),
        ZoneEvent::Died { entity, .. } => format!("died:{}", name(entity)),
        ZoneEvent::Evade { target, .. } => format!("evade:{}", name(target)),
        ZoneEvent::Error { code, .. } => format!("error:{}", error_code_name(code)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWER: EntityRef = EntityRef::Player(1);
    const WOLF: EntityRef = EntityRef::Creature(CreatureId::new(108));

    #[test]
    fn unit_specs_round_trip_and_reject_malformed_names() {
        for text in ["none", "bot:alice", "creature:108", "npc:8"] {
            let spec = UnitSpec::try_from(text.to_owned()).unwrap();
            assert_eq!(spec.to_string(), text);
        }
        for text in ["", "bot:", "wolf:1", "creature:x", "creature:-1", "npc"] {
            assert!(UnitSpec::try_from(text.to_owned()).is_err(), "{text}");
        }
        let player_of = |name: &str| (name == "alice").then_some(4);
        assert_eq!(
            UnitSpec::Bot("alice".into()).resolve(player_of),
            Ok(Some(EntityRef::Player(4)))
        );
        assert_eq!(
            UnitSpec::Bot("bob".into()).resolve(player_of),
            Err("bob".into())
        );
        assert_eq!(UnitSpec::None.resolve(player_of), Ok(None));
    }

    #[test]
    fn event_specs_match_kind_and_the_concerned_unit() {
        for code in [
            ErrorCode::NoTarget,
            ErrorCode::OutOfRange,
            ErrorCode::TargetDead,
            ErrorCode::NotAttackable,
            ErrorCode::YouAreDead,
            ErrorCode::NotDead,
            ErrorCode::InvalidTarget,
        ] {
            let spec = EventSpec::Error(code);
            assert_eq!(EventSpec::try_from(spec.to_string()), Ok(spec));
        }
        assert!(EventSpec::try_from("error:bogus".to_owned()).is_err());
        let missed = ZoneEvent::Miss {
            source: WOLF,
            target: VIEWER,
        };
        assert!(EventSpec::Miss.matches(&missed, VIEWER, Some(WOLF)));
        assert!(!EventSpec::Miss.matches(&missed, VIEWER, Some(VIEWER)));
        assert!(!EventSpec::Died.matches(&missed, VIEWER, None));
        let died = ZoneEvent::Died {
            entity: WOLF,
            killer: Some(VIEWER),
        };
        assert!(EventSpec::Died.matches(&died, VIEWER, Some(WOLF)));
        let name = |entity: EntityRef| format!("{entity:?}");
        assert_eq!(
            event_token(
                &ZoneEvent::DamageDealt {
                    source: VIEWER,
                    target: WOLF,
                    amount: 7,
                    critical: true,
                },
                name
            ),
            "dealt:Creature(CreatureId(108)):7!"
        );
    }
}
