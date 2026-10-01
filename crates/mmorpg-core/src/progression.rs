//! Immutable starter progression rules. Award eligibility and mutable XP
//! belong to the authoritative zone; these queries never grant experience.

use crate::unit::MAX_UNIT_LEVEL;

/// Starter progression ends at level 10; creatures retain their own level bound.
pub const MAX_PLAYER_LEVEL: u8 = 10;
const NEXT_LEVEL_EXPERIENCE: [u32; 10] = [100, 200, 300, 400, 500, 600, 700, 800, 900, 0];

/// Current-level XP needed for the next level. The cap returns zero;
/// unsupported player levels return `None`. XP is not a lifetime total.
#[must_use]
pub fn experience_to_next_level(level: u8) -> Option<u32> {
    level
        .checked_sub(1)
        .and_then(|index| NEXT_LEVEL_EXPERIENCE.get(usize::from(index)))
        .copied()
}

/// Kill reward before tap/range/liveness eligibility checks.
///
/// Base XP is 45 + 5 × creature level. Each level below the player reduces
/// it by 20%; five or more levels below gives zero. Each level above adds
/// 10%, capped at +50%. Integer rewards round down. Capped players get zero.
/// Unsupported player/creature levels return `None`, not a fallback reward.
#[must_use]
pub fn kill_experience(player_level: u8, creature_level: u8) -> Option<u32> {
    experience_to_next_level(player_level)?;
    if !(1..=MAX_UNIT_LEVEL).contains(&creature_level) {
        return None;
    }
    if player_level == MAX_PLAYER_LEVEL {
        return Some(0);
    }
    let percent = if creature_level < player_level {
        100_u32.saturating_sub(20 * u32::from(player_level - creature_level))
    } else {
        100 + 10 * u32::from((creature_level - player_level).min(5))
    };
    Some((45 + 5 * u32::from(creature_level)) * percent / 100)
}
