// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The settings page's rows (`svc.shortcuts.list`, RFC-0113 §7): one category's entries
//! in table order, their chords named for the page's language. app-host maps a row onto the DSL
//! record `ShortcutRow { id, keys, alt, active }`; the page names it from its language files by id.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/registry.rs`; the page in `tests/dsl_apps_conformance/tests/settings.rs`

use alloc::string::String;
use alloc::vec::Vec;

use crate::chord::Category;
use crate::label::{chord_label, chords_label};

/// One row of the page's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: &'static str,
    /// The primary chord.
    pub keys: String,
    /// The alternates, ` · ` between them; "" for none.
    pub alt: String,
    /// `false` = matched but nothing acts on it yet.
    pub active: bool,
}

/// The rows of the category with this id (`Category::id`); an unknown id lists nothing.
#[must_use]
pub fn listing(category: &str, lang: &str) -> Vec<Listed> {
    let Some(category) = Category::from_id(category) else {
        return Vec::new();
    };
    crate::in_category(category)
        .filter_map(|entry| {
            let (primary, alternates) = entry.chords.split_first()?;
            Some(Listed {
                id: entry.id,
                keys: chord_label(primary, lang)?,
                alt: chords_label(alternates, lang)?,
                active: entry.active,
            })
        })
        .collect()
}
