// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for deterministic repeat timing over injected monotonic time.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 4 integration tests.
//!
//! TEST_SCOPE:
//!   - repeat delay/rate scheduling
//!   - release cancellation
//!   - repeat configuration rejects
//!
//! TEST_SCENARIOS:
//!   - repeat_schedule_uses_injectable_time_deterministically()
//!   - repeat_release_cancels_future_repeats()
//!   - test_reject_* repeat config rejects
//!
//! DEPENDENCIES:
//!   - `key_repeat` crate scheduler and typed config
//!
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use key_repeat::{DelayMs, MonotonicNs, RateHz, RepeatConfig, RepeatEngine, RepeatKey};

fn ns(ms: u64) -> MonotonicNs {
    MonotonicNs::new(ms * 1_000_000)
}

#[test]
fn repeat_schedule_uses_injectable_time_deterministically() {
    let config =
        RepeatConfig::new(DelayMs::new(300).expect("delay"), RateHz::new(4).expect("rate"))
            .expect("config");
    let mut engine = RepeatEngine::new(config);
    let key = RepeatKey::new(0x04).expect("key");

    engine.press(key, ns(0)).expect("press");
    assert!(engine.tick(ns(299)).expect("before delay").is_empty());

    let first = engine.tick(ns(300)).expect("first repeat");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].key(), key);

    assert!(engine.tick(ns(549)).expect("between repeats").is_empty());
    let second = engine.tick(ns(550)).expect("second repeat");
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].key(), key);
}

#[test]
fn repeat_release_cancels_future_repeats() {
    let config =
        RepeatConfig::new(DelayMs::new(200).expect("delay"), RateHz::new(5).expect("rate"))
            .expect("config");
    let mut engine = RepeatEngine::new(config);
    let key = RepeatKey::new(0x05).expect("key");

    engine.press(key, ns(0)).expect("press");
    assert_eq!(engine.tick(ns(200)).expect("first").len(), 1);
    engine.release(key);
    assert!(engine.tick(ns(600)).expect("after release").is_empty());
}

#[test]
fn test_reject_repeat_config_zero_delay() {
    let err = DelayMs::new(0).unwrap_err();
    assert_eq!(err.code(), "repeat.delay.invalid");
}

#[test]
fn test_reject_repeat_config_zero_rate() {
    let err = RateHz::new(0).unwrap_err();
    assert_eq!(err.code(), "repeat.rate.invalid");
}
