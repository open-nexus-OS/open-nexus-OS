// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the `svc.files.list`/`svc.files.count` visibility predicate
//! (RFC-0084 Phase 6) — deliberately a pure function over a NAME, so it can be
//! host-tested. `effect_host.rs` is `#![cfg]`-gated to the RISC-V OS target and
//! nothing in it can be exercised on the host; a filter that decides what a
//! file manager shows is not something to leave to a boot log.
//!
//! ONE predicate, shared by both methods. A counter that says "7 objects" over
//! 3 visible rows is a lie that survives review, and the only structural way
//! to prevent it is to make the two calls physically unable to disagree.
//!
//! OWNERS: @ui
//! STATUS: Functional (RFC-0084 Phase 6)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (`cargo test -p app-host`)

/// Is this entry visible under the active filter?
///
/// - `query`: case-insensitive substring of the name; `""` matches everything.
///   ASCII-folded on purpose — there is no locale-aware casing available here,
///   and a half-correct Unicode fold would be worse than an honest ASCII one
///   (a German user searching "Ü" gets an exact match, not a wrong one).
/// - `show_hidden`: admit dot-files.
///
/// Allocation-free: the app-host runs on a bump allocator that never frees, and
/// this is called once per entry per listing.
#[must_use]
pub(crate) fn name_visible(name: &str, query: &str, show_hidden: bool) -> bool {
    if !show_hidden && name.starts_with('.') {
        return false;
    }
    if query.is_empty() {
        return true;
    }
    let needle = query.as_bytes();
    let hay = name.as_bytes();
    if needle.len() > hay.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::name_visible;

    #[test]
    fn an_empty_query_matches_everything_visible() {
        assert!(name_visible("IMG_4521.jpg", "", false));
        assert!(name_visible("Bilder", "", false));
    }

    #[test]
    fn dot_files_need_show_hidden() {
        assert!(!name_visible(".metadata_cache", "", false));
        assert!(name_visible(".metadata_cache", "", true));
        // A dot INSIDE the name is not a hidden file.
        assert!(name_visible("IMG_4521.jpg", "", false));
    }

    #[test]
    fn the_query_is_a_case_insensitive_substring() {
        assert!(name_visible("IMG_4521.jpg", "img", false));
        assert!(name_visible("IMG_4521.jpg", "4521", false));
        assert!(name_visible("IMG_4521.jpg", ".JPG", false));
        assert!(!name_visible("IMG_4521.jpg", "png", false));
    }

    #[test]
    fn a_query_longer_than_the_name_cannot_match() {
        // `windows()` panics on a zero-length slice and yields nothing when the
        // needle is longer — the explicit guard keeps both cases honest.
        assert!(!name_visible("a.txt", "a-much-longer-query", false));
    }

    #[test]
    fn hidden_and_query_compose_rather_than_override() {
        // A dot-file matching the query still stays hidden until asked for.
        assert!(!name_visible(".thumbnails", "thumb", false));
        assert!(name_visible(".thumbnails", "thumb", true));
    }

    #[test]
    fn non_ascii_names_match_exactly_not_case_folded() {
        // Documented limit: the fold is ASCII, so "Ü" matches "Ü" but not "ü".
        assert!(name_visible("Übersicht.txt", "Über", false));
        assert!(!name_visible("Übersicht.txt", "über", false));
        // The ASCII part of a mixed name still folds.
        assert!(name_visible("Übersicht.TXT", ".txt", false));
    }
}
