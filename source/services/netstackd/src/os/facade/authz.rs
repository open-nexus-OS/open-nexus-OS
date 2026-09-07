// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §7 network seams (TASK-0043 P2 / TASK-0052 P1 shared
//! identity model): every `connect`, `listen` and `udp bind` is decided by
//! policyd (`OP_ABI_EVAL`) for the kernel-attributed sender — netstackd
//! holds no policy of its own. `net.bind` carries the address class:
//! `Loopback` = 127/8 or the facade's loopback emulation (the QEMU user-net
//! fallback IP / 0.0.0.0 on the loopback port set — traffic that never
//! reaches the NIC), `Any` = everything else. Refusal ⇒ `STATUS_DENY` +
//! `!cap-deny: enforcer=netstackd …` line; policyd unreachable or an
//! unattributed sender ⇒ refusal (fail closed, `nexus_ipc::policyd::
//! seam_admits`). Subjects without an authored profile are not governed yet
//! (`STATUS_UNSUPPORTED` ⇒ admitted) — the same rule as statefsd's seam.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: decision in nexus-ipc (`test_reject_unattributed_connect`),
//!   policyd matcher tests; QEMU markers land with TASK-0043 P3 / 0052 P1
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use nexus_abi::policyd::{ABI_CLASS_NET_BIND, ABI_CLASS_NET_CONNECT};
use nexus_ipc::policyd::{abi_eval_on, seam_admits, PolicySlots};

use crate::os::config::{
    LOOPBACK_PORT, LOOPBACK_PORT_B, LOOPBACK_UDP_PORT, LOOPBACK_UDP_QUIC_CLIENT_PORT,
};
use crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP;
use crate::os::facade::dispatch::FacadeContext;

/// Wire address classes of `OP_ABI_EVAL` (`net.bind`).
const ADDR_LOOPBACK: u8 = 0;
const ADDR_ANY: u8 = 1;

/// Admitted tuples remembered per boot (dsoftbusd's connect retry storm
/// would otherwise cost one policyd roundtrip per attempt).
pub(crate) const ADMIT_CACHE_ENTRIES: usize = 16;

/// One admitted evaluation tuple.
#[derive(Clone, Copy, PartialEq, Eq)]
struct AdmitKey {
    sender: u64,
    class: u8,
    addr_class: u8,
    port: u16,
    addr: [u8; 4],
}

/// Bounded ring of admitted tuples (refusals are never cached).
pub(crate) struct AdmitCache {
    keys: [Option<AdmitKey>; ADMIT_CACHE_ENTRIES],
    next: usize,
}

impl AdmitCache {
    pub(crate) const fn new() -> Self {
        Self { keys: [None; ADMIT_CACHE_ENTRIES], next: 0 }
    }
    fn contains(&self, key: &AdmitKey) -> bool {
        self.keys.iter().any(|k| k.as_ref() == Some(key))
    }
    fn insert(&mut self, key: AdmitKey) {
        self.keys[self.next % ADMIT_CACHE_ENTRIES] = Some(key);
        self.next = self.next.wrapping_add(1);
    }
}

/// policyd's request endpoint + this service's `@reply` pair, wired by init
/// at fixed slots (like statefsd's 7/6/5): 7 = request SEND, 8 = reply RECV,
/// 9 = reply SEND. An enforcement seam never routes dynamically from its hot
/// loop (a routed lookup from the facade start wedged under icount).
const POLICY_SLOTS: PolicySlots = PolicySlots { send: 0x07, reply_send: 0x09, reply_recv: 0x08 };

/// Arms the seam with the init-wired slots. The status line is written raw
/// (never folded) — it is the boot-proof witness that `net.connect` /
/// `net.bind` are enforced from here on (`net-egress: enforced`, RFC-0091).
pub(crate) fn resolve(ctx_policy: &mut Option<PolicySlots>) {
    *ctx_policy = Some(POLICY_SLOTS);
    raw_line(b"net-egress: enforced (netstackd policy seam on)\n");
}

