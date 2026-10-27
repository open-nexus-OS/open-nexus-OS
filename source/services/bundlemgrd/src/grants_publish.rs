// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The registry publishes every installed app's manifest grants to the one permission
//! authority (policyd) — TASK-0333 6d-3, RFC-0111 §6, ADR-0084. Once per boot, from the
//! verified volume's `meta/app.properties` sidecars: the manifest's `caps`, plus
//! `nexus.permission.BACKGROUND` when `background = [...]` is non-empty. OWED, not timed: the
//! declared policyd leg is a late grant (pinned after the volume services run), so the publish
//! waits for the leg and runs BEFORE this service answers the first request after it — a reader
//! that saw our answer can never ask policyd ahead of the publish. One waited call per app, the
//! answer or policyd's death ends it; a refusal is a marker (`bundlemgrd: FAIL app grants
//! (app= status=)`) and the app stays unpublished — the authority answers DENY for it.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `grant_set_is_caps_plus_background` (host); the chain on the headless lane
//!   (`policyd: app grants (app=`, `SELFTEST: app grants ok (app=`)

use alloc::string::String;
use alloc::vec::Vec;

use crate::volume::AppInfo;

/// The grant names one app publishes: its `caps`, and `nexus.permission.BACKGROUND` once when
/// the manifest declares any background mode (the modes themselves are the runtime's later).
pub(crate) fn grant_set(app: &AppInfo) -> Vec<&str> {
    let mut grants: Vec<&str> = app.permissions.iter().map(String::as_str).collect();
    if !app.background.is_empty() && !grants.contains(&nexus_abi::policyd::PERMISSION_BACKGROUND) {
        grants.push(nexus_abi::policyd::PERMISSION_BACKGROUND);
    }
    grants
}

/// The owed publish: `None` until done; the loop calls [`Publisher::settle`] before it answers.
pub(crate) struct Publisher {
    done: bool,
    nonce: u32,
}

impl Publisher {
    pub(crate) const fn new() -> Self {
        Self { done: false, nonce: 0x6d30 }
    }

    /// Publishes every app of a verified volume once the policyd leg is live. Idempotent after
    /// success; a volume not (yet) verified or a leg not (yet) pinned leaves the debt standing.
    pub(crate) fn settle(&mut self, volume: &crate::volume::VolumeState) {
        if self.done {
            return;
        }
        let apps = volume.apps();
        if !matches!(volume, crate::volume::VolumeState::Verified(_)) {
            return;
        }
        let leg = nexus_service_topology::slots::bundlemgrd::POLICYD;
        // The leg is pinned by init after this service runs: present or not, never a guess.
        match nexus_abi::cap_clone(leg.send) {
            Ok(clone) => {
                let _ = nexus_abi::cap_close(clone);
            }
            Err(_) => return,
        }
        let reply = nexus_service_topology::slots::bundlemgrd::REPLY;
        let mut published = 0u32;
        for app in apps {
            let grants = grant_set(app);
            let grant_bytes: Vec<&[u8]> = grants.iter().map(|g| g.as_bytes()).collect();
            self.nonce = self.nonce.wrapping_add(1);
            let nonce = self.nonce;
            let mut req = [0u8; 1 + 4 + 4 + 48 + 1 + 16 * 49];
            let Some(len) = nexus_abi::policyd::encode_app_grants_put(
                nonce,
                app.id.as_bytes(),
                &grant_bytes,
                &mut req,
            ) else {
                emit_fail(&app.id, "encode");
                continue;
            };
            let mut rsp = [0u8; 16];
            let outcome =
                nexus_ipc::exchange::call_matching(leg.send, reply, &req[..len], &mut rsp, |f| {
                    nexus_abi::policyd::decode_rsp_v2(f)
                        .filter(|(op, n, _)| {
                            *op == nexus_abi::policyd::OP_APP_GRANTS_PUT && *n == nonce
                        })
                        .map(|(_, _, status)| status)
                });
            match outcome {
                Ok(nexus_abi::policyd::STATUS_ALLOW) => published += 1,
                Ok(status) => emit_fail(&app.id, status_name(status)),
                Err(_) => emit_fail(&app.id, "ipc"),
            }
        }
        self.done = true;
        crate::os_lite::emit_line(&alloc::format!(
            "bundlemgrd: app grants published (n={published}/{})",
            apps.len()
        ));
    }
}

fn status_name(status: u8) -> &'static str {
    match status {
        nexus_abi::policyd::STATUS_DENY => "deny",
        nexus_abi::policyd::STATUS_MALFORMED => "malformed",
        nexus_abi::policyd::STATUS_UNSUPPORTED => "unsupported",
        _ => "status",
    }
}

fn emit_fail(app: &str, why: &str) {
    crate::os_lite::emit_line(&alloc::format!("bundlemgrd: FAIL app grants (app={app} {why})"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn grant_set_is_caps_plus_background() {
        let mut app = crate::volume::parse_app_properties(
            "notes",
            b"permissions=nexus.permission.WINDOW,nexus.permission.NOTIFY\n",
        );
        assert_eq!(grant_set(&app), ["nexus.permission.WINDOW", "nexus.permission.NOTIFY"]);
        app.background = alloc::vec!["audio".to_string()];
        assert_eq!(
            grant_set(&app),
            ["nexus.permission.WINDOW", "nexus.permission.NOTIFY", "nexus.permission.BACKGROUND"]
        );
        // Declared twice over (a manifest that also lists the grant) stays one grant.
        app.permissions.push("nexus.permission.BACKGROUND".to_string());
        assert_eq!(grant_set(&app).len(), 3);
    }
}
