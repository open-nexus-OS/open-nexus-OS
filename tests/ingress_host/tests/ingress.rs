// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0092 Layer B host proofs (TASK-0052 P2): the allow path end
//! to end (intent → accept → bytes cross the relay → status) and the five
//! `test_reject_*` the contract names, all through the wire dispatcher so
//! the OS loop (P3) inherits proven verdicts. Plus the IDL↔grammar↔wire
//! consistency pins and the shipped-policy table check.
//! OWNERS: @runtime @security
//! STATUS: Experimental

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::fs;
use std::path::Path;

use ingress_host::{sid, table_from_toml, ScriptedPolicy};
use ingressd::dispatch::{handle_frame, Event};
use ingressd::forward::{IoError, Link, Relay, Stream};
use ingressd::intent::Registry;
use ingressd::table::{lookup, Proto};
use ingressd::wire::{
    decode_reply, encode_request, Reason, OP_EXPOSE, OP_EXPOSE_STATUS, OP_UNEXPOSE, STATUS_ALLOW,
    STATUS_DENY, STATUS_MALFORMED, STATUS_REPLY_LEN, STATUS_UNSUPPORTED,
};

const POLICY: &str = r#"
[[expose."demo.web"]]
port = 8080
proto = "tcp"
cidr_allow = ["10.0.2.0/24"]
rate_per_s = 2
burst = 3
backend = 18080

[[expose."demo.dns"]]
port = 5353
proto = "udp"
cidr_allow = ["0.0.0.0/0"]
rate_per_s = 100
burst = 10
backend = 15353
"#;

fn request(op: u8, nonce: u32, port: u16, proto: Proto) -> Vec<u8> {
    let mut buf = [0u8; 16];
    let n = encode_request(op, nonce, port, proto, &mut buf).unwrap();
    buf[..n].to_vec()
}

/// One dispatcher turn; returns (status, reason, event).
fn turn(
    reg: &mut Registry<'_>,
    host: &mut ScriptedPolicy,
    sender: u64,
    frame: &[u8],
) -> (u8, Reason, Event) {
    let mut out = [0u8; STATUS_REPLY_LEN];
    let outcome = handle_frame(reg, host, sender, frame, &mut out);
    let reply = decode_reply(&out[..outcome.reply_len]).expect("well-formed reply");
    (reply.status, reply.reason, outcome.event)
}

/// In-memory non-blocking stream: `rx` is what the peer sent us, `tx` what
/// we sent the peer.
#[derive(Default)]
struct Pipe {
    rx: VecDeque<u8>,
    tx: Vec<u8>,
    eof: bool,
}

impl Stream for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if self.rx.is_empty() {
            return if self.eof { Ok(0) } else { Err(IoError::WouldBlock) };
        }
        let n = buf.len().min(self.rx.len());
        for b in buf.iter_mut().take(n) {
            *b = self.rx.pop_front().unwrap();
        }
        Ok(n)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        self.tx.extend_from_slice(buf);
        Ok(buf.len())
    }
}

