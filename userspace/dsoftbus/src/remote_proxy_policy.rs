// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-testable remote proxy policy helpers (TASK-0005)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 unit tests (userspace/dsoftbus/tests/reject_remote_proxy.rs)
//!
//! SECURITY INVARIANTS:
//! - Deny-by-default: only explicitly allowlisted services are permitted.
//! - Inputs are bounded: oversized requests are rejected deterministically.
//! - No secrets are present or logged here (pure policy).
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

/// Maximum remote proxy request size (bytes).
pub const MAX_REMOTE_PROXY_REQ: usize = 256;

/// Allowlisted remote services for TASK-0005.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteService {
    Samgrd,
    Bundlemgrd,
}

impl RemoteService {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Samgrd => "samgrd",
            Self::Bundlemgrd => "bundlemgrd",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenyReason {
    Unauthenticated,
    ServiceNotAllowed,
    OversizedRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEvent {
    pub service: RemoteService,
    pub request_len: usize,
}

/// Authorize a remote proxy call (deny-by-default).
pub fn authorize_remote_proxy(
    authenticated: bool,
    service: Option<RemoteService>,
    request_len: usize,
) -> Result<AuditEvent, DenyReason> {
    if !authenticated {
        return Err(DenyReason::Unauthenticated);
    }
    let Some(service) = service else {
        return Err(DenyReason::ServiceNotAllowed);
    };
    if request_len > MAX_REMOTE_PROXY_REQ {
        return Err(DenyReason::OversizedRequest);
    }
    Ok(AuditEvent { service, request_len })
}
