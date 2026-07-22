// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0146/0149 / RFC-0075 deterministic IME composition core:
//! Latin dead keys + the CJK engines (JP romaji→kana→kanji, KR 2-set jamo,
//! ZH pinyin) behind the ONE `ImeEngine` trait, plus the bounded user-dict
//! API. Hosted by `imed`; no I/O, no IPC, no alloc.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable for RFC-0075 Phase 0
//! TEST_COVERAGE: Unit + integration tests in `tests/compose_contract.rs`.
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

mod compose;
mod engine;
mod jp;
mod kr;
mod outcome;
mod userdict;
mod zh;

pub use compose::{Composer, COMPOSE_PENDING_MAX};
pub use engine::{
    Candidate, CandidatePage, Engine, EngineId, EngineOutcome, ImeEngine, TextRun,
    CANDIDATE_MAX_BYTES, CANDIDATE_PAGE_MAX,
};
pub use jp::JpEngine;
pub use kr::KrEngine;
pub use outcome::{Commit, ImeAction, ImeKey, ImeOutcome, Preedit, PREEDIT_MAX_BYTES};
pub use userdict::{UserDict, USERDICT_CAP, USERDICT_KEY_MAX, USERDICT_TEXT_MAX};
pub use zh::ZhEngine;

/// Every character the shipped engines can OUTPUT (kana, lexicon kanji/han)
/// — consumed by the font bake so typed/candidate glyphs are always covered
/// (RFC-0075 Phase 8d). Deterministic order is irrelevant (the consumer
/// sorts + dedups).
#[cfg(not(all(nexus_env = "os", target_os = "none")))]
#[must_use]
pub fn engine_output_chars() -> Vec<char> {
    let mut out = Vec::new();
    jp::output_chars(&mut out);
    zh::output_chars(&mut out);
    out
}