#[test]
fn expose_allow_end_to_end() {
    let table = table_from_toml(POLICY).unwrap();
    let (wi, _) = lookup(table, 8080, Proto::Tcp).unwrap();
    let mut reg: Registry<'_> = Registry::new(table);
    let mut host = ScriptedPolicy::granting(&["demo.web", "demo.dns"]);
    let web = sid("demo.web");

    // Intent by the declared subject, capability granted ⇒ open.
    let (status, reason, event) =
        turn(&mut reg, &mut host, web, &request(OP_EXPOSE, 1, 8080, Proto::Tcp));
    assert_eq!((status, reason), (STATUS_ALLOW, Reason::None));
    assert_eq!(event, Event::Opened { idx: wi, port: 8080, proto: Proto::Tcp });
    assert!(reg.is_open(wi));
    assert_eq!(host.queries, vec![web], "policyd asked exactly once for the declared subject");

    // A peer inside the allow-list is admitted and forwarded: bytes cross
    // the gateway both ways.
    reg.admit_peer(wi, [10, 0, 2, 2], 1_000).unwrap();
    let mut peer =
        Pipe { rx: b"GET / HTTP/1.0\r\n\r\n".iter().copied().collect(), ..Default::default() };
    peer.eof = true;
    let mut backend =
        Pipe { rx: b"HTTP/1.0 200 OK\r\n".iter().copied().collect(), ..Default::default() };
    backend.eof = true;
    let mut relay: Relay<Pipe, Pipe> = Relay::new();
    let i = relay.attach(Link::new(peer, backend)).unwrap();
    let mut turns = 0;
    while relay.pump_all(8) > 0 {
        turns += 1;
        assert!(turns < 64);
    }
    assert!(relay.link(i).is_none(), "drained link is released");

    // Status is authority observability: counters reflect the admit.
    let mut out = [0u8; STATUS_REPLY_LEN];
    let outcome = handle_frame(
        &mut reg,
        &mut host,
        web,
        &request(OP_EXPOSE_STATUS, 2, 8080, Proto::Tcp),
        &mut out,
    );
    assert_eq!(outcome.event, Event::Status);
    let reply = decode_reply(&out[..outcome.reply_len]).unwrap();
    assert!(reply.open);
    assert_eq!((reply.counters.accepted, reply.counters.denied_cidr), (1, 0));

    // Re-open by the owner is idempotent; the owner may close.
    let (status, _, _) = turn(&mut reg, &mut host, web, &request(OP_EXPOSE, 3, 8080, Proto::Tcp));
    assert_eq!(status, STATUS_ALLOW);
    let (status, _, event) =
        turn(&mut reg, &mut host, web, &request(OP_UNEXPOSE, 4, 8080, Proto::Tcp));
    assert_eq!(status, STATUS_ALLOW);
    assert_eq!(event, Event::Closed { idx: wi, port: 8080, proto: Proto::Tcp });
    assert!(!reg.is_open(wi));
    assert_eq!(reg.admit_peer(wi, [10, 0, 2, 2], 2_000), Err(Reason::Policy));
}

#[test]
fn test_reject_intent_policy_denied() {
    let table = table_from_toml(POLICY).unwrap();
    let (wi, _) = lookup(table, 8080, Proto::Tcp).unwrap();
    let web = sid("demo.web");

    // (a) Declared, but policyd refuses `net.expose`.
    let mut reg: Registry<'_> = Registry::new(table);
    let mut host = ScriptedPolicy::granting(&["demo.dns"]);
    let (status, reason, event) =
        turn(&mut reg, &mut host, web, &request(OP_EXPOSE, 1, 8080, Proto::Tcp));
    assert_eq!((status, reason), (STATUS_DENY, Reason::Policy));
    assert_eq!(event, Event::Denied { reason: Reason::Policy });
    assert!(!reg.is_open(wi));

    // (b) Undeclared (port, proto): refused before policyd is asked.
    let mut host = ScriptedPolicy::granting(&["demo.web"]);
    let (status, reason, _) =
        turn(&mut reg, &mut host, web, &request(OP_EXPOSE, 2, 8081, Proto::Tcp));
    assert_eq!((status, reason), (STATUS_DENY, Reason::Policy));
    let (status, reason, _) =
        turn(&mut reg, &mut host, web, &request(OP_EXPOSE, 3, 8080, Proto::Udp));
    assert_eq!((status, reason), (STATUS_DENY, Reason::Policy));
    assert!(host.queries.is_empty(), "no authority round-trip for an undeclared exposure");

    // (c) Authority unreachable ⇒ fail closed.
    let mut host = ScriptedPolicy::unreachable();
    let (status, reason, _) =
        turn(&mut reg, &mut host, web, &request(OP_EXPOSE, 4, 8080, Proto::Tcp));
    assert_eq!((status, reason), (STATUS_DENY, Reason::Policy));
    assert_eq!(reg.open_count(), 0);

    // (d) Status without the capability is refused too.
    let (status, reason, _) =
        turn(&mut reg, &mut host, web, &request(OP_EXPOSE_STATUS, 5, 8080, Proto::Tcp));
    assert_eq!((status, reason), (STATUS_DENY, Reason::Policy));
}

