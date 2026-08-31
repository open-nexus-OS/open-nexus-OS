// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx image build/verify/patch/ota` (TASK-0260, RFC-0089) — the
//! host-side disk/OTA artifact authority. Layout comes from THE shared
//! table (`storage::layout`), GPT bytes from the shared writer/parser
//! (`storage::gpt` — tool and OS agree by construction), NXBD/BSB from
//! `bootfmt`. Deterministic end to end: no wall clock in any written
//! byte, build-twice ⇒ identical images. NXBD-LAST discipline on
//! build/patch: the image body lands before the descriptor, so an
//! interrupted write leaves an INVALID slot, never a half-bootable one.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/image_cli.rs
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::cli::{
    ImageAction, ImageArgs, ImageBuildArgs, ImageOtaArgs, ImagePatchArgs, ImageVerifyArgs,
};
use crate::error::{ExecResult, ExitClass, NxError};

use storage::gpt::{
    find_partition_named, parse_gpt, write_gpt, Partition, GUID_NEXUS_BOOT, GUID_NEXUS_BSB,
    GUID_NEXUS_DATA, GUID_NEXUS_STATE,
};
use storage::layout::{plan, NEXUS_DISK_BYTES};
use storage::{BlockDevice, BlockError};

const SECTOR: usize = 512;
/// Boot image payload starts at slot-partition sector 8 (RFC-0089 §5).
const IMAGE_START_SECTOR: u64 = 8;

pub(crate) fn handle_image(args: ImageArgs) -> ExecResult {
    match args.action {
        ImageAction::Build(a) => handle_build(a),
        ImageAction::Verify(a) => handle_verify(a),
        ImageAction::Patch(a) => handle_patch(a),
        ImageAction::Ota(a) => handle_ota(a),
        ImageAction::Fixtures(a) => crate::commands::image_fixtures::handle_fixtures(a),
    }
}

// ---------------------------------------------------------------- device --

/// File-backed 512-byte BlockDevice (sparse; bulk run overrides).
pub(crate) struct FileBlockDevice {
    file: RefCell<File>,
    sectors: u64,
}

impl FileBlockDevice {
    pub(crate) fn create(path: &Path, bytes: u64) -> std::io::Result<Self> {
        let file =
            OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?;
        file.set_len(bytes)?;
        Ok(Self { file: RefCell::new(file), sectors: bytes / SECTOR as u64 })
    }

    fn open_rw(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let bytes = file.metadata()?.len();
        Ok(Self { file: RefCell::new(file), sectors: bytes / SECTOR as u64 })
    }

    fn open_ro(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().read(true).open(path)?;
        let bytes = file.metadata()?.len();
        Ok(Self { file: RefCell::new(file), sectors: bytes / SECTOR as u64 })
    }

    fn io(&self, first: u64, len: usize) -> Result<(), BlockError> {
        if first >= self.sectors || (len as u64).div_ceil(SECTOR as u64) > self.sectors - first {
            return Err(BlockError::OutOfRange);
        }
        Ok(())
    }
}

impl BlockDevice for FileBlockDevice {
    fn block_size(&self) -> usize {
        SECTOR
    }
    fn block_count(&self) -> u64 {
        self.sectors
    }
    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.read_blocks(block_idx, &mut buf[..SECTOR])
    }
    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        let sector = &buf[..SECTOR];
        self.io(block_idx, sector.len())?;
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(block_idx * SECTOR as u64)).map_err(|_| BlockError::IoError)?;
        file.write_all(sector).map_err(|_| BlockError::IoError)
    }
    fn read_blocks(&self, first_block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.io(first_block, buf.len())?;
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(first_block * SECTOR as u64)).map_err(|_| BlockError::IoError)?;
        file.read_exact(buf).map_err(|_| BlockError::IoError)
    }
    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.io(first_block, buf.len())?;
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(first_block * SECTOR as u64)).map_err(|_| BlockError::IoError)?;
        file.write_all(buf).map_err(|_| BlockError::IoError)
    }
    fn sync(&mut self) -> Result<(), BlockError> {
        self.file.borrow_mut().sync_all().map_err(|_| BlockError::IoError)
    }
}

