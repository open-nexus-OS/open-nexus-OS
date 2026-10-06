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

use core::cell::RefCell;

use nexus_abi::yield_;
use nexus_driverkit::MmioSet;
use nexus_fdt::Fdt;
use nexus_ipc::timer::NotifyTimer;
use nexus_ipc::{KernelServer, Server as _, Wait};
use nexus_service_topology::{slots, SYSCON_MMIO_SLOTS};
use nexus_soc::{Pause, Provider, ProviderKind, Providers};
use nexus_wire::soc;

/// A bring-up's settle (TASK-0328 U3: a supply's start-up delay) spent on socd's declared
/// kernel one-shot, waited for on its frame — never a spin. Without the timer (its bind
/// failed) no pause exists and a plan with a settle faults honestly (`step=settle`).
struct OneShot(RefCell<NotifyTimer>);

impl Pause for OneShot {
    fn pause_ms(&self, ms: u32) {
        let mut timer = self.0.borrow_mut();
        timer.arm_in(u64::from(ms) * 1_000_000);
        let _ = timer.wait_fired();
    }
}

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
/// mapped: the providers (the block's address inside its window, for the plans) and the
/// windows as one bus (bounds-checked; an address outside every window floats). A kind the
/// tree does not list has an empty slot and is skipped.
fn map_providers(
    tree: Option<&Fdt<'_>>,
) -> (Providers, MmioSet<{ ProviderKind::COUNT }>, [usize; ProviderKind::COUNT]) {
    const KINDS: [ProviderKind; ProviderKind::COUNT] = [
        ProviderKind::Apbc,
        ProviderKind::Apmu,
        ProviderKind::Mpmu,
        ProviderKind::Apbc2,
        ProviderKind::Pll,
        ProviderKind::Pinctrl,
        ProviderKind::Gpio,
    ];
    let mut providers = Providers::new();
    let mut windows = MmioSet::new();
    // Each block's size as the tree names it (the stock comparison reads inside it).
    let mut sizes = [0usize; ProviderKind::COUNT];
    let Some(tree) = tree else { return (providers, windows, sizes) };
    for kind in KINDS {
        // Only a kind the tree names is expected in its slot.
        let Some(node) = tree.all_nodes().find(|n| ProviderKind::of(*n) == Some(kind)) else {
            continue;
        };
        // The grant covers the pages the block's registers occupy (init's `window_of`);
        // the block itself may start inside its page (the APMU at 0xd4282800) — the
        // offset is the tree's, read here from the same node (TASK-0260B P3).
        let reg = node.reg(0).ok().flatten();
        let offset = reg.map_or(0, |r| (r.addr % PAGE as u64) as usize);
        sizes[kind as usize] = reg.map_or(0, |r| usize::try_from(r.size).unwrap_or(0));
        let slot = SYSCON_MMIO_SLOTS[kind as usize];
        let mut info = nexus_abi::CapQuery::default();
        if nexus_abi::cap_query(slot, &mut info).is_err() || info.kind_tag != 2 {
            emit(&format!("socd: window for {:?} not granted (slot 0x{:x})", kind, slot));
            continue;
        }
        let len = usize::try_from(info.len).unwrap_or(0);
        match nexus_abi::MmioWindow::map(slot, 0, len) {
            Ok(window) => {
                providers.set(Provider { kind, base: window.base() + offset });
                windows.set(kind as usize, window);
            }
            Err(_) => emit(&format!("socd: window for {:?} map FAIL", kind)),
        }
    }
    (providers, windows, sizes)
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
    let (providers, bus, sizes) = map_providers(tree.as_ref());
    // A measurement (TASK-0328 U3): the loader's SoC state against the stock system's, before
    // any bring-up — the words that differ name the gates our tables do not cover.
    for (kind, name, dump) in [
        (ProviderKind::Apmu, "apmu", crate::stock::APMU),
        (ProviderKind::Mpmu, "mpmu", crate::stock::MPMU),
    ] {
        if let Some(p) = providers.get(kind) {
            crate::stock::report(name, p.base, sizes[kind as usize], dump, &bus, emit);
        }
    }

    if providers.count() == 0 {
        let _ = nexus_service_entry::ready("socd: ready (no soc glue in this tree)");
    } else {
        let _ =
            nexus_service_entry::ready(&format!("socd: ready (providers={})", providers.count()));
    }

    let server = declared_server().ok_or("server slots missing")?;
    let one_shot = NotifyTimer::bind(slots::socd::TIMER).ok().map(|t| OneShot(RefCell::new(t)));
    if one_shot.is_none() {
        emit("socd: settle timer FAIL (a bring-up with a start-up delay will fail)");
    }
    let pause = one_shot.as_ref().map(|p| p as &dyn Pause);
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
                let (n, outcome) = verdict::answer(
                    frame,
                    access,
                    tree.as_ref(),
                    &providers,
                    &bus,
                    pause,
                    &mut out,
                );
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
