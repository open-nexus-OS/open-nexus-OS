// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for deterministic repeat timing over injected monotonic time.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 5 integration tests.
//!
//! TEST_SCOPE:
//!   - repeat delay/rate scheduling
//!   - release cancellation
//!   - repeat configuration rejects
//!
//! TEST_SCENARIOS:
//!   - repeat_schedule_uses_injectable_time_deterministically()
//!   - repeat_release_cancels_future_repeats()
//!   - a_stall_gives_one_repeat_and_the_rhythm_resumes_from_it()
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
    assert_eq!(engine.tick(ns(299)).expect("before delay"), None);

    let first = engine.tick(ns(300)).expect("first repeat").expect("due");
    assert_eq!(first.key(), key);
    assert_eq!(first.timestamp(), ns(300));

    assert_eq!(engine.tick(ns(549)).expect("between repeats"), None);
    let second = engine.tick(ns(550)).expect("second repeat").expect("due");
    assert_eq!(second.key(), key);
    assert_eq!(engine.next_deadline_ns(), Some(ns(800).raw()), "the rhythm holds when on time");
}

#[test]
fn a_stall_gives_one_repeat_and_the_rhythm_resumes_from_it() {
    let config =
        RepeatConfig::new(DelayMs::new(300).expect("delay"), RateHz::new(4).expect("rate"))
            .expect("config");
    let mut engine = RepeatEngine::new(config);
    let key = RepeatKey::new(0x04).expect("key");
    engine.press(key, ns(0)).expect("press");
    // The consumer slept through eleven repeats (300 ms + 10 × 250 ms): ONE comes, stamped when
    // it was first due, and the next is a period after now — no burst, no stuck key.
    let late = engine.tick(ns(2_900)).expect("late tick").expect("due");
    assert_eq!(late.timestamp(), ns(300));
    assert_eq!(engine.tick(ns(2_900)).expect("same instant"), None, "one per tick");
    assert_eq!(engine.next_deadline_ns(), Some(ns(3_150).raw()));
    assert!(engine.tick(ns(3_150)).expect("resumed").is_some());
}

#[test]
fn repeat_release_cancels_future_repeats() {
    let config =
        RepeatConfig::new(DelayMs::new(200).expect("delay"), RateHz::new(5).expect("rate"))
            .expect("config");
    let mut engine = RepeatEngine::new(config);
    let key = RepeatKey::new(0x05).expect("key");

    engine.press(key, ns(0)).expect("press");
    assert!(engine.tick(ns(200)).expect("first").is_some());
    engine.release(key);
    assert_eq!(engine.tick(ns(600)).expect("after release"), None);
    assert_eq!(engine.next_deadline_ns(), None, "nothing held, no timer");
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
