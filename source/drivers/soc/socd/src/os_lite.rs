// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The service loop (OS target): map the tree alias and every provider window
//! init granted (by kind), announce what the tree holds, then serve requests —
//! policy first (`soc.glue` of the kernel-attributed sender), then the verdict.
//! Every slot is declared and pinned before socd runs (TASK-0246 P4c): it serves the
//! block owner during the core plane, while init's responder does not answer yet, so
//! it asks init for nothing — not its own server, not its route to policyd.

extern crate alloc;

use alloc::format;

use nexus_abi::yield_;
use nexus_fdt::Fdt;
use nexus_ipc::{KernelServer, Server as _, Wait};
use nexus_service_topology::{slots, SYSCON_MMIO_SLOTS};
use nexus_soc::{Provider, ProviderKind, Providers};
use nexus_wire::soc;

use crate::bus::MmioBus;

/// The grant's granularity (RFC-0017).
const PAGE: usize = 4096;
use crate::verdict::{self, Access, REPLY_MAX};

pub type Result<T> = core::result::Result<T, &'static str>;

/// The policy capability a requester must hold.
const CAP_SOC_GLUE: &[u8] = b"soc.glue";

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

/// Every provider kind whose window init granted into `SYSCON_MMIO_SLOTS[kind]`,
/// mapped; a kind the tree does not list has an empty slot and is skipped.
fn map_providers(tree: Option<&Fdt<'_>>) -> Providers {
    const KINDS: [ProviderKind; 6] = [
        ProviderKind::Apbc,
        ProviderKind::Apmu,
        ProviderKind::Mpmu,
        ProviderKind::Apbc2,
        ProviderKind::Pll,
        ProviderKind::Pinctrl,
    ];
    let mut providers = Providers::new();
    let Some(tree) = tree else { return providers };
    for kind in KINDS {
        // Only a kind the tree names is expected in its slot.
        let Some(node) = tree.all_nodes().find(|n| ProviderKind::of(*n) == Some(kind)) else {
            continue;
        };
        // The grant covers the pages the block's registers occupy (init's `window_of`);
        // the block itself may start inside its page (the APMU at 0xd4282800) — the
        // offset is the tree's, read here from the same node (TASK-0260B P3).
        let offset = node.reg(0).ok().flatten().map_or(0, |r| (r.addr % PAGE as u64) as usize);
        let slot = SYSCON_MMIO_SLOTS[kind as usize];
        let mut info = nexus_abi::CapQuery::default();
        if nexus_abi::cap_query(slot, &mut info).is_err() || info.kind_tag != 2 {
            emit(&format!("socd: window for {:?} not granted (slot 0x{:x})", kind, slot));
            continue;
        }
        let len = usize::try_from(info.len).unwrap_or(0);
        match nexus_abi::mmio_map_auto(slot, 0, len) {
            Ok(base) => providers.set(Provider { kind, base: base + offset }),
            Err(_) => emit(&format!("socd: window for {:?} map FAIL", kind)),
        }
    }
    providers
}

/// socd's server on the slots init pins for it — the only source (no route ask: the core
/// plane runs before init's responder answers).
fn declared_server() -> Option<KernelServer> {
    let pair = slots::socd::SERVER;
    KernelServer::new_with_slots(pair.recv, pair.send).ok()
}

/// policyd's verdict on `soc.glue` for the requester, over socd's declared route and reply
/// inbox; anything but an allow — including an unreachable authority — is a denial.
fn access_of(sender_service_id: u64) -> Access {
    let decision = nexus_ipc::policyd::check_cap_on(
        slots::socd::POLICYD.send,
        slots::socd::REPLY.send,
        slots::socd::REPLY.recv,
        sender_service_id,
        CAP_SOC_GLUE,
    );
    match decision {
        nexus_ipc::policyd::CapDecision::Allow => Access::Allowed,
        _ => Access::Denied,
    }
}

pub fn service_main_loop() -> Result<()> {
    let tree_bytes = nexus_abi::device_tree::map_read_only(slots::socd::DEVICE_TREE);
    let tree = tree_bytes.and_then(|b| Fdt::new(b).ok());
    let providers = map_providers(tree.as_ref());
    let bus = MmioBus;

    if providers.count() == 0 {
        let _ = nexus_service_entry::ready("socd: ready (no soc glue in this tree)");
    } else {
        let _ =
            nexus_service_entry::ready(&format!("socd: ready (providers={})", providers.count()));
    }

    let server = declared_server().ok_or("server slots missing")?;
    nexus_abi::service_verdict_flush("socd");
    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    let mut recv_frame = alloc::vec![0u8; nexus_abi::IPC_PAYLOAD_MAX];
    let mut pending = nexus_ipc::PendingReply::new();
    let mut out = [0u8; REPLY_MAX];
    loop {
        match server.serve_next(&mut pending, Wait::Blocking, &mut recv_frame) {
            Ok((_hdr, frame_len, sender_service_id, reply)) => {
                breaker.on_success();
                let frame = &recv_frame[..frame_len];
                // Policy before any register: the kernel-attributed sender, never a
                // payload string. A malformed frame is answered without a policy trip.
                let access = if soc::decode_request_op(frame).is_some() {
                    access_of(sender_service_id)
                } else {
                    Access::Denied
                };
                let (n, outcome) =
                    verdict::answer(frame, access, tree.as_ref(), &providers, &bus, &mut out);
                if let Some((_, path)) = soc::decode_bring_up_req(frame) {
                    emit(verdict::bring_up_marker(path, &outcome).as_str());
                }
                let rsp = &out[..n];
                if let Some(reply) = reply {
                    pending.park(reply, rsp);
                } else {
                    let _ = server.send(rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err("disconnected"),
            Err(_) => {
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit("socd: recv error (transient)");
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        return Err("endpoint defect");
                    }
                }
            }
        }
    }
}
