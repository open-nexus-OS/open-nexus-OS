// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The loader-flow fixtures both proof binaries share (`loader_flow.rs`,
//! `boot_disk.rs`): a factory GPT disk assembled with the SAME shared authorities `nx image
//! build` uses (storage::layout::plan + write_gpt, bootfmt factory BSB, NXBD-last slot writes,
//! the dev OS-image signing seed the anchor is baked from), a container boot image composed with
//! the packer's codec (ADR-0081), and one `run` into a 4 MiB window.
//! OWNERS: @security
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md, docs/adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md

use bootfmt::bsb::{self, Bsb, Slot};
use nxboot::flow::{self, Event, FlowError, IMAGE_START_SECTOR, SECTOR};
use storage::gpt::{find_partition_named, write_gpt, Partition, GUID_NEXUS_BOOT, GUID_NEXUS_BSB};
use storage::layout::{plan, NEXUS_DISK_BYTES};
use storage::{BlockDevice, MemBlockDevice};

pub fn dev_seed() -> [u8; 32] {
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

/// The fixture kernel's memory footprint: the bootfs lands at the first `BOOTFS_ALIGN`
/// boundary past it (2 MiB here), inside the 4 MiB window the tests load into.
pub const FIXTURE_FOOTPRINT: u64 = 1024 * 1024;

/// A fixture boot image (ADR-0081): one container — a kernel item deliberately not
/// sector-aligned (padding handling) and a one-file bootfs — composed with the packer's codec.
pub fn kernel_fixture(tag: u8) -> Vec<u8> {
    let kernel: Vec<u8> = (0..100_001u32).map(|i| (i as u8) ^ tag).collect();
    let mut files = nexus_bootfs::toc::Builder::new();
    files.add("init-lite", &vec![tag ^ 0x5A; 3000], 16, 0x1800).expect("fixture file");
    let bootfs = files.finish().expect("fixture bootfs");
    nexus_bootfs::payload::compose(&kernel, FIXTURE_FOOTPRINT, &bootfs).expect("fixture image")
}

/// The verified bytes where the loader put them: the kernel at the window's base, the bootfs
/// at the first 2 MiB boundary past the footprint — and nothing of the container's framing.
pub fn assert_placed(dest: &[u8], container: &[u8], loaded: &flow::Loaded) {
    let h = nexus_bootfs::payload::decode(container).expect("fixture header");
    let (ko, kl) = (h.kernel_offset as usize, h.kernel_len as usize);
    let (bo, bl) = (h.bootfs_offset as usize, h.bootfs_len as usize);
    assert_eq!(loaded.kernel_len, kl);
    assert_eq!(loaded.bootfs, (flow::BOOTFS_ALIGN, bl), "the first boundary past the footprint");
    assert_eq!(&dest[..kl], &container[ko..ko + kl], "verified kernel bytes in dest");
    let at = flow::BOOTFS_ALIGN;
    assert_eq!(&dest[at..at + bl], &container[bo..bo + bl], "verified bootfs bytes at its place");
}

pub fn signed_nxbd(
    kernel: &[u8],
    build: &str,
    rollback_index: u32,
    seed: &[u8; 32],
) -> [u8; SECTOR] {
    use sha2_digest::sha256;
    let (_pk, pubkey_id) = bootfmt::nxbd::pubkey_id_for_seed(seed);
    bootfmt::nxbd::sign(
        &bootfmt::nxbd::Nxbd {
            rollback_index,
            image_size: kernel.len() as u64,
            image_sha256: sha256(kernel),
            build_id: bootfmt::nxbd::Nxbd::build_id_from(build),
            load_addr: bootfmt::nxbd::LOAD_ADDR_RELOCATABLE,
            pubkey_id,
        },
        seed,
    )
}

pub mod sha2_digest {
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
pub fn write_slot<D: BlockDevice>(
    dev: &mut D,
    part: &Partition,
    kernel: &[u8],
    nxbd: &[u8; SECTOR],
) {
    dev.write_blocks(part.first_lba, &[0u8; SECTOR]).expect("clear nxbd");
    let mut padded = kernel.to_vec();
    padded.resize(kernel.len().div_ceil(SECTOR) * SECTOR, 0);
    dev.write_blocks(part.first_lba + IMAGE_START_SECTOR, &padded).expect("image body");
    dev.write_blocks(part.first_lba, nxbd).expect("nxbd last");
}

pub struct Fixture<D: BlockDevice = MemBlockDevice> {
    pub dev: D,
    pub bsb: Partition,
    pub boot_a: Partition,
    pub boot_b: Partition,
    pub kernel_a: Vec<u8>,
}

/// Factory disk exactly as `nx image build` lays it out: GPT + factory BSB
/// (block 0 seq 1, active A committed; block 1 zeroed) + signed boot-a;
/// boot-b stays zeroed (invalid by definition).
pub fn factory_fixture() -> Fixture {
    factory_fixture_on(MemBlockDevice::new(SECTOR, NEXUS_DISK_BYTES / SECTOR as u64))
}

/// The factory disk written onto `dev` through its own `BlockDevice` face — for the SDHCI reader,
/// every sector crosses its PIO writes.
pub fn factory_fixture_on<D: BlockDevice>(mut dev: D) -> Fixture<D> {
    let parts = plan().expect("layout plan");
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

pub fn run<D: BlockDevice>(
    fx: &mut Fixture<D>,
) -> (Result<flow::Loaded, FlowError>, Vec<Event>, Vec<u8>) {
    let mut dest = vec![0u8; 4 * 1024 * 1024];
    let mut events = Vec::new();
    let got = flow::run(&mut fx.dev, &mut dest, &mut |e| events.push(e));
    (got, events, dest)
}

pub fn schedule_trial<D: BlockDevice>(fx: &mut Fixture<D>, floor: u32, tries: u8) {
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
