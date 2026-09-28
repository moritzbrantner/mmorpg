//! Derived XZ broad phase for interest policy, never for collision or authority.

use std::collections::{BTreeMap, BTreeSet};

use crate::{INTEREST_RADIUS_UNITS, PlayerId, ZoneSnapshot};

/// Deterministic query work, excluding index maintenance and serialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InterestQueryStats {
    pub cells_visited: usize,
    pub candidates_tested: usize,
}

/// Cumulative deterministic index maintenance work since zone construction or recovery.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InterestMaintenanceStats {
    pub full_rebuilds: usize,
    pub bucket_inserts: usize,
    pub bucket_removes: usize,
    pub bucket_moves: usize,
    pub players_inspected: usize,
}

/// The same projection used for publication, with deterministic workload evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerProjection {
    pub snapshot: ZoneSnapshot,
    pub stats: InterestQueryStats,
}

#[derive(Default)]
pub(crate) struct InterestIndex {
    cells: BTreeMap<(i64, i64), BTreeSet<PlayerId>>,
    memberships: BTreeMap<PlayerId, (i64, i64)>,
}

impl InterestIndex {
    pub(crate) fn insert(&mut self, player_id: PlayerId, x: i32, z: i32) {
        let cell = Self::cell(x, z);
        debug_assert!(!self.memberships.contains_key(&player_id));
        self.cells.entry(cell).or_default().insert(player_id);
        self.memberships.insert(player_id, cell);
    }

    pub(crate) fn remove(&mut self, player_id: PlayerId) {
        let Some(cell) = self.memberships.remove(&player_id) else {
            return;
        };
        if let Some(players) = self.cells.get_mut(&cell) {
            players.remove(&player_id);
            if players.is_empty() {
                self.cells.remove(&cell);
            }
        }
    }

    /// Returns true only when a bucket membership actually changes.
    pub(crate) fn move_if_needed(&mut self, player_id: PlayerId, x: i32, z: i32) -> bool {
        let cell = Self::cell(x, z);
        if self.memberships.get(&player_id) == Some(&cell) {
            return false;
        }
        self.remove(player_id);
        self.insert(player_id, x, z);
        true
    }

    pub(crate) fn candidates(&self, x: i32, z: i32) -> (Vec<PlayerId>, InterestQueryStats) {
        let (center_x, center_z) = Self::cell(x, z);
        let mut candidates = Vec::new();
        let mut stats = InterestQueryStats::default();
        // Cell width equals the interest radius: all relevant centers are in these
        // nine cells. Use i64 and floor division for negative/extreme coordinates.
        for cell_x in center_x - 1..=center_x + 1 {
            for cell_z in center_z - 1..=center_z + 1 {
                stats.cells_visited += 1;
                if let Some(players) = self.cells.get(&(cell_x, cell_z)) {
                    candidates.extend(players);
                }
            }
        }
        candidates.sort_unstable();
        stats.candidates_tested = candidates.len();
        (candidates, stats)
    }

    fn cell(x: i32, z: i32) -> (i64, i64) {
        let width = i64::from(INTEREST_RADIUS_UNITS);
        (
            i64::from(x).div_euclid(width),
            i64::from(z).div_euclid(width),
        )
    }
}
