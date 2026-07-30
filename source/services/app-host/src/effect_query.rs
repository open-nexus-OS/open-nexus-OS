// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the embedded `nexus-query` demo store (the chat transcript
//! corpus) — split out of `effect_host.rs` (structure ratchet). v1 catalog:
//! one `messages` table until statefsd-backed tables land (@persist wiring).
//! OWNERS: @runtime
//! STATUS: Functional (demo data source)
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised via the chat lazy-loading QEMU path.

use alloc::string::String;

/// The embedded `nexus-query` engine + its KV. v1 catalog: one demo
/// `messages` table (seq Int pk/order, text Str) seeded with a large
/// transcript — the scrolling/lazy-loading proof corpus until statefsd-backed
/// tables land (@persist wiring).
pub(crate) struct QueryStore {
    pub(crate) engine: nexus_query::Engine,
    pub(crate) kv: nexus_query::MemKv,
}

/// Demo transcript scale: large enough that only WINDOWS of it are ever
/// resident in the DSL store (the lazy-loading contract), small enough to
/// seed in one bounded pass.
// Demo-source size: the synthetic generator below is zero-resident (only the
// requested page is materialized), so this is just the upper bound of the
// transcript. The store-window builtin `tail(list, 96)` in chat.store.nx keeps
// the resident `messages` list (and the derived emit/layout/paint/concat cost)
// bounded, so paging this far no longer grows unbounded. The ceiling now is the
// non-freeing bump heap's tolerance for the per-page whole-scene re-emit churn
// (see chat.store.nx); 300 pages cleanly, unbounded needs emit-virtualization.
pub(crate) const SEED_MESSAGES: i64 = 300;

impl QueryStore {
    pub(crate) fn seeded() -> Self {
        use nexus_query::{QType, QVal, TableDef};
        let engine = nexus_query::Engine::new(alloc::vec![TableDef {
            id: 0,
            columns: alloc::vec![QType::Int, QType::Str],
            pk_col: 0,
            indexed: alloc::vec![0],
        }]);
        let mut kv = nexus_query::MemKv::new();
        // Deterministic two-voice transcript (no external data source yet).
        const LINES: [&str; 6] = [
            "Hast du den neuen Build schon gebootet?",
            "Ja - der Frost-Effekt sitzt jetzt richtig.",
            "Dann teste mal drei Fenster gleichzeitig.",
            "Laeuft. Fokus und Drag fuehlen sich gut an.",
            "Als naechstes kommt das lange Transcript.",
            "Genau dafuer ist diese Nachricht da.",
        ];
        for seq in 1..=SEED_MESSAGES {
            // Pure line — the UI derives the voice from the seq PARITY
            // (even = "you", right-aligned accent bubble) and renders the
            // sender label itself; a "Mira #3:" prefix inside the bubble
            // was the old plain-list look.
            let line = LINES[(seq as usize) % LINES.len()];
            let mut text = String::new();
            let _ = core::fmt::write(&mut text, format_args!("{line} (#{seq})"));
            let _ = engine.put(&mut kv, 0, &[QVal::Int(seq), QVal::Str(text)]);
        }
        Self { engine, kv }
    }

    /// Column index of `name` in the `messages` table.
    pub(crate) fn col(name: &str) -> usize {
        match name {
            "text" => 1,
            _ => 0, // seq (pk/order)
        }
    }
}
