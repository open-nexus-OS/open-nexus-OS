// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Filter-variant index. The proof/target-test filter panel (word
//! list, scrollbar, input glyphs, layout build) was deleted in RFC-0067 C1.
//! Only `filter_layout_variant_index` remains — it maps the typed text to a
//! variant index that still drives the filter selftest markers.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via windowd selftest markers

use super::LIVE_FILTER_VARIANTS;

pub(crate) fn filter_layout_variant_index(filter_text: &str) -> usize {
    let mut best_idx = 0;
    let mut best_len = 0;
    for (idx, candidate) in LIVE_FILTER_VARIANTS.iter().enumerate() {
        if filter_text.starts_with(candidate) && candidate.len() >= best_len {
            best_idx = idx;
            best_len = candidate.len();
        }
    }
    best_idx
}
