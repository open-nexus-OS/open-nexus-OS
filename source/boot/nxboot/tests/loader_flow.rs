// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Loader-flow integration proof (TASK-0289 A2) against the shared fixture disk
//! (`common/`): the clean boot, the trial/exhaustion ladder and the adversarial matrix
//! (tamper — in the kernel and inside the bootfs —, downgrade, stranger key, zeroed slot,
//! both-bad, torn BSB, a container the window cannot place, a header that is no container).
//! The boot disk's own proofs (the SDHCI reader, the board's SD hosts) are `boot_disk.rs`.
//! OWNERS: @security
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md, docs/adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md

mod common;

use bootfmt::bsb::Slot;
use common::*;
use nxboot::flow::{Event, FlowError, PayloadWhy, Reason, IMAGE_START_SECTOR, SECTOR};
use storage::BlockDevice;

#[test]
fn factory_disk_boots_slot_a_clean() {
    let mut fx = factory_fixture();
    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("clean boot");
    assert_eq!(loaded.slot, Slot::A);
    assert!(!loaded.tries_decremented);
    assert_eq!(loaded.bsb_seq, 1);
    assert_placed(&dest, &fx.kernel_a, &loaded);
    assert_eq!(events[0], Event::BsbOk { slot: Slot::A, seq: 1 });
    assert!(matches!(events[1], Event::VerifyOk { slot: Slot::A, .. }));
    assert_eq!(events.len(), 2, "clean boot emits no actuator events");
}

#[test]
fn test_reject_a_tamper_inside_the_bootfs_as_a_digest_failure() {
    let mut fx = factory_fixture();
    let kernel_b = kernel_fixture(0xB7);
    let nxbd_b = signed_nxbd(&kernel_b, "build-B", 2, &dev_seed());
    write_slot(&mut fx.dev, &fx.boot_b.clone(), &kernel_b, &nxbd_b);
    // One byte of a bootfs FILE flipped after staging: the digest covers the bootfs too.
    let h = nexus_bootfs::payload::decode(&kernel_b).expect("header");
    let mut sector = [0u8; SECTOR];
    let lba = fx.boot_b.first_lba + IMAGE_START_SECTOR + h.bootfs_offset / SECTOR as u64 + 9;
    fx.dev.read_blocks(lba, &mut sector).expect("read");
    sector[5] ^= 0x01;
    fx.dev.write_blocks(lba, &sector).expect("tamper");
    schedule_trial(&mut fx, 1, 2);

    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("fallback boot");
    assert_eq!(loaded.slot, Slot::A, "a bootfs tamper falls back to active");
    assert!(events.contains(&Event::VerifyFail { slot: Slot::B, reason: Reason::Digest }));
    assert_placed(&dest, &fx.kernel_a, &loaded);
}

/// A slot whose container header says something else than the bytes: re-signed, so only the
/// placement check can refuse it.
fn stage_patched_header(fx: &mut Fixture, patch: impl FnOnce(&mut [u8])) {
    let mut image = kernel_fixture(0xB7);
    patch(&mut image[..nexus_bootfs::payload::HEADER_LEN]);
    let nxbd = signed_nxbd(&image, "build-B", 2, &dev_seed());
    write_slot(&mut fx.dev, &fx.boot_b.clone(), &image, &nxbd);
    schedule_trial(fx, 1, 2);
}

#[test]
fn test_reject_a_container_the_window_cannot_place() {
    let mut fx = factory_fixture();
    // A footprint of 3 MiB puts the bootfs at 4 MiB — the window's end.
    stage_patched_header(&mut fx, |h| h[24..32].copy_from_slice(&(3u64 << 20).to_le_bytes()));
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("fallback").slot, Slot::A);
    assert!(events.contains(&Event::VerifyFail {
        slot: Slot::B,
        reason: Reason::Payload(PayloadWhy::Window),
    }));
}

