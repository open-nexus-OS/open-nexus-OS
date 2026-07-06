// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The `svc.*` adapter layer — the effect-side bridge between the
//! interpreter's [`EffectHost`](crate::EffectHost) seam and concrete
//! backends. Host: [`TranscriptHost`] (deterministic record/replay of
//! request→response exchanges, docs/dev/dsl/services.md). The real IPC host
//! rides with the app-host process (TASK-0080D) behind the same trait.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests in `transcript.rs` + `tests/dsl_conformance`

mod transcript;
mod value_text;

pub use transcript::{Recorder, TranscriptHost, ERR_TRANSCRIPT_MISS};
pub use value_text::{parse_value, value_to_text};
