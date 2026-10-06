// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pure request → reply function: no IPC, no policy transport (the caller
//! supplies the policy verdict), no mapping — only the tree, the provider
//! windows and a `Bus`. Host tests run it against the golden tree and a register
//! file seeded from the board; the service loop runs it against real windows.
//! The marker is pure too: `bring_up_marker` writes the line the loop prints.

use core::fmt::{self, Write as _};

use nexus_fdt::{Fdt, Node};
use nexus_hal::Bus;
use nexus_soc::{
    bring_up, bring_up_with, clock_rate, BringUp, BringUpError, Pause, PlanError, Providers,
    RateError,
};
use nexus_wire::soc;

/// The largest reply any op produces.
pub const REPLY_MAX: usize = 32;

/// The most registers a bring-up marker names (the encoder touches seven: three glue words and
/// four pads).
pub const MAX_WORDS: usize = 8;

/// What the caller established about the requester before the verdict runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// policyd allowed `soc.glue` for the requester.
    Allowed,
    /// Denied, or the authority was unreachable — fail closed.
    Denied,
}

/// One register a bring-up touched: where it lives and its word before and after.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Word {
    pub addr: usize,
    /// The provider window's name (`apmu`) and the offset inside it.
    pub window: &'static str,
    pub offset: usize,
    pub before: u32,
    pub after: u32,
}

/// A failed step: its kind and, when it has one, the register and the word it read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failed {
    pub step: &'static str,
    pub reg: Option<(&'static str, usize, u32)>,
}

/// What a request did, for the marker: the reply's counts, the rates set and the writes the
/// bring-up took, the failed step, a plan's refusal, and the words of every register the
/// bring-up touched — before and after (the instrument that decides a domain's protocol on
/// the board, `docs/board/measurements/2026-09-30-power-domains/`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub status: u8,
    pub domains: u8,
    pub resets: u8,
    pub clocks: u8,
    pub rates: u8,
    pub pads: u8,
    /// GPIO lines driven (TASK-0328 U3).
    pub gpios: u8,
    pub writes: u8,
    pub failed: Option<Failed>,
    pub refused: Option<PlanError>,
    pub words: [Word; MAX_WORDS],
    pub nwords: usize,
}

impl Outcome {
    const fn of(status: u8) -> Self {
        Outcome {
            status,
            domains: 0,
            resets: 0,
            clocks: 0,
            rates: 0,
            pads: 0,
            gpios: 0,
            writes: 0,
            failed: None,
            refused: None,
            words: [Word { addr: 0, window: "", offset: 0, before: 0, after: 0 }; MAX_WORDS],
            nwords: 0,
        }
    }

    /// The registers the bring-up touched, in step order.
    pub fn words(&self) -> &[Word] {
        &self.words[..self.nwords]
    }
}

fn sat(n: usize) -> u8 {
    n.min(255) as u8
}

/// The window name and offset of a planned register (`apmu`, 0x3f4).
fn name(providers: &Providers, addr: usize) -> (&'static str, usize) {
    providers.locate(addr).map_or(("?", addr), |(kind, offset)| (kind.name(), offset))
}

/// The words of every register `node`'s plan touches, read before the bring-up.
fn words_before<B: Bus>(node: Node<'_>, providers: &Providers, bus: &B, o: &mut Outcome) {
    let Ok(plan) = nexus_soc::plan(node, providers) else { return };
    for addr in plan.registers().iter().take(MAX_WORDS) {
        let (window, offset) = name(providers, addr);
        o.words[o.nwords] = Word { addr, window, offset, before: bus.read(addr), after: 0 };
        o.nwords += 1;
    }
}