#[test]
fn test_reject_a_header_that_is_not_a_container() {
    let mut fx = factory_fixture();
    stage_patched_header(&mut fx, |h| h[0] = b'?');
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("fallback").slot, Slot::A);
    assert!(events.contains(&Event::VerifyFail {
        slot: Slot::B,
        reason: Reason::Payload(PayloadWhy::Header(nexus_bootfs::Error::Magic)),
    }));
    // Items that do not add up to the descriptor's image size.
    let mut fx = factory_fixture();
    stage_patched_header(&mut fx, |h| h[40..48].copy_from_slice(&(8u64 * 4096).to_le_bytes()));
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("fallback").slot, Slot::A);
    assert!(events
        .contains(&Event::VerifyFail { slot: Slot::B, reason: Reason::Payload(PayloadWhy::Size) }));
}

#[test]
fn trial_ladder_decrements_then_falls_back_exhausted() {
    let mut fx = factory_fixture();
    let kernel_b = kernel_fixture(0xB7);
    let nxbd_b = signed_nxbd(&kernel_b, "build-B", 2, &dev_seed());
    write_slot(&mut fx.dev, &fx.boot_b.clone(), &kernel_b, &nxbd_b);
    schedule_trial(&mut fx, 1, 2);

    // Boot 1: trial, tries 2->1, boots B.
    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("trial boot 1");
    assert_eq!(loaded.slot, Slot::B);
    assert!(loaded.tries_decremented);
    assert_eq!(loaded.bsb_seq, 3, "actuator write bumped seq");
    assert_placed(&dest, &kernel_b, &loaded);
    assert_eq!(events[1], Event::Tries { slot: Slot::B, from: 2, to: 1 });

    // Boot 2 (no health commit happened): tries 1->0, still boots B.
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("trial boot 2").slot, Slot::B);
    assert_eq!(events[1], Event::Tries { slot: Slot::B, from: 1, to: 0 });

    // Boot 3: exhausted — clears next, boots the standing active slot A.
    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("exhausted fallback");
    assert_eq!(loaded.slot, Slot::A);
    assert!(!loaded.tries_decremented);
    assert_eq!(events[1], Event::Exhausted { attempted: Slot::B, to: Slot::A });
    assert_placed(&dest, &fx.kernel_a, &loaded);

    // Boot 4: steady state again, no more actuator writes.
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("steady").slot, Slot::A);
    assert_eq!(events.len(), 2);
}

#[test]
fn tampered_trial_image_falls_back_to_active() {
    let mut fx = factory_fixture();
    let kernel_b = kernel_fixture(0xB7);
    let nxbd_b = signed_nxbd(&kernel_b, "build-B", 2, &dev_seed());
    write_slot(&mut fx.dev, &fx.boot_b.clone(), &kernel_b, &nxbd_b);
    // Boot-time tamper: flip one byte in the slot body AFTER staging.
    let mut sector = [0u8; SECTOR];
    let lba = fx.boot_b.first_lba + IMAGE_START_SECTOR + 3;
    fx.dev.read_blocks(lba, &mut sector).expect("read");
    sector[17] ^= 0x40;
    fx.dev.write_blocks(lba, &sector).expect("tamper");
    schedule_trial(&mut fx, 1, 2);

    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("fallback boot");
    assert_eq!(loaded.slot, Slot::A, "digest failure falls back to active");
    assert!(events.contains(&Event::VerifyFail { slot: Slot::B, reason: Reason::Digest }));
    assert!(events.contains(&Event::VerifyFallback { to: Slot::A }));
    assert_placed(&dest, &fx.kernel_a, &loaded);
}

