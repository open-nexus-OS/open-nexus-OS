// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pure request → reply function: no IPC, no policy transport (the caller
//! supplies the policy verdict), no mapping — only the tree, the provider
//! windows and a `Bus`. Host tests run it against the golden tree and a register
//! file seeded from the board; the service loop runs it against real windows.

use nexus_fdt::Fdt;
use nexus_hal::Bus;
use nexus_soc::{bring_up, clock_rate, BringUp, BringUpError, Fault, Providers, RateError};
use nexus_wire::soc;

/// The largest reply any op produces.
pub const REPLY_MAX: usize = 32;

/// What the caller established about the requester before the verdict runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// policyd allowed `soc.glue` for the requester.
    Allowed,
    /// Denied, or the authority was unreachable — fail closed.
    Denied,
}

/// Human-readable outcome of a bring-up, for the marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub status: u8,
    pub domains: u8,
    pub resets: u8,
    pub clocks: u8,
}

/// Answer `frame` into `out`; returns the reply length and the outcome (for the
/// marker). A frame that is not ours or malformed gets a short status reply.
pub fn answer<B: Bus>(
    frame: &[u8],
    access: Access,
    tree: Option<&Fdt<'_>>,
    providers: &Providers,
    bus: &B,
    out: &mut [u8; REPLY_MAX],
) -> (usize, Outcome) {
    let short = |out: &mut [u8; REPLY_MAX], op: u8, status: u8, nonce: u32| {
        let n = soc::encode_status_rsp(out, op, status, nonce).unwrap_or(0);
        (n, Outcome { status, domains: 0, resets: 0, clocks: 0 })
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
            let finish = |out: &mut [u8; REPLY_MAX], rsp: soc::BringUpReply| {
                let n = soc::encode_bring_up_rsp(out, &rsp).unwrap_or(0);
                (n, outcome_of(&rsp))
            };
            if access == Access::Denied {
                rsp.status = soc::STATUS_DENIED;
                return finish(out, rsp);
            }
            let Some(tree) = tree else {
                // No tree at all (a foreign loader): nothing can be bound.
                return finish(out, rsp);
            };
            let Some(node) = tree.node_at_path(path) else {
                rsp.status = soc::STATUS_NO_SUCH_NODE;
                return finish(out, rsp);
            };
            match bring_up(node, providers, bus) {
                Ok(BringUp::NotNeeded) => {}
                Ok(BringUp::Up(report)) => {
                    rsp.status = soc::STATUS_OK;
                    rsp.domains = report.domains.min(255) as u8;
                    rsp.resets = report.resets_released.min(255) as u8;
                    rsp.clocks = report.clocks_on.min(255) as u8;
                }
                Err(BringUpError::Fault(Fault::ReadBack { addr, value }))
                | Err(BringUpError::Fault(Fault::FcStuck { addr, value })) => {
                    rsp.status = soc::STATUS_FAILED;
                    rsp.fault_addr = addr as u64;
                    rsp.fault_value = value;
                }
                Err(BringUpError::Fault(Fault::Range)) => rsp.status = soc::STATUS_FAILED,
                Err(BringUpError::Plan(_)) => rsp.status = soc::STATUS_UNSUPPORTED,
            }
            finish(out, rsp)
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
            (n, Outcome { status, domains: 0, resets: 0, clocks: 0 })
        }
        _ => short(out, op, soc::STATUS_MALFORMED, 0),
    }
}

fn outcome_of(rsp: &soc::BringUpReply) -> Outcome {
    Outcome { status: rsp.status, domains: rsp.domains, resets: rsp.resets, clocks: rsp.clocks }
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