#[test]
fn test_reject_forged_intent_sender() {
    let table = table_from_toml(POLICY).unwrap();
    let (wi, _) = lookup(table, 8080, Proto::Tcp).unwrap();
    let mut reg: Registry<'_> = Registry::new(table);
    // Even a subject that holds `net.expose` cannot claim another's exposure.
    let mut host = ScriptedPolicy::granting(&["demo.web", "demo.dns", "intruder"]);
    let intruder = sid("intruder");
    let (status, reason, event) =
        turn(&mut reg, &mut host, intruder, &request(OP_EXPOSE, 1, 8080, Proto::Tcp));
    assert_eq!((status, reason), (STATUS_DENY, Reason::Identity));
    assert_eq!(event, Event::Denied { reason: Reason::Identity });
    assert!(host.queries.is_empty(), "identity is checked before the authority is asked");

    // The owner opens; a forged unexpose from another subject is refused
    // and the exposure stays open.
    let web = sid("demo.web");
    let (status, _, _) = turn(&mut reg, &mut host, web, &request(OP_EXPOSE, 2, 8080, Proto::Tcp));
    assert_eq!(status, STATUS_ALLOW);
    let dns = sid("demo.dns");
    let (status, reason, _) =
        turn(&mut reg, &mut host, dns, &request(OP_UNEXPOSE, 3, 8080, Proto::Tcp));
    assert_eq!((status, reason), (STATUS_DENY, Reason::Identity));
    assert!(reg.is_open(wi));
}

#[test]
fn test_reject_cidr() {
    let table = table_from_toml(POLICY).unwrap();
    let (wi, _) = lookup(table, 8080, Proto::Tcp).unwrap();
    let (di, _) = lookup(table, 5353, Proto::Udp).unwrap();
    let mut reg: Registry<'_> = Registry::new(table);
    let mut host = ScriptedPolicy::granting(&["demo.web", "demo.dns"]);
    reg.open(&mut host, sid("demo.web"), 8080, Proto::Tcp).unwrap();
    reg.open(&mut host, sid("demo.dns"), 5353, Proto::Udp).unwrap();

    // Outside 10.0.2.0/24 ⇒ closed + counted; inside ⇒ admitted.
    assert_eq!(reg.admit_peer(wi, [10, 0, 3, 2], 0), Err(Reason::Cidr));
    assert_eq!(reg.admit_peer(wi, [203, 0, 113, 9], 0), Err(Reason::Cidr));
    assert_eq!(reg.admit_peer(wi, [10, 0, 2, 200], 0), Ok(()));
    let c = reg.counters(wi);
    assert_eq!((c.accepted, c.denied_cidr, c.denied_rate), (1, 2, 0));
    // "0.0.0.0/0" was written explicitly: everyone is admitted there.
    assert_eq!(reg.admit_peer(di, [203, 0, 113, 9], 0), Ok(()));
    // The grammar never yields an empty allow-list: "everyone" must be explicit.
    let bad = POLICY.replace("cidr_allow = [\"10.0.2.0/24\"]", "cidr_allow = []");
    assert!(table_from_toml(&bad).unwrap_err().contains("no cidr_allow"));
}

#[test]
fn test_reject_rate_exceeded() {
    let table = table_from_toml(POLICY).unwrap();
    let (wi, _) = lookup(table, 8080, Proto::Tcp).unwrap();
    let mut host = ScriptedPolicy::granting(&["demo.web"]);
    let run = |reg: &mut Registry<'_>| -> Vec<Result<(), Reason>> {
        // burst 3 at 2/s: three admits, the fourth is refused; 500 ms later
        // one token is back; 400 ms is not enough.
        let peer = [10, 0, 2, 2];
        vec![
            reg.admit_peer(wi, peer, 0),
            reg.admit_peer(wi, peer, 0),
            reg.admit_peer(wi, peer, 0),
            reg.admit_peer(wi, peer, 0),
            reg.admit_peer(wi, peer, 400_000_000),
            reg.admit_peer(wi, peer, 500_000_000),
            reg.admit_peer(wi, peer, 500_000_000),
        ]
    };
    let mut reg: Registry<'_> = Registry::new(table);
    reg.open(&mut host, sid("demo.web"), 8080, Proto::Tcp).unwrap();
    let first = run(&mut reg);
    assert_eq!(
        first,
        vec![
            Ok(()),
            Ok(()),
            Ok(()),
            Err(Reason::Rate),
            Err(Reason::Rate),
            Ok(()),
            Err(Reason::Rate)
        ]
    );
    let c = reg.counters(wi);
    assert_eq!((c.accepted, c.denied_cidr, c.denied_rate), (4, 0, 3));
    // Deterministic: the same timestamps replay to the same verdicts.
    let mut again: Registry<'_> = Registry::new(table);
    again.open(&mut host, sid("demo.web"), 8080, Proto::Tcp).unwrap();
    assert_eq!(run(&mut again), first);
}

