// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The service launcher asks the permission authority what a spawned app holds
//! (TASK-0333 6d-3, RFC-0111 §6, ADR-0084) — one waited call per app spawn over the declared
//! `execd → policyd` leg, the answer or policyd's death ends it (no clock). The grants decide
//! which SDK service routes the child gets (`provision_app_service_routes`); the build-time
//! manifest table that used to decide is gone. Fail-closed: no answer ⇒ no routes, and the
//! reason is a marker (`execd: FAIL app grants (app= …)`), never a silent empty set.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (every `execd: app route granted svc=` on the ladder rides this ask)

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use nexus_abi::policyd as wire;
use nexus_service_topology::slots::execd as topo;

static NONCE: AtomicU32 = AtomicU32::new(0x6d32_0000);

/// The grants policyd holds for `app`, or `None` when it holds none (DENY: never published or
/// rejected at publish) or could not be asked — each refusal named on the console.
pub(super) fn fetch(app: &str) -> Option<Vec<String>> {
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    let mut req = [0u8; 4 + 4 + 1 + wire::MAX_APP_ID_BYTES];
    let Some(len) = wire::encode_app_grants_get(nonce, app.as_bytes(), &mut req) else {
        fail(app, "app id");
        return None;
    };
    let mut rsp = [0u8; 4 + 4 + 2 + wire::MAX_APP_GRANTS * (1 + wire::MAX_GRANT_BYTES)];
    let answer = nexus_ipc::exchange::call_matching(
        topo::POLICYD.send,
        topo::REPLY,
        &req[..len],
        &mut rsp,
        |frame| {
            let (n, status, list) = wire::decode_app_grants_rsp(frame)?;
            (n == nonce).then(|| {
                let grants: Vec<String> =
                    list.filter_map(|g| core::str::from_utf8(g).ok()).map(String::from).collect();
                (status, grants)
            })
        },
    );
    match answer {
        Ok((wire::STATUS_ALLOW, grants)) => Some(grants),
        Ok(_) => {
            fail(app, "denied");
            None
        }
        Err(_) => {
            fail(app, "ipc");
            None
        }
    }
}

fn fail(app: &str, why: &str) {
    nexus_abi::DebugLine::new()
        .put(b"execd: FAIL app grants (app=")
        .str(app)
        .put(b" ")
        .str(why)
        .put(b")")
        .emit();
}