#[test]
fn downgrade_below_floor_is_rejected_by_the_loader_backstop() {
    let mut fx = factory_fixture();
    let kernel_b = kernel_fixture(0xB7);
    // rollbackIndex 0 while the BSB floor says 1 — bypassed userspace.
    let nxbd_b = signed_nxbd(&kernel_b, "build-B", 0, &dev_seed());
    write_slot(&mut fx.dev, &fx.boot_b.clone(), &kernel_b, &nxbd_b);
    schedule_trial(&mut fx, 1, 2);

    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("fallback").slot, Slot::A);
    assert!(events.contains(&Event::VerifyFail {
        slot: Slot::B,
        reason: Reason::Rollback { have: 0, min: 1 },
    }));
}

#[test]
fn stranger_key_and_zeroed_slot_reject_with_stable_reasons() {
    let mut fx = factory_fixture();
    let kernel_b = kernel_fixture(0xB7);
    let stranger = signed_nxbd(&kernel_b, "build-X", 2, &[0x42; 32]);
    write_slot(&mut fx.dev, &fx.boot_b.clone(), &kernel_b, &stranger);
    schedule_trial(&mut fx, 1, 1);
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("fallback").slot, Slot::A);
    assert!(events.contains(&Event::VerifyFail { slot: Slot::B, reason: Reason::Sig }));

    // Factory boot-b (zeroed NXBD) is `nxbd`, not `sig`.
    let mut fx = factory_fixture();
    schedule_trial(&mut fx, 1, 1);
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("fallback").slot, Slot::A);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::VerifyFail { slot: Slot::B, reason: Reason::Nxbd { .. } })));
}

#[test]
fn both_slots_bad_is_terminal_and_ordered() {
    let mut fx = factory_fixture();
    // Corrupt boot-a's NXBD too (bit flip breaks the signature).
    let mut sector = [0u8; SECTOR];
    fx.dev.read_blocks(fx.boot_a.first_lba, &mut sector).expect("read");
    sector[30] ^= 1;
    fx.dev.write_blocks(fx.boot_a.first_lba, &sector).expect("corrupt");

    let (got, _events, _) = run(&mut fx);
    assert!(matches!(
        got.expect_err("must not boot unverified bytes"),
        FlowError::BothSlotsBad {
            first: (Slot::A, Reason::Sig),
            second: (Slot::B, Reason::Nxbd { .. })
        }
    ));
}

#[test]
fn torn_bsb_pair_never_guesses_a_slot() {
    let mut fx = factory_fixture();
    let mut sector = [0u8; SECTOR];
    fx.dev.read_blocks(fx.bsb.first_lba, &mut sector).expect("read");
    sector[9] ^= 1; // corrupt seq under the CRC
    fx.dev.write_blocks(fx.bsb.first_lba, &sector).expect("torn");
    // Block 1 is still zeroed on the factory disk -> both invalid.
    let (got, _events, _) = run(&mut fx);
    assert_eq!(got.expect_err("no guessing"), FlowError::BsbInvalid);
}

/// A descriptor demanding a FIXED address is not bootable by a loader that
/// places a position-independent image (RFC-0098 C6).
#[test]
fn wrong_load_addr_is_a_descriptor_reject() {
    let mut fx = factory_fixture();
    let kernel_b = kernel_fixture(0xB7);
    let (_pk, pubkey_id) = bootfmt::nxbd::pubkey_id_for_seed(&dev_seed());
    let nxbd = bootfmt::nxbd::sign(
        &bootfmt::nxbd::Nxbd {
            rollback_index: 2,
            image_size: kernel_b.len() as u64,
            image_sha256: sha2_digest::sha256(&kernel_b),
            build_id: bootfmt::nxbd::Nxbd::build_id_from("bad-addr"),
            load_addr: 0x9000_0000,
            pubkey_id,
        },
        &dev_seed(),
    );
    write_slot(&mut fx.dev, &fx.boot_b.clone(), &kernel_b, &nxbd);
    schedule_trial(&mut fx, 1, 1);
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("fallback").slot, Slot::A);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::VerifyFail { slot: Slot::B, reason: Reason::Nxbd { .. } })));
}
