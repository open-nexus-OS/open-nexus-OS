// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The one permission authority for apps (TASK-0333 6d-3, RFC-0111 §6, ADR-0084).
//! The bundle manager publishes an installed app's manifest grants (`nexus.permission.*`,
//! `app.<bundle>.<CAP>`) here once per boot from the verified system volume; the lifecycle
//! authority and the service launcher ASK and never read a manifest or a build-time table.
//! Pure and host-tested: the policy table decides who may publish (`app.grants.publish`) and
//! who may read (`app.grants.read`), an unknown permission rejects the whole app, an app never
//! published answers DENY (fail-closed), and the table is bounded.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 9 tests (publish/read round trip, the three `test_reject_*`, bounds,
//!   namespaces, the SDK route permissions)

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use nexus_abi::policyd as wire;
use nexus_sel::Policy;

/// The capability a sender needs to publish an app's grants (the bundle manager's).
pub const CAP_PUBLISH: &str = "app.grants.publish";
/// The capability a sender needs to read an app's grants (the launch authority's, the
/// service launcher's, the proof harness's).
pub const CAP_READ: &str = "app.grants.read";
/// The one grant of 6d-3 with a runtime effect of its own: an app holding it is not frozen
/// for being hidden (RFC-0111 §4); it comes from a non-empty `background = [...]` in the
/// manifest, never from `caps`.
pub const BACKGROUND: &str = wire::PERMISSION_BACKGROUND;
/// The grant of 6e-1: the app's `@persist` snapshot survives a kill; it comes from
/// `restorable = true` in the manifest, never from `caps`.
pub const STATE: &str = wire::PERMISSION_STATE;
/// Most apps the table holds (the verified volume carries well under this).
pub const MAX_APPS: usize = 32;

/// The platform permissions the authority recognizes. A manifest permission outside this set
/// — and outside an app's own `app.<bundle>.<CAP>` namespace — rejects the app at publish
/// (fail-closed). Grows as real permissions are introduced; the namespace mirrors the manifest.
pub const KNOWN_PERMISSIONS: &[&str] = &[
    // Bind a window surface via windowd (every UI app needs this).
    "nexus.permission.WINDOW",
    // Post notifications via notifd (RFC-0065 notifications).
    "nexus.permission.NOTIFY",
    // The app's own durable record in statefsd (`svc.state`): `restorable = true` in the
    // manifest (TASK-0333 6e-1, ADR-0086) — never named in `caps`.
    STATE,
    // Execute QuerySpec v1 queries via queryd (docs/dev/dsl/db-queries.md).
    "nexus.permission.QUERY",
    // System-role permissions (TASK-0080C): the privilege ceiling (nxb-pack) only lets the
    // role's bundle type ship them; here they are recognized as valid platform permissions.
    "nexus.permission.SESSION",
    "nexus.permission.LAUNCH",
    "nexus.permission.ENUMERATE",
    "nexus.permission.SETTINGS",
    "nexus.permission.FILES",
    "nexus.permission.TIME",
    "nexus.permission.IME",
    "nexus.permission.UPDATES",
    // Reach clipboardd (TASK-0067 / RFC-0094): copy and paste are every app's.
    "nexus.permission.CLIPBOARD",
    // Reach screencapd (TASK-0068 / RFC-0095): shell- and settings-type only (nxb-pack).
    "nexus.permission.SCREENCAP",
    // Not frozen for being hidden (RFC-0111 §4): `background = [...]` in the manifest.
    BACKGROUND,
];

/// `true` for a recognized platform permission or a well-formed app-owned one
/// (`app.<bundle>.<CAP>`, manifest v2.2 exports — the namespace is the bundle's own; nxb-pack
/// refuses an export outside its bundle's namespace at pack time).
pub fn is_known_permission(cap: &str) -> bool {
    KNOWN_PERMISSIONS.contains(&cap) || is_app_owned(cap)
}