#[test]
fn test_reject_malformed_and_unsupported_frames() {
    let table = table_from_toml(POLICY).unwrap();
    let mut reg: Registry<'_> = Registry::new(table);
    let mut host = ScriptedPolicy::granting(&["demo.web"]);
    let web = sid("demo.web");
    let mut out = [0u8; STATUS_REPLY_LEN];
    let o = handle_frame(&mut reg, &mut host, web, b"IG\x01\x01short", &mut out);
    assert_eq!(o.event, Event::Malformed);
    assert_eq!(decode_reply(&out[..o.reply_len]).unwrap().status, STATUS_MALFORMED);
    let mut unk = request(OP_EXPOSE, 9, 8080, Proto::Tcp);
    unk[3] = 0x7f;
    let o = handle_frame(&mut reg, &mut host, web, &unk, &mut out);
    assert_eq!(o.event, Event::Unsupported);
    let r = decode_reply(&out[..o.reply_len]).unwrap();
    assert_eq!((r.status, r.nonce), (STATUS_UNSUPPORTED, 9));
    // A reply buffer too small for the frame sends nothing (never truncated).
    let mut tiny = [0u8; 4];
    let o =
        handle_frame(&mut reg, &mut host, web, &request(OP_EXPOSE, 1, 8080, Proto::Tcp), &mut tiny);
    assert_eq!(o.reply_len, 0);
    // The verdict itself was still taken (the authority was asked once);
    // only the reply is withheld.
    assert_eq!(host.queries.len(), 1);
}

#[test]
fn idl_grammar_and_wire_agree() {
    let idl = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/nexus-idl/schemas/ingress.capnp"),
    )
    .unwrap();
    // Every grammar field has its IDL twin.
    for field in ["service", "port", "proto", "cidrAllow", "ratePerS", "burst", "tls", "backend"] {
        assert!(idl.contains(&format!("{field} @")), "IDL lacks {field}");
    }
    // Proto ordinals are the wire bytes; deny reasons share the ordinals.
    assert!(idl.contains("tcp @0") && idl.contains("udp @1"));
    assert_eq!((Proto::Tcp as u8, Proto::Udp as u8), (0, 1));
    for (name, reason) in [
        ("policy", Reason::Policy),
        ("identity", Reason::Identity),
        ("limit", Reason::Limit),
        ("tls", Reason::Tls),
    ] {
        assert!(idl.contains(&format!("{name} @{}", reason as u8)), "IDL reason {name}");
    }
    // The TLS slot exists in the IDL and is refused by the grammar.
    assert!(idl.contains("mtls @2"));
    let tls = POLICY.replace("backend = 18080", "tls = \"mtls\"\nbackend = 18080");
    assert!(table_from_toml(&tls).unwrap_err().contains("contract slot"));
}

#[test]
fn shipped_policy_table_is_consistent() {
    // ingressd's build-time table equals what the host loader compiles from
    // the same policy root (currently no exposure is declared — P3 adds the
    // selftest's; this pin catches the drift either way).
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../policies");
    let tree = nexus_policy::PolicyTree::load_root(&root).unwrap();
    let built = ingressd::table::generated::EXPOSE_ENTRIES;
    assert_eq!(built.len(), tree.policy().expose_count());
    for (subject, list) in tree.policy().exposes() {
        for e in list {
            let proto = match e.proto {
                nexus_policy::expose::Proto::Tcp => Proto::Tcp,
                nexus_policy::expose::Proto::Udp => Proto::Udp,
            };
            let entry =
                built.iter().find(|b| b.port == e.port && b.proto == proto).unwrap_or_else(|| {
                    panic!("{subject}: {}/{} missing from table", e.proto.label(), e.port)
                });
            assert_eq!(entry.subject_id, sid(subject));
            assert_eq!(
                (entry.backend, entry.rate_per_s, entry.burst),
                (e.backend, e.rate_per_s, e.burst)
            );
            assert_eq!(entry.cidr_allow.len(), e.cidr_allow.len());
        }
    }
    // A duplicate (port, proto) across subjects never compiles into a table.
    let dup = format!("{POLICY}\n[[expose.\"demo.other\"]]\nport = 8080\nproto = \"tcp\"\ncidr_allow = [\"10.0.2.0/24\"]\nrate_per_s = 1\nburst = 1\nbackend = 1\n");
    assert!(table_from_toml(&dup).unwrap_err().contains("exactly one owner"));
}