/// Raw, never-folded UART line from a stack copy (the kernel's user-slice
/// check is the one every `debug_println` satisfies).
fn raw_line(line: &[u8]) {
    let mut buf = [0u8; 80];
    let n = line.len().min(buf.len());
    buf[..n].copy_from_slice(&line[..n]);
    let _ = nexus_abi::debug_write(&buf[..n]);
}

/// `Loopback` when the bind never reaches the NIC.
pub(crate) fn bind_addr_class(ip: [u8; 4], port: u16) -> u8 {
    let loop_port = matches!(
        port,
        LOOPBACK_PORT | LOOPBACK_PORT_B | LOOPBACK_UDP_PORT | LOOPBACK_UDP_QUIC_CLIENT_PORT
    );
    if ip[0] == 127 || (loop_port && (ip == QEMU_USERNET_FALLBACK_IP || ip == [0, 0, 0, 0])) {
        ADDR_LOOPBACK
    } else {
        ADDR_ANY
    }
}

/// The seam's inputs, copied out of the context before a handler takes its
/// mutable borrows of `net`/`state`.
#[derive(Clone, Copy)]
pub(crate) struct Seam {
    policy: Option<PolicySlots>,
    sender: u64,
}

impl Seam {
    pub(crate) fn of(ctx: &FacadeContext<'_>) -> Self {
        Self { policy: ctx.state.policy, sender: ctx.sender_service_id }
    }

    fn evaluate(
        self,
        cache: &mut AdmitCache,
        class: u8,
        addr_class: u8,
        port: u16,
        addr: [u8; 4],
    ) -> bool {
        let key = AdmitKey { sender: self.sender, class, addr_class, port, addr };
        if self.sender != 0 && cache.contains(&key) {
            return true;
        }
        let status = self.policy.and_then(|slots| {
            abi_eval_on(
                slots.send,
                slots.reply_send,
                slots.reply_recv,
                self.sender,
                class,
                addr_class,
                port,
                u32::from_be_bytes(addr),
                0,
                0,
                b"",
            )
        });
        let admitted = seam_admits(self.sender, status);
        if admitted {
            cache.insert(key);
        } else {
            emit_deny(self.sender, class, port, addr_class);
        }
        admitted
    }

    /// `net.bind` gate for listen / udp bind.
    pub(crate) fn admit_bind(self, cache: &mut AdmitCache, ip: [u8; 4], port: u16) -> bool {
        self.evaluate(cache, ABI_CLASS_NET_BIND, bind_addr_class(ip, port), port, [0; 4])
    }

    /// `net.connect` gate.
    pub(crate) fn admit_connect(self, cache: &mut AdmitCache, ip: [u8; 4], port: u16) -> bool {
        self.evaluate(cache, ABI_CLASS_NET_CONNECT, 0, port, ip)
    }
}

/// `!cap-deny: enforcer=netstackd class=<c> port=<p> addr=<loopback|any> subject=0x<sid>` —
/// the greppable refusal line (same shape as `nexus_ipc::policyd::authorize`).
fn emit_deny(subject: u64, class: u8, port: u16, addr_class: u8) {
    let mut line = [0u8; 96];
    let mut n = 0usize;
    let mut put = |b: &[u8]| {
        for &c in b {
            if n < line.len() {
                line[n] = c;
                n += 1;
            }
        }
    };
    put(b"!cap-deny: enforcer=netstackd class=");
    put(if class == ABI_CLASS_NET_CONNECT { b"net.connect" } else { b"net.bind" });
    put(b" port=");
    let mut d = [0u8; 5];
    let mut k = 0;
    let mut v = port;
    loop {
        d[k] = b'0' + (v % 10) as u8;
        k += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while k > 0 {
        k -= 1;
        put(&d[k..k + 1]);
    }
    put(b" addr=");
    put(if addr_class == ADDR_ANY { b"any" } else { b"loopback" });
    put(b" subject=0x");
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for i in 0..16 {
        put(&[HEX[((subject >> (60 - 4 * i)) & 0xf) as usize]]);
    }
    if let Ok(text) = core::str::from_utf8(&line[..n]) {
        let _ = nexus_abi::debug_println(text);
    }
}
