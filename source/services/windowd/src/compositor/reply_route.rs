// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where a windowd reply goes, and the count of each route
//! (TASK-0054C P2-g). windowd answers a request one of three ways: on the
//! client's DEDICATED channel (the nonce- or surface-bound endpoint), on a
//! reply capability the client MOVED with its request, or — the fallback — on
//! windowd's OWN shared response endpoint with a blocking send.
//!
//! WHY THIS FILE EXISTS: that third way is the 0049B wedge class this task
//! removed from logd (P2-c), metricsd (P2-e) and statefsd (P2-f). In those
//! three nobody read the shared queue, so the acks piled up until the next
//! blocking send never returned. windowd's shared endpoint DOES have readers
//! (inputd, imed, execd and the app-host children all route to it as
//! `SharedResponse`), so the failure mode is subtler and worse: any reader may
//! take any ack, so a reply can be consumed by the wrong client and the queue
//! can still fill — and when it does, the blocking send stops the COMPOSITOR,
//! which stops every frame and every input event behind it.
//!
//! P2-g measures before it changes anything: the routing is now ONE function
//! with counters, and the shared fallback announces itself at every power of
//! two so a filling queue is visible in the boot log (the endpoint depth is 8,
//! so `n=8` is the wedge itself). The 1s loop line cannot be that witness — it
//! only prints when frames are flowing, which is exactly not the case once the
//! compositor is stuck.
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`WINDOWD: reply route` markers; the input + present ladders)
//! RFC: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md

use core::sync::atomic::{AtomicU32, Ordering};

use nexus_ipc::{KernelServer, Server as _, Wait};

/// Replies delivered on a client's dedicated channel.
static ON_CHANNEL: AtomicU32 = AtomicU32::new(0);
/// Replies delivered on a reply capability the client moved with its request.
static ON_CAPMOVE: AtomicU32 = AtomicU32::new(0);
/// Replies delivered on windowd's own shared response endpoint (the fallback).
static ON_SHARED: AtomicU32 = AtomicU32::new(0);

/// Answer one request, wherever its client can receive it, and count the route.
///
/// `delivered_on_channel` is true when the caller already shipped `response` on
/// the client's dedicated channel; `reply` is the capability the client moved,
/// if any. The remaining case is the shared-endpoint fallback.
pub(super) fn answer(
    server: &KernelServer,
    reply: Option<nexus_ipc::ReplyCap>,
    response: &[u8],
    delivered_on_channel: bool,
    op: u8,
) {
    if delivered_on_channel {
        ON_CHANNEL.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if let Some(reply) = reply {
        ON_CAPMOVE.fetch_add(1, Ordering::Relaxed);
        let _ = reply.reply_and_close_wait(response, Wait::Blocking);
        return;
    }
    let n = ON_SHARED.fetch_add(1, Ordering::Relaxed) + 1;
    // Announce at every power of two: the first use proves the fallback is
    // live, and n=8 is the endpoint depth — the point at which the next
    // blocking send has nowhere to go.
    if n.is_power_of_two() {
        let _ = nexus_abi::debug_println(&alloc::format!(
            "WINDOWD: reply route shared (op={op:#04x} n={n})"
        ));
    }
    let _ = server.send(response, Wait::Blocking);
}

/// The three counts, for the telemetry line.
pub(super) fn counts() -> (u32, u32, u32) {
    (
        ON_CHANNEL.load(Ordering::Relaxed),
        ON_CAPMOVE.load(Ordering::Relaxed),
        ON_SHARED.load(Ordering::Relaxed),
    )
}