/// Answer `frame` into `out`; returns the reply length and the outcome (for the
/// marker). A frame that is not ours or malformed gets a short status reply.
pub fn answer<B: Bus>(
    frame: &[u8],
    access: Access,
    tree: Option<&Fdt<'_>>,
    providers: &Providers,
    bus: &B,
    pause: Option<&dyn Pause>,
    out: &mut [u8; REPLY_MAX],
) -> (usize, Outcome) {
    let short = |out: &mut [u8; REPLY_MAX], op: u8, status: u8, nonce: u32| {
        let n = soc::encode_status_rsp(out, op, status, nonce).unwrap_or(0);
        (n, Outcome::of(status))
    };
    let Some(op) = soc::decode_request_op(frame) else {
        return short(out, 0, soc::STATUS_MALFORMED, 0);
    };
    match op {
        soc::OP_BRING_UP => {
            let Some((nonce, path)) = soc::decode_bring_up_req(frame) else {
                return short(out, op, soc::STATUS_MALFORMED, 0);
            };
            // Every verdict of a well-formed request is the full reply shape: one
            // decoder for callers, the nonce always echoed.
            let mut rsp = soc::BringUpReply {
                status: soc::STATUS_NOT_NEEDED,
                nonce,
                domains: 0,
                resets: 0,
                clocks: 0,
                pads: 0,
                fault_addr: 0,
                fault_value: 0,
            };
            let mut outcome = Outcome::of(soc::STATUS_NOT_NEEDED);
            let finish = |out: &mut [u8; REPLY_MAX], rsp: soc::BringUpReply, mut o: Outcome| {
                let n = soc::encode_bring_up_rsp(out, &rsp).unwrap_or(0);
                (o.status, o.domains, o.resets, o.clocks, o.pads) =
                    (rsp.status, rsp.domains, rsp.resets, rsp.clocks, rsp.pads);
                (n, o)
            };
            if access == Access::Denied {
                rsp.status = soc::STATUS_DENIED;
                return finish(out, rsp, outcome);
            }
            let Some(tree) = tree else {
                // No tree at all (a foreign loader): nothing can be bound.
                return finish(out, rsp, outcome);
            };
            let Some(node) = tree.node_at_path(path) else {
                rsp.status = soc::STATUS_NO_SUCH_NODE;
                return finish(out, rsp, outcome);
            };
            words_before(node, providers, bus, &mut outcome);
            let result = match pause {
                Some(pause) => bring_up_with(node, providers, bus, pause),
                None => bring_up(node, providers, bus),
            };
            for w in outcome.words.iter_mut().take(outcome.nwords) {
                w.after = bus.read(w.addr);
            }
            match result {
                Ok(BringUp::NotNeeded) => {}
                Ok(BringUp::Up(report)) => {
                    rsp.status = soc::STATUS_OK;
                    rsp.domains = sat(report.domains);
                    rsp.resets = sat(report.resets_released);
                    rsp.clocks = sat(report.clocks_on);
                    rsp.pads = sat(report.pads);
                    outcome.gpios = sat(report.gpios);
                    outcome.rates = sat(report.rates_set);
                    outcome.writes = sat(report.writes);
                }
                Err(BringUpError::Fault(fault)) => {
                    rsp.status = soc::STATUS_FAILED;
                    let reg = fault.register();
                    if let Some((addr, value)) = reg {
                        rsp.fault_addr = addr as u64;
                        rsp.fault_value = value;
                    }
                    outcome.failed = Some(Failed {
                        step: fault.step(),
                        reg: reg.map(|(addr, value)| {
                            let (window, offset) = name(providers, addr);
                            (window, offset, value)
                        }),
                    });
                }
                Err(BringUpError::Plan(why)) => {
                    rsp.status = soc::STATUS_UNSUPPORTED;
                    outcome.refused = Some(why);
                }
            }
            finish(out, rsp, outcome)
        }
        soc::OP_CLOCK_RATE => {
            let Some((nonce, path, name)) = soc::decode_clock_rate_req(frame) else {
                return short(out, op, soc::STATUS_MALFORMED, 0);
            };
            let (status, hz) = if access == Access::Denied {
                (soc::STATUS_DENIED, 0)
            } else {
                rate_of(tree, providers, bus, path, name)
            };
            let n = soc::encode_clock_rate_rsp(out, status, nonce, hz).unwrap_or(0);
            (n, Outcome::of(status))
        }
        _ => short(out, op, soc::STATUS_MALFORMED, 0),
    }
}

/// The wire verdict on a named clock's rate (`nexus_soc::clock_rate`): a node that names no
/// such clock needs none, a provider or id the tables do not cover is unsupported.
fn rate_of<B: Bus>(
    tree: Option<&Fdt<'_>>,
    providers: &Providers,
    bus: &B,
    path: &str,
    name: &str,
) -> (u8, u64) {
    let Some(tree) = tree else { return (soc::STATUS_NOT_NEEDED, 0) };
    let Some(node) = tree.node_at_path(path) else { return (soc::STATUS_NO_SUCH_NODE, 0) };
    match clock_rate(node, name, providers, bus) {
        Ok(hz) => (soc::STATUS_OK, hz),
        Err(RateError::NoSuchClock) => (soc::STATUS_NOT_NEEDED, 0),
        Err(RateError::ProviderKindUnknown)
        | Err(RateError::ProviderUnknown(_))
        | Err(RateError::IdUnknown(..)) => (soc::STATUS_UNSUPPORTED, 0),
    }
}