fn is_app_owned(cap: &str) -> bool {
    let Some(rest) = cap.strip_prefix("app.") else { return false };
    let Some((bundle, name)) = rest.split_once('.') else { return false };
    let ok = |s: &str| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    ok(bundle) && ok(name)
}

/// The published grants, by app id — ONE table per boot, replaced per app on every publish.
pub struct AppGrants {
    apps: Vec<(String, Vec<String>)>,
}

impl Default for AppGrants {
    fn default() -> Self {
        Self::new()
    }
}

impl AppGrants {
    pub const fn new() -> Self {
        Self { apps: Vec::new() }
    }

    /// The apps published so far.
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    /// `true` while nothing was published.
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// The grants of `app` — `None` when the app was never published (fail-closed: the
    /// caller treats it as "no grants, and the app is unknown").
    pub fn grants_of(&self, app: &str) -> Option<&[String]> {
        self.apps.iter().find(|(id, _)| id == app).map(|(_, g)| g.as_slice())
    }

    fn remove(&mut self, app: &str) -> bool {
        let before = self.apps.len();
        self.apps.retain(|(id, _)| id != app);
        before != self.apps.len()
    }

    /// Stores `grants` for `app` (replacing an earlier entry); `Err(())` when the table is
    /// full and the app is new.
    fn put(&mut self, app: &str, grants: Vec<String>) -> Result<bool, ()> {
        let replaced = self.remove(app);
        if self.apps.len() >= MAX_APPS {
            return Err(());
        }
        self.apps.push((String::from(app), grants));
        Ok(replaced)
    }
}

/// What a handled frame did — the OS half prints the marker from it.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<'a> {
    /// A publish was stored: `n` grants for `app` (`replaced`: an earlier entry went).
    Stored { app: &'a [u8], n: usize, replaced: bool },
    /// A publish named a permission the authority does not know — the whole app is refused
    /// (and any earlier entry removed: its grants are unknown now).
    Rejected { app: &'a [u8], cap: &'a [u8] },
    /// The sender lacks the capability the op needs.
    Denied { op: u8 },
    /// The frame did not decode.
    Malformed { op: u8 },
    /// The table is full (a new app cannot be stored).
    Full { app: &'a [u8] },
    /// A read: `found` tells whether the app was ever published.
    Read { app: &'a [u8], found: bool },
}

/// Handles an app-grants frame (`None`: not one — the caller's other handlers decide).
/// `out` receives the answer; the returned length is what to send.
pub fn handle<'f>(
    policy: &Policy<'_>,
    table: &mut AppGrants,
    frame: &'f [u8],
    sender_service_id: u64,
    out: &mut [u8],
) -> Option<(usize, Outcome<'f>)> {
    let op = *frame.get(3)?;
    if frame.len() < 4
        || frame[2] != wire::VERSION_V2
        || !matches!(op, wire::OP_APP_GRANTS_PUT | wire::OP_APP_GRANTS_GET)
    {
        return None;
    }
    // The nonce rides at 4..8 of every v2 frame; a frame too short for one answers nonce 0.
    let nonce = frame.get(4..8).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0);
    let (len, outcome) = match op {
        wire::OP_APP_GRANTS_PUT => {
            if !policy.allows(sender_service_id, CAP_PUBLISH) {
                (rsp_v2(op, nonce, wire::STATUS_DENY, out)?, Outcome::Denied { op })
            } else {
                match wire::decode_app_grants_put(frame) {
                    None => {
                        (rsp_v2(op, nonce, wire::STATUS_MALFORMED, out)?, Outcome::Malformed { op })
                    }
                    Some((nonce, app, list)) => publish(table, nonce, app, list, out)?,
                }
            }
        }
        _ => {
            if !policy.allows(sender_service_id, CAP_READ) {
                (
                    wire::encode_app_grants_rsp(nonce, wire::STATUS_DENY, &[], out)?,
                    Outcome::Denied { op },
                )
            } else {
                match wire::decode_app_grants_get(frame) {
                    None => (
                        wire::encode_app_grants_rsp(nonce, wire::STATUS_MALFORMED, &[], out)?,
                        Outcome::Malformed { op },
                    ),
                    Some((nonce, app)) => read(table, nonce, app, out)?,
                }
            }
        }
    };
    Some((len, outcome))
}