// --------------------------------------------------------------- helpers --

pub(crate) fn read_seed(path: &Path) -> Result<[u8; 32], NxError> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        NxError::new(ExitClass::MissingDependency, format!("image: read {}: {err}", path.display()))
    })?;
    decode_hex32(text.trim()).ok_or_else(|| {
        NxError::new(ExitClass::ValidationReject, "image: key file must hold 64 hex chars")
    })
}

fn decode_hex32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn read_kernel(path: &Path) -> Result<Vec<u8>, NxError> {
    let bytes = std::fs::read(path).map_err(|err| {
        NxError::new(ExitClass::MissingDependency, format!("image: read {}: {err}", path.display()))
    })?;
    if bytes.is_empty() {
        return Err(NxError::new(ExitClass::ValidationReject, "image: kernel image is empty"));
    }
    Ok(bytes)
}

fn part(parts: &[Partition], guid: &[u8; 16], name: &str) -> Result<Partition, NxError> {
    find_partition_named(parts, guid, name).ok_or_else(|| {
        NxError::new(ExitClass::ValidationReject, format!("image: partition `{name}` missing"))
    })
}

fn slot_budget_sectors(p: &Partition) -> u64 {
    (p.last_lba - p.first_lba + 1).saturating_sub(IMAGE_START_SECTOR)
}

fn make_nxbd(
    kernel: &[u8],
    build_id: &str,
    rollback_index: u32,
    os_seed: &[u8; 32],
) -> Result<[u8; SECTOR], NxError> {
    if build_id.len() > 32 || !build_id.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "image: --build-id must be ≤ 32 printable ASCII chars",
        ));
    }
    let (_pk, pubkey_id) = bootfmt::nxbd::pubkey_id_for_seed(os_seed);
    let desc = bootfmt::nxbd::Nxbd {
        rollback_index,
        image_size: kernel.len() as u64,
        image_sha256: sha256(kernel),
        build_id: bootfmt::nxbd::Nxbd::build_id_from(build_id),
        load_addr: 0x8020_0000,
        pubkey_id,
    };
    Ok(bootfmt::nxbd::sign(&desc, os_seed))
}

/// Writes a boot slot with the NXBD-LAST discipline: zero the descriptor,
/// write the padded image body, THEN the signed descriptor.
fn write_boot_slot(
    dev: &mut FileBlockDevice,
    slot: &Partition,
    kernel: &[u8],
    nxbd: &[u8; SECTOR],
) -> Result<(), NxError> {
    let budget = slot_budget_sectors(slot) * SECTOR as u64;
    if kernel.len() as u64 > budget {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("image: kernel ({} B) exceeds the slot budget ({budget} B)", kernel.len()),
        ));
    }
    let io = |e: BlockError| {
        NxError::new(ExitClass::Internal, format!("image: slot write failed ({e:?})"))
    };
    dev.write_blocks(slot.first_lba, &[0u8; SECTOR]).map_err(io)?;
    let mut padded = kernel.to_vec();
    padded.resize(kernel.len().div_ceil(SECTOR) * SECTOR, 0);
    dev.write_blocks(slot.first_lba + IMAGE_START_SECTOR, &padded).map_err(io)?;
    dev.write_blocks(slot.first_lba, nxbd).map_err(io)?;
    Ok(())
}

fn seed_partition(dev: &mut FileBlockDevice, p: &Partition, path: &Path) -> Result<u64, NxError> {
    let bytes = std::fs::read(path).map_err(|err| {
        NxError::new(ExitClass::MissingDependency, format!("image: read {}: {err}", path.display()))
    })?;
    let budget = (p.last_lba - p.first_lba + 1) * SECTOR as u64;
    if bytes.len() as u64 > budget {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("image: seed for `{}` ({} B) exceeds {budget} B", p.name, bytes.len()),
        ));
    }
    let mut padded = bytes;
    let copied = padded.len() as u64;
    padded.resize(padded.len().div_ceil(SECTOR) * SECTOR, 0);
    dev.write_blocks(p.first_lba, &padded)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("image: seed write ({e:?})")))?;
    Ok(copied)
}

