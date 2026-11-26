// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot-time proofs of the TLB shootdown on a machine of two or more harts (split out
//! of `selftest/mod.rs` with the targeted proof, TASK-0330 P3). A poll with nothing pending
//! flushes and acknowledges nothing; a shootdown of every hart advances the epoch and is
//! acknowledged by the other hart; a shootdown for one address space reaches exactly the harts
//! that may hold a translation of it — none for a space no hart read through, the one hart
//! recorded as reading through it, and none again once that hart's flush found it on another
//! space and it left the set.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: tlb shootdown counterfactual ok`, `KSELFTEST: tlb shootdown ok`,
//!   `KSELFTEST: tlb shootdown targeted ok (…)` on every lane with two or more harts
//!   (`scripts/qemu-test.sh`); the sets themselves on the host (`hart_spaces`)

use super::Context;
use crate::smp::tlb;
use crate::types::CpuId;
use nexus_sched::CpuSet;

/// `other` is a second online hart (it acknowledges from its software-interrupt trap).
pub(super) fn run(ctx: &mut Context<'_>, other: CpuId) {
    every_hart(other);
    one_space(ctx, other);
}

/// A5: the counterfactual first (a poll with no pending epoch must not flush or acknowledge),
/// then the real epoch round trip with the other hart's acknowledgement as evidence.
fn every_hart(other: CpuId) {
    let boot = CpuId::BOOT;
    let (_, _, ack_before) = tlb::selftest_evidence(boot);
    let outcome = tlb::poll_mailbox(boot);
    let (_, _, ack_after) = tlb::selftest_evidence(boot);
    if outcome == tlb::TlbPollOutcome::NoPending && ack_after == ack_before {
        log_info!(target: "selftest", "KSELFTEST: tlb shootdown counterfactual ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: tlb shootdown counterfactual FAIL outcome={:?} ack {}->{}",
            outcome,
            ack_before,
            ack_after
        );
    }

    let (epoch_before, _, other_ack_before) = tlb::selftest_evidence(other);
    tlb::shootdown_all();
    let (epoch_after, other_req, other_ack) = tlb::selftest_evidence(other);
    if epoch_after == epoch_before + 1
        && other_req == epoch_after
        && other_ack == other_req
        && other_ack > other_ack_before
    {
        log_info!(target: "selftest", "KSELFTEST: tlb shootdown ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: tlb shootdown FAIL epoch {}->{} req={} ack={} (before {})",
            epoch_before,
            epoch_after,
            other_req,
            other_ack,
            other_ack_before
        );
    }
}

/// TASK-0330 P3: a shootdown for one space reaches the harts that ran it, counted.
fn one_space(ctx: &mut Context<'_>, other: CpuId) {
    let (Some(harts), Ok(handle)) = (crate::hart_spaces::installed(), ctx.address_spaces.create())
    else {
        log_error!(target: "selftest", "KSELFTEST: tlb shootdown targeted FAIL (no space)");
        return;
    };
    let raw = handle.to_raw();
    let ran = || CpuSet::from_bits(harts.ran(raw).unwrap_or(u64::MAX));
    let mailbox = || tlb::selftest_evidence(other);

    // A space no hart read through: nobody is reached, the other hart's mailbox stays as it is.
    let (_, requested_before, _) = mailbox();
    let nobody = tlb::shootdown_of(raw);
    let unreached = ran().is_empty() && nobody.is_empty() && mailbox().1 == requested_before;

    // The other hart recorded as its dispatch (or its copy through the tables) records it:
    // it is reached, it acknowledges — and, running on another space, its flush lets it leave.
    harts.reads_through(other.as_index(), raw);
    let ran_before = ran();
    let reached = tlb::shootdown_of(raw);
    let (_, requested, acked) = mailbox();
    let answered = ran_before == CpuSet::EMPTY.with(other.as_index())
        && reached == ran_before
        && requested > requested_before
        && acked == requested;
    let left = ran().is_empty();

    // And so the next one reaches nobody again.
    let (_, requested_after, _) = mailbox();
    let again = tlb::shootdown_of(raw).is_empty() && mailbox().1 == requested_after;

    let freed = ctx.address_spaces.destroy(handle).is_ok();
    let online = crate::smp::cpu_online().count();
    if unreached && answered && left && again && freed {
        log_info!(
            target: "selftest",
            "KSELFTEST: tlb shootdown targeted ok (harts={} ran={} reached={} left=1 spared={})",
            online,
            ran_before.count(),
            reached.count(),
            online.saturating_sub(1 + reached.count())
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: tlb shootdown targeted FAIL unreached={} answered={} left={} again={} freed={} ran=0x{:x} reached=0x{:x}",
            unreached,
            answered,
            left,
            again,
            freed,
            ran_before.bits(),
            reached.bits()
        );
    }
}