fn rsp_v2(op: u8, nonce: wire::Nonce, status: u8, out: &mut [u8]) -> Option<usize> {
    let rsp = wire::encode_rsp_v2(op, nonce, status);
    out.get_mut(..rsp.len())?.copy_from_slice(&rsp);
    Some(rsp.len())
}

fn publish<'f>(
    table: &mut AppGrants,
    nonce: wire::Nonce,
    app: &'f [u8],
    list: wire::GrantList<'f>,
    out: &mut [u8],
) -> Option<(usize, Outcome<'f>)> {
    let op = wire::OP_APP_GRANTS_PUT;
    let Ok(app_str) = core::str::from_utf8(app) else {
        return Some((rsp_v2(op, nonce, wire::STATUS_MALFORMED, out)?, Outcome::Malformed { op }));
    };
    let mut grants = Vec::with_capacity(list.len());
    for cap in list {
        match core::str::from_utf8(cap) {
            Ok(s) if is_known_permission(s) => grants.push(String::from(s)),
            _ => {
                // Fail-closed: the app's grants are unknown now — an earlier entry goes too.
                table.remove(app_str);
                let len = rsp_v2(op, nonce, wire::STATUS_DENY, out)?;
                return Some((len, Outcome::Rejected { app, cap }));
            }
        }
    }
    let n = grants.len();
    match table.put(app_str, grants) {
        Ok(replaced) => Some((
            rsp_v2(op, nonce, wire::STATUS_ALLOW, out)?,
            Outcome::Stored { app, n, replaced },
        )),
        Err(()) => Some((rsp_v2(op, nonce, wire::STATUS_UNSUPPORTED, out)?, Outcome::Full { app })),
    }
}

