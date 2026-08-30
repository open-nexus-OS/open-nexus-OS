// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Loader-flow integration proof (TASK-0289 A2) against a fixture
//! GPT disk assembled with the SAME shared authorities `nx image build`
//! uses (storage::layout::plan + write_gpt, bootfmt factory BSB, NXBD-last
//! slot writes, the dev OS-image signing seed the anchor is baked from).
//! Covers the clean boot, the trial/exhaustion ladder and the adversarial
//! matrix (tamper, downgrade, stranger key, zeroed slot, both-bad, torn BSB).
//! OWNERS: @security
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

use bootfmt::bsb::{self, Bsb, Slot};
use nxboot::flow::{self, Event, FlowError, Reason, IMAGE_START_SECTOR, SECTOR};
use storage::gpt::{find_partition_named, write_gpt, Partition, GUID_NEXUS_BOOT, GUID_NEXUS_BSB};
use storage::layout::{plan, NEXUS_DISK_BYTES};
use storage::{BlockDevice, MemBlockDevice};

fn dev_seed() -> [u8; 32] {
    let hex = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../keys/dev-os-image.ed25519.seed"),
    )
    .expect("dev-os-image seed");
    let hex = hex.trim();
    let mut seed = [0u8; 32];
    for (i, slot) in seed.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex");
    }
    seed
}

fn kernel_fixture(tag: u8) -> Vec<u8> {
    // Deliberately not sector-aligned to prove padding handling.
    (0..100_001u32).map(|i| (i as u8) ^ tag).collect()
}

fn signed_nxbd(kernel: &[u8], build: &str, rollback_index: u32, seed: &[u8; 32]) -> [u8; SECTOR] {
    use sha2_digest::sha256;
    let (_pk, pubkey_id) = bootfmt::nxbd::pubkey_id_for_seed(seed);
    bootfmt::nxbd::sign(
        &bootfmt::nxbd::Nxbd {
            rollback_index,
            image_size: kernel.len() as u64,
            image_sha256: sha256(kernel),
            build_id: bootfmt::nxbd::Nxbd::build_id_from(build),
            load_addr: 0x8020_0000,
            pubkey_id,
        },
        seed,
    )
}

mod sha2_digest {
    pub fn sha256(bytes: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let mut out = [0u8; 32];
        out.copy_from_slice(&hasher.finalize());
        out
    }
}

/// NXBD-last slot write — the same discipline `nx image` and `updated` use.
fn write_slot(dev: &mut MemBlockDevice, part: &Partition, kernel: &[u8], nxbd: &[u8; SECTOR]) {
    dev.write_blocks(part.first_lba, &[0u8; SECTOR]).expect("clear nxbd");
    let mut padded = kernel.to_vec();
    padded.resize(kernel.len().div_ceil(SECTOR) * SECTOR, 0);
    dev.write_blocks(part.first_lba + IMAGE_START_SECTOR, &padded).expect("image body");
    dev.write_blocks(part.first_lba, nxbd).expect("nxbd last");
}

struct Fixture {
    dev: MemBlockDevice,
    bsb: Partition,
    boot_a: Partition,
    boot_b: Partition,
    kernel_a: Vec<u8>,
}

/// Factory disk exactly as `nx image build` lays it out: GPT + factory BSB
/// (block 0 seq 1, active A committed; block 1 zeroed) + signed boot-a;
/// boot-b stays zeroed (invalid by definition).
fn factory_fixture() -> Fixture {
    let parts = plan().expect("layout plan");
    let mut dev = MemBlockDevice::new(SECTOR, NEXUS_DISK_BYTES / SECTOR as u64);
    write_gpt(&mut dev, &parts).expect("gpt");
    let bsb_part = find_partition_named(&parts, &GUID_NEXUS_BSB, "bsb").expect("bsb part");
    let boot_a = find_partition_named(&parts, &GUID_NEXUS_BOOT, "boot-a").expect("boot-a");
    let boot_b = find_partition_named(&parts, &GUID_NEXUS_BOOT, "boot-b").expect("boot-b");
    dev.write_blocks(bsb_part.first_lba, &bsb::encode(&Bsb::factory())).expect("factory bsb");
    let kernel_a = kernel_fixture(0);
    let nxbd = signed_nxbd(&kernel_a, "build-A", 1, &dev_seed());
    write_slot(&mut dev, &boot_a, &kernel_a, &nxbd);
    Fixture { dev, bsb: bsb_part, boot_a, boot_b, kernel_a }
}

fn run(fx: &mut Fixture) -> (Result<flow::Loaded, FlowError>, Vec<Event>, Vec<u8>) {
    let mut dest = vec![0u8; 2 * 1024 * 1024];
    let mut events = Vec::new();
    let got = flow::run(&mut fx.dev, &mut dest, &mut |e| events.push(e));
    (got, events, dest)
}

fn schedule_trial(fx: &mut Fixture, floor: u32, tries: u8) {
    // bootctld-style projection: alternate block, higher seq, next=b.
    let next = Bsb {
        seq: 2,
        active_slot: Slot::A,
        next_slot: Some(Slot::B),
        tries_left: tries,
        health_committed: false,
        boot_target: 0,
        rollback_min_index: floor,
    };
    fx.dev.write_blocks(fx.bsb.first_lba + 1, &bsb::encode(&next)).expect("schedule");
}

#[test]
fn factory_disk_boots_slot_a_clean() {
    let mut fx = factory_fixture();
    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("clean boot");
    assert_eq!(loaded.slot, Slot::A);
    assert_eq!(loaded.image_len, fx.kernel_a.len());
    assert!(!loaded.tries_decremented);
    assert_eq!(loaded.bsb_seq, 1);
    assert_eq!(&dest[..fx.kernel_a.len()], fx.kernel_a.as_slice(), "verified bytes in dest");
    assert_eq!(events[0], Event::BsbOk { slot: Slot::A, seq: 1 });
    assert!(matches!(events[1], Event::VerifyOk { slot: Slot::A, .. }));
    assert_eq!(events.len(), 2, "clean boot emits no actuator events");
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
    assert_eq!(&dest[..kernel_b.len()], kernel_b.as_slice());
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
    assert_eq!(&dest[..fx.kernel_a.len()], fx.kernel_a.as_slice());

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
    assert_eq!(&dest[..fx.kernel_a.len()], fx.kernel_a.as_slice());
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
    assert!(events.contains(&Event::VerifyFail { slot: Slot::B, reason: Reason::Nxbd }));
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
    assert_eq!(
        got.expect_err("must not boot unverified bytes"),
        FlowError::BothSlotsBad { first: (Slot::A, Reason::Sig), second: (Slot::B, Reason::Nxbd) }
    );
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
    assert!(events.contains(&Event::VerifyFail { slot: Slot::B, reason: Reason::Nxbd }));
}
