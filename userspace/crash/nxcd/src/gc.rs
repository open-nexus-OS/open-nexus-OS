// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Pure, deterministic GC/budget planning for crash-dump directories.
//! The planner decides which dump ids to delete; the caller (`nx crash purge`)
//! applies the plan to the filesystem.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests below; integration in `tests/crashdump_v2_host`
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

/// One dump candidate as seen by the planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcEntry {
    /// Stable identifier (file name).
    pub id: String,
    /// On-disk size in bytes.
    pub bytes: u64,
    /// Capture timestamp (newer dumps are kept preferentially).
    pub timestamp_nsec: u64,
}

/// Retention budget. Both limits apply; `0` means "keep nothing" for counts
/// and "no byte budget" is expressed with `u64::MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcBudget {
    pub max_total_bytes: u64,
    pub max_count: usize,
}

/// Deterministic purge plan: keep the newest dumps that fit the budget,
/// return the ids to delete in ascending id order.
///
/// Ordering: newest first by `timestamp_nsec`, ties broken by descending id
/// so the plan is total and reproducible for identical inputs.
pub fn plan_purge(entries: &[GcEntry], budget: &GcBudget) -> Vec<String> {
    let mut sorted: Vec<&GcEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| b.timestamp_nsec.cmp(&a.timestamp_nsec).then_with(|| b.id.cmp(&a.id)));

    let mut kept_bytes: u64 = 0;
    let mut kept_count: usize = 0;
    let mut delete = Vec::new();
    for entry in sorted {
        let fits_count = kept_count < budget.max_count;
        let fits_bytes = kept_bytes.saturating_add(entry.bytes) <= budget.max_total_bytes;
        if fits_count && fits_bytes {
            kept_bytes = kept_bytes.saturating_add(entry.bytes);
            kept_count += 1;
        } else {
            delete.push(entry.id.clone());
        }
    }
    delete.sort();
    delete
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, bytes: u64, ts: u64) -> GcEntry {
        GcEntry { id: String::from(id), bytes, timestamp_nsec: ts }
    }

    #[test]
    fn test_gc_keeps_newest_within_byte_budget() {
        let entries =
            vec![entry("a.nxcd", 100, 1), entry("b.nxcd", 100, 2), entry("c.nxcd", 100, 3)];
        let plan = plan_purge(&entries, &GcBudget { max_total_bytes: 200, max_count: 10 });
        assert_eq!(plan, vec![String::from("a.nxcd")]);
    }

    #[test]
    fn test_gc_enforces_count_budget() {
        let entries = vec![entry("a.nxcd", 1, 1), entry("b.nxcd", 1, 2), entry("c.nxcd", 1, 3)];
        let plan = plan_purge(&entries, &GcBudget { max_total_bytes: u64::MAX, max_count: 1 });
        assert_eq!(plan, vec![String::from("a.nxcd"), String::from("b.nxcd")]);
    }

    #[test]
    fn test_gc_is_deterministic_with_timestamp_ties() {
        let entries = vec![entry("b.nxcd", 10, 5), entry("a.nxcd", 10, 5), entry("c.nxcd", 10, 5)];
        let a = plan_purge(&entries, &GcBudget { max_total_bytes: 20, max_count: 10 });
        let b = plan_purge(&entries, &GcBudget { max_total_bytes: 20, max_count: 10 });
        assert_eq!(a, b);
        // Ties break by descending id: keep "c" and "b", delete "a".
        assert_eq!(a, vec![String::from("a.nxcd")]);
    }

    #[test]
    fn test_gc_empty_and_zero_budget() {
        assert!(plan_purge(&[], &GcBudget { max_total_bytes: 0, max_count: 0 }).is_empty());
        let entries = vec![entry("a.nxcd", 1, 1)];
        let plan = plan_purge(&entries, &GcBudget { max_total_bytes: u64::MAX, max_count: 0 });
        assert_eq!(plan, vec![String::from("a.nxcd")]);
    }
}
