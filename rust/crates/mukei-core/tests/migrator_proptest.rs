//! Architect review GH #36 / GH #43 — property-based fuzz of
//! `Migrator::verify_order`.
//!
//! The migration order check is one of the few places where a silent
//! pass means corrupted user data on every subsequent boot.

#![cfg(feature = "rusqlite")]

use mukei_core::error::MukeiError;
use mukei_core::storage::migrations::{MigrationRecord, Migrator};
use proptest::prelude::*;

fn record(id: u32) -> MigrationRecord {
    MigrationRecord {
        id,
        name: format!("V{id:03}__test"),
        applied_at: chrono::Utc::now(),
        checksum: "x".into(),
    }
}

fn avail(ids: &[u32]) -> Vec<(u32, String, String)> {
    ids.iter()
        .map(|i| (*i, format!("V{i:03}__test"), String::new()))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, .. ProptestConfig::default() })]

    /// Property #1 — strictly-contiguous applied prefix is always ok.
    #[test]
    fn contiguous_prefix_verifies_clean(applied_n in 0u32..32, extra in 0u32..16) {
        let applied: Vec<MigrationRecord> = (1..=applied_n).map(record).collect();
        let avail_ids: Vec<u32> = (1..=(applied_n + extra)).collect();
        let availv = avail(&avail_ids);
        let res = Migrator::verify_order(&availv, &applied);
        prop_assert!(
            res.is_ok(),
            "contiguous prefix rejected: applied 1..={}, avail 1..={}",
            applied_n,
            applied_n + extra,
        );
    }

    /// Property #2 — any gap (no matter the surrounding context) is a
    /// conflict.
    #[test]
    fn any_gap_is_a_conflict(gap_after in 1u32..16, after_gap in 1u32..8) {
        // applied = [1..=gap_after, (gap_after+2)..=(gap_after+1+after_gap)]
        // -- the missing id `gap_after + 1` is the gap.
        let mut applied: Vec<MigrationRecord> = (1..=gap_after).map(record).collect();
        for i in (gap_after + 2)..=(gap_after + 1 + after_gap) {
            applied.push(record(i));
        }
        let max = gap_after + 1 + after_gap;
        let availv = avail(&(1..=max).collect::<Vec<_>>());
        let err = Migrator::verify_order(&availv, &applied)
            .expect_err("gap should always conflict");
        prop_assert!(
            matches!(err, MukeiError::MigrationOrderConflict { .. }),
            "gap should yield MigrationOrderConflict, got {err:?}",
        );
    }

    /// Property #3 — an applied id that exceeds max(available) is a
    /// conflict.
    #[test]
    fn applied_beyond_available_is_a_conflict(max_avail in 1u32..16, overshoot in 1u32..8) {
        let availv = avail(&(1..=max_avail).collect::<Vec<_>>());
        let mut applied: Vec<MigrationRecord> = (1..=max_avail).map(record).collect();
        applied.push(record(max_avail + overshoot));
        let err = Migrator::verify_order(&availv, &applied)
            .expect_err("applied beyond available should always conflict");
        prop_assert!(
            matches!(err, MukeiError::MigrationOrderConflict { .. }),
            "overshoot should yield MigrationOrderConflict, got {err:?}",
        );
    }
}