// ----------------------------------------------------------------- build --

fn handle_build(args: ImageBuildArgs) -> ExecResult {
    let os_seed = read_seed(&args.sign)?;
    let kernel = read_kernel(&args.kernel)?;
    let parts = plan().ok_or_else(|| {
        NxError::new(ExitClass::Internal, "image: layout exceeds NEXUS_DISK_BYTES")
    })?;
    if let Some(parent) = args.out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut dev = FileBlockDevice::create(&args.out, NEXUS_DISK_BYTES).map_err(|err| {
        NxError::new(ExitClass::Internal, format!("image: create {}: {err}", args.out.display()))
    })?;
    write_gpt(&mut dev, &parts)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("image: gpt write ({e:?})")))?;

    // Factory BSB: block 0 = seq 1 (active A, committed, floor = the
    // shipped image's rollback index); block 1 stays zeroed (invalid) —
    // the reader rule picks block 0.
    let bsb_part = part(&parts, &GUID_NEXUS_BSB, "bsb")?;
    dev.write_blocks(
        bsb_part.first_lba,
        &bootfmt::bsb::encode(&bootfmt::bsb::Bsb::factory_with_floor(args.rollback_index)),
    )
    .map_err(|e| NxError::new(ExitClass::Internal, format!("image: bsb write ({e:?})")))?;

    // boot-a: NXBD-last; boot-b stays zeroed (invalid by definition).
    let boot_a = part(&parts, &GUID_NEXUS_BOOT, "boot-a")?;
    let nxbd = make_nxbd(&kernel, &args.build_id, args.rollback_index, &os_seed)?;
    write_boot_slot(&mut dev, &boot_a, &kernel, &nxbd)?;

    let mut seeded = Vec::new();
    if let Some(state) = &args.state {
        let p = part(&parts, &GUID_NEXUS_STATE, "state")?;
        seeded.push(json!({ "part": "state", "bytes": seed_partition(&mut dev, &p, state)? }));
    }
    if let Some(data) = &args.data {
        let p = part(&parts, &GUID_NEXUS_DATA, "data")?;
        seeded.push(json!({ "part": "data", "bytes": seed_partition(&mut dev, &p, data)? }));
    }
    dev.sync().map_err(|e| NxError::new(ExitClass::Internal, format!("image: sync ({e:?})")))?;

    let data = json!({
        "out": args.out.display().to_string(),
        "disk_bytes": NEXUS_DISK_BYTES,
        "build_id": args.build_id,
        "rollback_index": args.rollback_index,
        "kernel_bytes": kernel.len(),
        "kernel_sha256": hex(&sha256(&kernel)),
        "seeded": seeded,
    });
    Ok((
        ExitClass::Success,
        format!("image: built {} (build={})", args.out.display(), args.build_id),
        args.json,
        Some(data),
    ))
}

// ---------------------------------------------------------------- verify --