fn read<'f>(
    table: &AppGrants,
    nonce: wire::Nonce,
    app: &'f [u8],
    out: &mut [u8],
) -> Option<(usize, Outcome<'f>)> {
    let found = core::str::from_utf8(app).ok().and_then(|a| table.grants_of(a));
    let len = match found {
        Some(grants) => {
            let mut refs: [&[u8]; wire::MAX_APP_GRANTS] = [&[]; wire::MAX_APP_GRANTS];
            let n = grants.len().min(wire::MAX_APP_GRANTS);
            for (slot, g) in refs.iter_mut().zip(grants.iter()) {
                *slot = g.as_bytes();
            }
            wire::encode_app_grants_rsp(nonce, wire::STATUS_ALLOW, &refs[..n], out)?
        }
        None => wire::encode_app_grants_rsp(nonce, wire::STATUS_DENY, &[], out)?,
    };
    Some((len, Outcome::Read { app, found: found.is_some() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_sel::PolicyEntry;

    fn sid(name: &str) -> u64 {
        nexus_abi::service_id_from_name(name.as_bytes())
    }

    fn policy() -> Policy<'static> {
        // The ids are computed at run time; leak one small table per test (host only).
        let entries: &'static [PolicyEntry] = alloc::boxed::Box::leak(alloc::boxed::Box::new([
            PolicyEntry { service_id: sid("bundlemgrd"), capabilities: &[CAP_PUBLISH] },
            PolicyEntry { service_id: sid("abilitymgr"), capabilities: &[CAP_READ] },
            PolicyEntry { service_id: sid("windowd"), capabilities: &["ipc.core"] },
        ]));
        Policy::new(entries)
    }

    fn put_frame(app: &str, grants: &[&str]) -> Vec<u8> {
        let refs: Vec<&[u8]> = grants.iter().map(|g| g.as_bytes()).collect();
        let mut buf = [0u8; 512];
        let n = wire::encode_app_grants_put(0x51, app.as_bytes(), &refs, &mut buf).unwrap();
        buf[..n].to_vec()
    }

    fn get_frame(app: &str) -> Vec<u8> {
        let mut buf = [0u8; 64];
        let n = wire::encode_app_grants_get(0x52, app.as_bytes(), &mut buf).unwrap();
        buf[..n].to_vec()
    }

    fn run<'f>(
        table: &mut AppGrants,
        frame: &'f [u8],
        sender: &str,
        out: &mut [u8; 512],
    ) -> (usize, Outcome<'f>) {
        handle(&policy(), table, frame, sid(sender), out).expect("an app-grants frame")
    }

    #[test]
    fn publish_then_read_round_trip() {
        let mut table = AppGrants::new();
        let mut out = [0u8; 512];
        let put = put_frame("notes", &["nexus.permission.WINDOW", BACKGROUND]);
        let (n, outcome) = run(&mut table, &put, "bundlemgrd", &mut out);
        assert_eq!(outcome, Outcome::Stored { app: b"notes", n: 2, replaced: false });
        let (op, nonce, status) = wire::decode_rsp_v2(&out[..n]).unwrap();
        assert_eq!((op, nonce, status), (wire::OP_APP_GRANTS_PUT, 0x51, wire::STATUS_ALLOW));

        let get = get_frame("notes");
        let (n, outcome) = run(&mut table, &get, "abilitymgr", &mut out);
        assert_eq!(outcome, Outcome::Read { app: b"notes", found: true });
        let (nonce, status, list) = wire::decode_app_grants_rsp(&out[..n]).unwrap();
        assert_eq!((nonce, status, list.len()), (0x52, wire::STATUS_ALLOW, 2));
        let got: Vec<&[u8]> = list.collect();
        assert_eq!(got, [&b"nexus.permission.WINDOW"[..], BACKGROUND.as_bytes()]);

        // A second publish REPLACES (an upgrade dropped a permission).
        let put = put_frame("notes", &["nexus.permission.WINDOW"]);
        let (_, outcome) = run(&mut table, &put, "bundlemgrd", &mut out);
        assert_eq!(outcome, Outcome::Stored { app: b"notes", n: 1, replaced: true });
        assert_eq!(table.grants_of("notes").unwrap().len(), 1);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn test_reject_app_grants_from_a_stranger() {
        // windowd holds `ipc.core`, not `app.grants.publish`: DENY, and nothing is stored.
        let mut table = AppGrants::new();
        let mut out = [0u8; 512];
        let put = put_frame("notes", &["nexus.permission.WINDOW"]);
        let (n, outcome) = run(&mut table, &put, "windowd", &mut out);
        assert_eq!(outcome, Outcome::Denied { op: wire::OP_APP_GRANTS_PUT });
        assert_eq!(wire::decode_rsp_v2(&out[..n]).unwrap().2, wire::STATUS_DENY);
        assert!(table.is_empty());
        // Nor may a reader publish.
        let (_, outcome) = run(&mut table, &put, "abilitymgr", &mut out);
        assert_eq!(outcome, Outcome::Denied { op: wire::OP_APP_GRANTS_PUT });
        // An unknown sender neither.
        let (_, outcome) = run(&mut table, &put, "demo.testsvc", &mut out);
        assert_eq!(outcome, Outcome::Denied { op: wire::OP_APP_GRANTS_PUT });
        assert!(table.is_empty());
    }

    #[test]
    fn test_reject_app_grants_read_without_cap() {
        let mut table = AppGrants::new();
        let mut out = [0u8; 512];
        let put = put_frame("notes", &["nexus.permission.WINDOW"]);
        run(&mut table, &put, "bundlemgrd", &mut out);
        // The publisher itself may not read; a stranger neither: DENY carries no grants.
        for who in ["bundlemgrd", "windowd", "demo.testsvc"] {
            let get = get_frame("notes");
            let (n, outcome) = run(&mut table, &get, who, &mut out);
            assert_eq!(outcome, Outcome::Denied { op: wire::OP_APP_GRANTS_GET }, "{who}");
            let (_, status, list) = wire::decode_app_grants_rsp(&out[..n]).unwrap();
            assert_eq!((status, list.len()), (wire::STATUS_DENY, 0));
        }
    }

    #[test]
    fn test_reject_unknown_permission_rejects_the_app() {
        let mut table = AppGrants::new();
        let mut out = [0u8; 512];
        run(&mut table, &put_frame("notes", &["nexus.permission.WINDOW"]), "bundlemgrd", &mut out);
        // A typo in the new manifest: the publish is refused AND the earlier entry is gone —
        // the app's grants are unknown now, so a reader gets DENY, not yesterday's set.
        let put = put_frame("notes", &["nexus.permission.WINDOW", "nexus.permission.BOGUS"]);
        let (n, outcome) = run(&mut table, &put, "bundlemgrd", &mut out);
        assert_eq!(outcome, Outcome::Rejected { app: b"notes", cap: b"nexus.permission.BOGUS" });
        assert_eq!(wire::decode_rsp_v2(&out[..n]).unwrap().2, wire::STATUS_DENY);
        assert!(table.grants_of("notes").is_none());
        let get = get_frame("notes");
        let (n, outcome) = run(&mut table, &get, "abilitymgr", &mut out);
        assert_eq!(outcome, Outcome::Read { app: b"notes", found: false });
        assert_eq!(wire::decode_app_grants_rsp(&out[..n]).unwrap().1, wire::STATUS_DENY);
        // A foreign app namespace is unknown too (`app.ghost.SEND` is nobody's to grant
        // unless well formed — the form is accepted, the exporter check is the mediation's).
        let bad = put_frame("x", &["app.x"]);
        let (_, outcome) = run(&mut table, &bad, "bundlemgrd", &mut out);
        assert_eq!(outcome, Outcome::Rejected { app: b"x", cap: b"app.x" });
        let good = put_frame("x", &["app.chat.SEND"]);
        let (_, outcome) = run(&mut table, &good, "bundlemgrd", &mut out);
        assert_eq!(outcome, Outcome::Stored { app: b"x", n: 1, replaced: false });
    }

    /// RFC-0113 §8: the `stream` input class is declared, not grantable — nxb-pack refuses it at
    /// pack time and the authority does not know it, so a bundle that names it anyway never
    /// publishes.
    #[test]
    fn test_reject_stream_class_at_publish() {
        let mut table = AppGrants::new();
        let mut out = [0u8; 512];
        let put =
            put_frame("painter", &["nexus.permission.WINDOW", "nexus.permission.INPUT_STREAM"]);
        let (_, outcome) = run(&mut table, &put, "bundlemgrd", &mut out);
        let cap = b"nexus.permission.INPUT_STREAM";
        assert_eq!(outcome, Outcome::Rejected { app: b"painter", cap });
        assert!(table.grants_of("painter").is_none());
    }

    #[test]
    fn unknown_app_reads_deny_and_a_published_app_without_grants_reads_allow_empty() {
        let mut table = AppGrants::new();
        let mut out = [0u8; 512];
        let ghost = get_frame("ghost");
        let (n, outcome) = run(&mut table, &ghost, "abilitymgr", &mut out);
        assert_eq!(outcome, Outcome::Read { app: b"ghost", found: false });
        assert_eq!(wire::decode_app_grants_rsp(&out[..n]).unwrap().1, wire::STATUS_DENY);
        run(&mut table, &put_frame("plain", &[]), "bundlemgrd", &mut out);
        let (n, _) = run(&mut table, &get_frame("plain"), "abilitymgr", &mut out);
        let (_, status, list) = wire::decode_app_grants_rsp(&out[..n]).unwrap();
        assert_eq!((status, list.len()), (wire::STATUS_ALLOW, 0));
    }

    #[test]
    fn malformed_and_foreign_frames() {
        let mut table = AppGrants::new();
        let mut out = [0u8; 512];
        // Not an app-grants frame: the other handlers decide.
        let route = [b'P', b'O', 2, wire::OP_ROUTE, 1, 0, 0, 0, 1, b'a', 1, b'b'];
        assert!(handle(&policy(), &mut table, &route, sid("bundlemgrd"), &mut out).is_none());
        assert!(handle(&policy(), &mut table, &[], sid("bundlemgrd"), &mut out).is_none());
        // Ours, but truncated: MALFORMED with the nonce echoed.
        let put = put_frame("notes", &["nexus.permission.WINDOW"]);
        let (n, outcome) = run(&mut table, &put[..put.len() - 3], "bundlemgrd", &mut out);
        assert_eq!(outcome, Outcome::Malformed { op: wire::OP_APP_GRANTS_PUT });
        assert_eq!(wire::decode_rsp_v2(&out[..n]).unwrap(), (wire::OP_APP_GRANTS_PUT, 0x51, 2));
        let get = get_frame("notes");
        let (n, outcome) = run(&mut table, &get[..6], "abilitymgr", &mut out);
        assert_eq!(outcome, Outcome::Malformed { op: wire::OP_APP_GRANTS_GET });
        assert_eq!(wire::decode_app_grants_rsp(&out[..n]).unwrap().1, wire::STATUS_MALFORMED);
        assert!(table.is_empty());
    }

    #[test]
    fn the_table_is_bounded() {
        let mut table = AppGrants::new();
        let mut out = [0u8; 512];
        for i in 0..MAX_APPS {
            let put = put_frame(&alloc::format!("app{i}"), &[]);
            let (_, outcome) = run(&mut table, &put, "bundlemgrd", &mut out);
            assert!(matches!(outcome, Outcome::Stored { .. }));
        }
        let more = put_frame("one-more", &[]);
        let (n, outcome) = run(&mut table, &more, "bundlemgrd", &mut out);
        assert_eq!(outcome, Outcome::Full { app: b"one-more" });
        assert_eq!(wire::decode_rsp_v2(&out[..n]).unwrap().2, wire::STATUS_UNSUPPORTED);
        // A known app is still replaceable when the table is full.
        let again = put_frame("app0", &[]);
        let (_, outcome) = run(&mut table, &again, "bundlemgrd", &mut out);
        assert_eq!(outcome, Outcome::Stored { app: b"app0", n: 0, replaced: true });
    }

    #[test]
    fn app_owned_namespace_syntax() {
        assert!(is_known_permission("app.chat.SEND"));
        assert!(is_known_permission("app.my-app.READ_ALL"));
        assert!(!is_known_permission("app.chat"));
        assert!(!is_known_permission("app..SEND"));
        assert!(!is_known_permission("app.chat."));
        assert!(!is_known_permission("app.ch at.SEND"));
        assert!(!is_known_permission("nexus.permission.BOGUS"));
        assert!(is_known_permission(BACKGROUND));
    }

    /// Guard: every routable permission in the SDK routing SSOT must be one this authority
    /// recognizes — otherwise the launcher would wire a route for a grant the publish rejects.
    #[test]
    fn every_sdk_route_permission_is_known() {
        for r in nexus_sdk_routes::SERVICE_ROUTES {
            assert!(
                is_known_permission(r.permission),
                "SDK route `svc.{}` needs permission `{}` which is not in KNOWN_PERMISSIONS",
                r.svc,
                r.permission
            );
        }
    }
}
