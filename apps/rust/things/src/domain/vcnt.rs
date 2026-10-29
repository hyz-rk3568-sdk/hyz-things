//! Pure card-level virtual scheduling shared by independent training pools.

use std::collections::HashMap;

/// Card-only snapshots; each training pool loads its own independent state.
#[derive(Clone, Debug, Default)]
pub struct CardSchedule {
    pub cards: HashMap<String, i64>,
    pub increments: HashMap<String, i64>,
}

impl CardSchedule {
    pub fn card(&self, id: &str) -> i64 {
        self.cards.get(id).copied().unwrap_or(0)
    }

    pub fn increment(&self, id: &str) -> i64 {
        self.increments.get(id).copied().unwrap_or(1024)
    }
}

/// Return at most `count` unique IDs; input order does not affect seeded ties.
/// Missing cards start at the pool's minimum VCNT, or zero for an empty pool.
pub fn select_lowest_distinct<'a>(
    ids: &[&'a str],
    schedule: &CardSchedule,
    seed: u64,
    count: usize,
) -> Vec<&'a str> {
    let mut cards = ids.to_vec();
    cards.sort_unstable();
    cards.dedup();
    let minimum = schedule.cards.values().copied().min().unwrap_or(0);
    let mut state = seed;
    let mut ranked: Vec<_> = cards
        .into_iter()
        .map(|id| {
            // SplitMix64 gives every seed a distinct initial state, including zero.
            state = state.wrapping_add(0x9e3779b97f4a7c15);
            let mut tie = state;
            tie = (tie ^ (tie >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            tie = (tie ^ (tie >> 27)).wrapping_mul(0x94d049bb133111eb);
            tie ^= tie >> 31;
            (schedule.cards.get(id).copied().unwrap_or(minimum), tie, id)
        })
        .collect();
    ranked.sort_unstable();
    ranked
        .into_iter()
        .take(count)
        .map(|(_, _, id)| id)
        .collect()
}

/// Use only submitted, actually answered results frozen at planning time.
pub fn delta_vcnt(correct: u64, answered: u64) -> i64 {
    let p = (correct as f64 + 2.0) / (answered as f64 + 4.0);
    let weight = (1.4 - 0.8 * p).clamp(0.7, 1.3);
    (1024.0 / weight).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_distinct_cards_by_lowest_snapshot_before_seeded_ties() {
        let mut schedule = CardSchedule::default();
        schedule.cards.insert("high".into(), 20);
        schedule.cards.insert("middle".into(), 10);
        schedule.cards.insert("low".into(), 0);
        let ids = ["high", "middle", "low", "low"];
        assert_eq!(
            select_lowest_distinct(&ids, &schedule, 19, 3),
            vec!["low", "middle", "high"]
        );
        assert_eq!(select_lowest_distinct(&ids, &schedule, 19, 1), vec!["low"]);
        assert!(select_lowest_distinct(&ids, &schedule, 19, 0).is_empty());
        assert!(select_lowest_distinct(&[], &schedule, 19, 10).is_empty());
        assert_eq!(select_lowest_distinct(&ids, &schedule, 19, 10).len(), 3);
    }

    #[test]
    fn seeded_ties_ignore_input_order_and_do_not_change_snapshots() {
        let schedule = CardSchedule::default();
        let ids = ["a", "b", "c", "d", "e", "f"];
        let reversed = ["f", "e", "d", "c", "b", "a", "a"];
        let first = select_lowest_distinct(&ids, &schedule, 0, 6);
        assert_eq!(first, select_lowest_distinct(&reversed, &schedule, 0, 6));
        assert_eq!(first, select_lowest_distinct(&ids, &schedule, 0, 6));
        assert_ne!(first, select_lowest_distinct(&ids, &schedule, 1, 6));
        assert!(schedule.cards.is_empty());
        assert!(schedule.increments.is_empty());
    }

    #[test]
    fn unseen_cards_start_at_minimum_without_changing_legacy_defaults() {
        let mut schedule = CardSchedule::default();
        schedule.cards.insert("lowest".into(), 500);
        schedule.cards.insert("high".into(), 900);
        schedule.increments.insert("lowest".into(), 838);
        for seed in 0..32 {
            let selected = select_lowest_distinct(&["high", "new", "lowest"], &schedule, seed, 3);
            assert_eq!(selected[2], "high");
        }
        assert_eq!(schedule.card("new"), 0);
        assert_eq!(schedule.increment("new"), 1024);
        assert_eq!(schedule.increment("lowest"), 838);
    }

    #[test]
    fn shared_weight_matches_previous_data_analysis_formula() {
        for answered in 0..100 {
            for correct in 0..=answered {
                let p = (correct as f64 + 2.0) / (answered as f64 + 4.0);
                let previous = (1024.0 / (1.4 - 0.8 * p).clamp(0.7, 1.3)).round() as i64;
                assert_eq!(delta_vcnt(correct, answered), previous);
            }
        }
    }

    #[test]
    fn weighted_increment_favors_weak_cards() {
        assert_eq!(delta_vcnt(0, 0), 1024);
        assert_eq!(delta_vcnt(0, 5), 838);
        assert_eq!(delta_vcnt(5, 5), 1317);
        assert_eq!(delta_vcnt(0, u64::MAX), 788);
        assert_eq!(delta_vcnt(u64::MAX, u64::MAX), 1463);
    }
}