fn handle_verify(args: ImageVerifyArgs) -> ExecResult {
    let pubkey = match (&args.pubkey, &args.key) {
        (Some(hexkey), None) => decode_hex32(hexkey).ok_or_else(|| {
            NxError::new(ExitClass::ValidationReject, "image: --pubkey must be 64 hex chars")
        })?,
        (None, Some(seed_path)) => bootfmt::nxbd::pubkey_id_for_seed(&read_seed(seed_path)?).0,
        _ => {
            return Err(NxError::new(
                ExitClass::Usage,
                "image: exactly one of --pubkey / --key is required",
            ))
        }
    };
    let dev = FileBlockDevice::open_ro(&args.image).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("image: open {}: {err}", args.image.display()),
        )
    })?;
    let parts = parse_gpt(&dev).map_err(|e| {
        NxError::new(ExitClass::ValidationReject, format!("image: gpt parse failed ({e:?})"))
    })?;

    // BSB (double block, reader rule).
    let bsb_part = part(&parts, &GUID_NEXUS_BSB, "bsb")?;
    let mut blocks = [0u8; 2 * SECTOR];
    dev.read_blocks(bsb_part.first_lba, &mut blocks)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("image: bsb read ({e:?})")))?;
    let (bsb, bsb_block) = bootfmt::bsb::pick(&blocks[..SECTOR], &blocks[SECTOR..])
        .ok_or_else(|| NxError::new(ExitClass::ValidationReject, "image: no valid BSB block"))?;

    // boot-a NXBD + streamed digest.
    let boot_a = part(&parts, &GUID_NEXUS_BOOT, "boot-a")?;
    let mut nxbd_sector = [0u8; SECTOR];
    dev.read_blocks(boot_a.first_lba, &mut nxbd_sector)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("image: nxbd read ({e:?})")))?;
    let desc = bootfmt::nxbd::verify(&nxbd_sector, &pubkey).map_err(|e| {
        NxError::new(ExitClass::ValidationReject, format!("image: nxbd verify failed ({e:?})"))
    })?;
    if desc.image_size > slot_budget_sectors(&boot_a) * SECTOR as u64 {
        return Err(NxError::new(ExitClass::ValidationReject, "image: nxbd size exceeds slot"));
    }
    let mut hasher = Sha256::new();
    let mut remaining = desc.image_size as usize;
    let mut lba = boot_a.first_lba + IMAGE_START_SECTOR;
    let mut chunk = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let take = remaining.min(chunk.len());
        let padded = take.div_ceil(SECTOR) * SECTOR;
        dev.read_blocks(lba, &mut chunk[..padded])
            .map_err(|e| NxError::new(ExitClass::Internal, format!("image: read ({e:?})")))?;
        hasher.update(&chunk[..take]);
        remaining -= take;
        lba += (padded / SECTOR) as u64;
    }
    let digest: [u8; 32] = hasher.finalize().into();
    if digest != desc.image_sha256 {
        return Err(NxError::new(ExitClass::ValidationReject, "image: boot-a digest mismatch"));
    }

    let data = json!({
        "image": args.image.display().to_string(),
        "partitions": parts.iter().map(|p| json!({
            "name": p.name, "first_lba": p.first_lba, "last_lba": p.last_lba })).collect::<Vec<_>>(),
        "bsb": {
            "block": bsb_block,
            "seq": bsb.seq,
            "active_slot": match bsb.active_slot { bootfmt::bsb::Slot::A => "a", bootfmt::bsb::Slot::B => "b" },
            "tries_left": bsb.tries_left,
            "health_committed": bsb.health_committed,
            "rollback_min_index": bsb.rollback_min_index,
        },
        "boot_a": {
            "build_id": desc.build_id_str(),
            "rollback_index": desc.rollback_index,
            "image_size": desc.image_size,
            "image_sha256": hex(&desc.image_sha256),
        },
    });
    Ok((
        ExitClass::Success,
        format!("image: verify ok (build={} rbidx={})", desc.build_id_str(), desc.rollback_index),
        args.json,
        Some(data),
    ))
}

// ----------------------------------------------------------------- patch --

