// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Negative tests for DSoftBus remote proxy policy (TASK-0005)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 4 unit tests
//!
//! TEST_SCOPE:
//! - Reject unauthenticated calls.
//! - Reject non-allowlisted services.
//! - Enforce bounded request sizes.
//! - Emit auditable allow decisions (host model).
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use dsoftbus::remote_proxy_policy::{
    authorize_remote_proxy, DenyReason, RemoteService, MAX_REMOTE_PROXY_REQ,
};

#[test]
fn test_reject_remote_unauthenticated_remote_call() {
    let err = authorize_remote_proxy(false, Some(RemoteService::Samgrd), 8).unwrap_err();
    assert_eq!(err, DenyReason::Unauthenticated);
}

#[test]
fn test_reject_remote_disallowed_service_proxy() {
    let err = authorize_remote_proxy(true, None, 8).unwrap_err();
    assert_eq!(err, DenyReason::ServiceNotAllowed);
}

#[test]
fn test_reject_remote_oversized_remote_request() {
    let err = authorize_remote_proxy(true, Some(RemoteService::Bundlemgrd), MAX_REMOTE_PROXY_REQ + 1)
        .unwrap_err();
    assert_eq!(err, DenyReason::OversizedRequest);
}

#[test]
fn test_reject_remote_audit_remote_call_logged() {
    let ev = authorize_remote_proxy(true, Some(RemoteService::Samgrd), 42).unwrap();
    assert_eq!(ev.service.as_str(), "samgrd");
    assert_eq!(ev.request_len, 42);
}
