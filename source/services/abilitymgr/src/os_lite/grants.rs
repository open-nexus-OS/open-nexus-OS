// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The lifecycle authority asks the permission authority (TASK-0333 6d-3, RFC-0111 §6,
//! ADR-0084): one waited call per launch over the declared `abilitymgr → policyd` leg, the
//! answer or policyd's death ends it (no clock). `None` for a DENY (the app was never published,
//! or its manifest was rejected) and for a transport failure alike — the caller refuses the
//! launch (`abilitymgr: launch denied (grants app=`); the failure is its own marker
//! (`abilitymgr: FAIL grants ask (app=`) so a missing leg is never read as a missing app.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (every `abilitymgr: launch (app=` on the ladder rides this ask; the
//!   decision is host-tested in `wire.rs`)

use alloc::string::String;
use alloc::vec::Vec;

use nexus_abi::policyd as wire;
use nexus_service_topology::slots::abilitymgr as topo;

/// The policyd leg's client: a nonce per ask, matched on the shared reply inbox.
pub(super) struct PolicydGrants {
    nonce: u32,
}

impl PolicydGrants {
    pub(super) const fn new() -> Self {
        Self { nonce: 0x6d31_0000 }
    }
}

impl crate::wire::GrantSource for PolicydGrants {
    fn grants_of(&mut self, app_id: &str) -> Option<Vec<String>> {
        self.nonce = self.nonce.wrapping_add(1);
        let nonce = self.nonce;
        let mut req = [0u8; 4 + 4 + 1 + wire::MAX_APP_ID_BYTES];
        let len = wire::encode_app_grants_get(nonce, app_id.as_bytes(), &mut req)?;
        // The answer: status + up to MAX_APP_GRANTS names of MAX_GRANT_BYTES.
        let mut rsp = [0u8; 4 + 4 + 2 + wire::MAX_APP_GRANTS * (1 + wire::MAX_GRANT_BYTES)];
        let answer = nexus_ipc::exchange::call_matching(
            topo::POLICYD.send,
            topo::REPLY,
            &req[..len],
            &mut rsp,
            |frame| {
                let (n, status, list) = wire::decode_app_grants_rsp(frame)?;
                (n == nonce).then(|| {
                    let grants: Vec<String> = list
                        .filter_map(|g| core::str::from_utf8(g).ok())
                        .map(String::from)
                        .collect();
                    (status, grants)
                })
            },
        );
        match answer {
            Ok((wire::STATUS_ALLOW, grants)) => Some(grants),
            Ok(_) => None,
            Err(_) => {
                nexus_abi::debug_ts_prefix();
                super::emit_prefix(b"abilitymgr: FAIL grants ask (app=");
                super::emit_str(app_id);
                super::emit_prefix(b")");
                super::emit_newline();
                None
            }
        }
    }
}