fn handle_patch(args: ImagePatchArgs) -> ExecResult {
    if args.part != "boot-a" && args.part != "boot-b" {
        return Err(NxError::new(ExitClass::Usage, "image: --part must be boot-a or boot-b"));
    }
    let os_seed = read_seed(&args.sign)?;
    let kernel = read_kernel(&args.kernel)?;
    let mut dev = FileBlockDevice::open_rw(&args.image).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("image: open {}: {err}", args.image.display()),
        )
    })?;
    let parts = parse_gpt(&dev).map_err(|e| {
        NxError::new(ExitClass::ValidationReject, format!("image: gpt parse failed ({e:?})"))
    })?;
    let slot = part(&parts, &GUID_NEXUS_BOOT, &args.part)?;
    let nxbd = make_nxbd(&kernel, &args.build_id, args.rollback_index, &os_seed)?;
    write_boot_slot(&mut dev, &slot, &kernel, &nxbd)?;
    dev.sync().map_err(|e| NxError::new(ExitClass::Internal, format!("image: sync ({e:?})")))?;
    Ok((
        ExitClass::Success,
        format!("image: patched {} (build={})", args.part, args.build_id),
        args.json,
        Some(json!({
            "image": args.image.display().to_string(),
            "part": args.part,
            "build_id": args.build_id,
            "kernel_bytes": kernel.len(),
        })),
    ))
}

// ------------------------------------------------------------------- ota --

fn handle_ota(args: ImageOtaArgs) -> ExecResult {
    use ed25519_dalek::{Signer, SigningKey};

    let publisher_seed = read_seed(&args.sign_publisher)?;
    let os_seed = read_seed(&args.sign_os)?;
    let kernel = read_kernel(&args.kernel)?;
    let nxbd = make_nxbd(&kernel, &args.build_id, args.rollback_index, &os_seed)?;

    // manifest.nxo — capnp ComponentManifest (RFC-0089 §3): ONE component
    // of kind boot-image (1); kindData carries the signed NXBD verbatim,
    // so the publisher signature transitively binds it.
    let publisher_key = SigningKey::from_bytes(&publisher_seed);
    let publisher_pub = publisher_key.verifying_key().to_bytes();
    let manifest_bytes = {
        let mut builder = capnp::message::Builder::new_default();
        let mut root =
            builder.init_root::<updates::system_set_capnp::component_manifest::Builder>();
        root.set_schema_version(2);
        root.set_publisher_key_id(&publisher_pub[..8]);
        root.set_build_id(&args.build_id);
        root.set_rollback_index(args.rollback_index);
        let mut list = root.init_components(1);
        {
            let mut c = list.reborrow().get(0);
            c.set_kind(1);
            c.set_name("boot-image");
            c.set_size(kernel.len() as u64);
            c.set_sha256(&sha256(&kernel));
            c.set_payload_path("boot.img");
            c.set_kind_data(&nxbd);
        }
        let mut out = Vec::new();
        capnp::serialize::write_message(&mut out, &builder)
            .map_err(|err| NxError::new(ExitClass::Internal, format!("image: capnp: {err}")))?;
        out
    };
    let signature = publisher_key.sign(&manifest_bytes).to_bytes();

    if let Some(parent) = args.out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = File::create(&args.out).map_err(|err| {
        NxError::new(ExitClass::Internal, format!("image: create {}: {err}", args.out.display()))
    })?;
    let mut tar = tar::Builder::new(file);
    append_tar(&mut tar, "manifest.nxo", &manifest_bytes)?;
    append_tar(&mut tar, "manifest.sig.ed25519", &signature)?;
    append_tar(&mut tar, "boot.img", &kernel)?;
    tar.into_inner()
        .and_then(|f| f.sync_all())
        .map_err(|err| NxError::new(ExitClass::Internal, format!("image: tar: {err}")))?;

    Ok((
        ExitClass::Success,
        format!("image: ota container written to {}", args.out.display()),
        args.json,
        Some(json!({
            "out": args.out.display().to_string(),
            "build_id": args.build_id,
            "rollback_index": args.rollback_index,
            "publisher": hex(&publisher_pub),
            "kernel_bytes": kernel.len(),
            "kernel_sha256": hex(&sha256(&kernel)),
        })),
    ))
}

/// Deterministic tar entry (nxs-pack conventions: mode 644, uid/gid 0,
/// mtime 0 — no wall clock in any archive byte).
pub(crate) fn append_tar<W: Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    bytes: &[u8],
) -> Result<(), NxError> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, path, bytes)
        .map_err(|err| NxError::new(ExitClass::Internal, format!("image: tar {path}: {err}")))
}
