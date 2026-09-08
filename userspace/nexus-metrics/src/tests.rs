// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests of the nexus-metrics wire codecs (split out of
//! lib.rs for the structure ratchet).
//! OWNERS: @runtime
//! STATUS: Experimental
//! TEST_COVERAGE: this file

use super::*;
use core::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn test_reject_metric_name_over_limit() {
    let name = [b'a'; MAX_METRIC_NAME_LEN + 1];
    assert_eq!(MetricName::new(&name), Err(EncodeError::InvalidArgs));
}

#[test]
fn test_reject_attrs_over_limit() {
    let attrs = [b'k'; MAX_ATTRS_LEN + 1];
    assert_eq!(BoundedFields::attrs(&attrs), Err(EncodeError::OverLimit));
}

#[test]
fn test_deterministic_ids_are_monotonic() {
    let mut ids = DeterministicIdSource::new(0xAA55);
    let s1 = ids.next_span_id().0;
    let s2 = ids.next_span_id().0;
    let t1 = ids.next_trace_id().0;
    assert!(s2 > s1);
    assert_ne!(s1, t1);
}

#[test]
fn test_send_sync_boundaries() {
    assert_send_sync::<DeterministicIdSource>();
    assert_send_sync::<SeriesId>();
    assert_send_sync::<SpanId>();
    assert_send_sync::<TraceId>();
    assert_send_sync::<EncodeError>();
    assert_send_sync::<DecodeError>();
    assert_send_sync::<ClientError>();
}

#[test]
fn test_counter_wire_roundtrip() {
    let frame = encode_counter_inc(
        7,
        MetricName::new(b"sched.wakeups").unwrap(),
        BoundedFields::labels(b"svc=timed\n").unwrap(),
        3,
    )
    .unwrap();
    let req = decode_request(&frame).unwrap();
    match req {
        Request::CounterInc { nonce, name, labels, delta } => {
            assert_eq!(nonce, 7);
            assert_eq!(name, b"sched.wakeups");
            assert_eq!(labels, b"svc=timed\n");
            assert_eq!(delta, 3);
        }
        _ => panic!("wrong request variant"),
    }
}

#[test]
fn test_best_effort_macros_record_events_on_host_backend() {
    let mut backend = host::HostBackend::new();
    metrics_counter_inc!(backend, "boot.events", 1);
    metrics_gauge_set!(backend, "sched.depth", -2);
    metrics_hist_observe!(backend, "timed.latency", 77);
    assert_eq!(backend.events().len(), 3);
}

struct FakeSpanClient {
    calls: AtomicUsize,
    span: AtomicU64,
    end_ns: AtomicU64,
    status: AtomicU8,
}

impl FakeSpanClient {
    const fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            span: AtomicU64::new(0),
            end_ns: AtomicU64::new(0),
            status: AtomicU8::new(0),
        }
    }
}

impl SpanEndClient for FakeSpanClient {
    type Error = ();

    fn end_span(
        &self,
        span_id: SpanId,
        end_ns: u64,
        status: u8,
        _attrs: &[u8],
    ) -> Result<u8, Self::Error> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.span.store(span_id.0, Ordering::Relaxed);
        self.end_ns.store(end_ns, Ordering::Relaxed);
        self.status.store(status, Ordering::Relaxed);
        Ok(STATUS_OK)
    }
}

fn fixed_end_now() -> u64 {
    1234
}

#[test]
fn test_span_guard_drop_sends_end_once() {
    let fake = FakeSpanClient::new();
    {
        let _guard = SpanGuard::new(&fake, SpanId(77), fixed_end_now);
    }
    assert_eq!(fake.calls.load(Ordering::Relaxed), 1);
    assert_eq!(fake.span.load(Ordering::Relaxed), 77);
    assert_eq!(fake.end_ns.load(Ordering::Relaxed), 1234);
    assert_eq!(fake.status.load(Ordering::Relaxed), STATUS_OK);
}

#[test]
fn test_span_guard_manual_end_disarms_drop() {
    let fake = FakeSpanClient::new();
    let guard = SpanGuard::new(&fake, SpanId(99), fixed_end_now);
    let _ = guard.end(777, 3, b"result=ok\n");
    assert_eq!(fake.calls.load(Ordering::Relaxed), 1);
    assert_eq!(fake.span.load(Ordering::Relaxed), 99);
    assert_eq!(fake.end_ns.load(Ordering::Relaxed), 777);
    assert_eq!(fake.status.load(Ordering::Relaxed), 3);
}