/// A marker line fits the kernel console's line (256 bytes) with room for its newline.
pub const MARKER_MAX: usize = 240;

/// The room a dropped-word count takes (` +6 more`).
const MORE: usize = 8;

/// One marker line, built without an allocator; a register word that does not fit is
/// counted (`+N more`), never cut in half — the count's room is held back while words follow.
pub struct Marker {
    buf: [u8; MARKER_MAX],
    len: usize,
    limit: usize,
}

impl Default for Marker {
    fn default() -> Self {
        Self::new()
    }
}

impl Marker {
    pub const fn new() -> Self {
        Marker { buf: [0; MARKER_MAX], len: 0, limit: MARKER_MAX }
    }

    /// Append `args` whole, or not at all: `false` when it does not fit (the caller starts the
    /// next line).
    pub fn push_fmt(&mut self, args: fmt::Arguments<'_>) -> bool {
        self.try_push(args)
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("socd: marker not utf-8")
    }

    /// Append `args` whole, or not at all (the length is restored on overflow).
    fn try_push(&mut self, args: fmt::Arguments<'_>) -> bool {
        let at = self.len;
        if self.write_fmt(args).is_err() {
            self.len = at;
            return false;
        }
        true
    }
}

impl fmt::Write for Marker {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        if end > self.limit {
            return Err(fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// The line `socd` prints for a bring-up (RFC-0106): the node and what happened —
/// `ok (domains=… resets=… clocks=… rates=… pads=… gpios=… writes=…)`, `not needed`, or `FAIL (step=…
/// reg=<window>+0x… val=0x…)` — followed by every register the bring-up touched as
/// `<window>+<offset>:<before>><after>`, all hex — compact, so a node's glue fits one line.
pub fn bring_up_marker(path: &str, o: &Outcome) -> Marker {
    let mut m = Marker::new();
    let words = o.words();
    if !words.is_empty() {
        m.limit = MARKER_MAX - MORE;
    }
    let head = match o.status {
        soc::STATUS_OK => m.try_push(format_args!(
            "socd: bring-up {} ok (domains={} resets={} clocks={} rates={} pads={} gpios={} writes={})",
            path, o.domains, o.resets, o.clocks, o.rates, o.pads, o.gpios, o.writes
        )),
        soc::STATUS_NOT_NEEDED => m.try_push(format_args!("socd: bring-up {} not needed", path)),
        soc::STATUS_FAILED => match o.failed {
            Some(Failed { step, reg: Some((window, offset, value)) }) => m.try_push(format_args!(
                "socd: bring-up {} FAIL (step={} reg={}+0x{:x} val=0x{:x})",
                path, step, window, offset, value
            )),
            Some(Failed { step, reg: None }) => {
                m.try_push(format_args!("socd: bring-up {} FAIL (step={})", path, step))
            }
            None => m.try_push(format_args!("socd: bring-up {} FAIL", path)),
        },
        soc::STATUS_UNSUPPORTED => match o.refused {
            Some(why) => {
                m.try_push(format_args!("socd: bring-up {} FAIL (refused: {:?})", path, why))
            }
            None => m.try_push(format_args!("socd: bring-up {} FAIL (refused)", path)),
        },
        soc::STATUS_DENIED => m.try_push(format_args!("socd: bring-up {} FAIL (denied)", path)),
        soc::STATUS_NO_SUCH_NODE => {
            m.try_push(format_args!("socd: bring-up {} FAIL (no such node)", path))
        }
        status => m.try_push(format_args!("socd: bring-up {} FAIL (status={})", path, status)),
    };
    if !head {
        // A path longer than a console line: the verdict still goes out, the path does not.
        m.limit = MARKER_MAX;
        let _ = m.try_push(format_args!("socd: bring-up (path too long) status={}", o.status));
        return m;
    }
    for (i, w) in words.iter().enumerate() {
        if i + 1 == words.len() {
            m.limit = MARKER_MAX;
        }
        let fits =
            m.try_push(format_args!(" {}+{:x}:{:x}>{:x}", w.window, w.offset, w.before, w.after));
        if !fits {
            m.limit = MARKER_MAX;
            let _ = m.try_push(format_args!(" +{} more", words.len() - i));
            break;
        }
    }
    m
}
