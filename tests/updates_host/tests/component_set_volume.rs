// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host proofs for bundle-set staging (RFC-0089 §12.4, TASK-0321
//! P3) through the REAL engine + the REAL `VolumeAssembler` over a
//! sector-fake device: a set `[boot-image, system-volume, bundle…]`
//! assembles the inactive volume byte-identical to the host build,
//! reusing unshipped bundles from the ACTIVE volume (hashed against the
//! NEW index); the reject matrix (`order`, `bundle-not-in-index`,
//! `volume-digest`, `volume-binding`); the power-cut matrix (a cut at any
//! op leaves the inactive NXSV invalid; a restage converges); volume-only
//! sets pair with the active NXBD digest.
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 7 integration tests
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use storage::pkgimg::{PkgImgCaps, PkgImgFileSpec};
use storage::pkgimg_bundles::{build_volume, parse_index, BundleLaunch, VolumeIndex};
use updates::component_set::{
    verify_and_apply, RejectReason, KIND_BOOT_IMAGE, KIND_BUNDLE, KIND_SYSTEM_VOLUME,
};
use updates::system_set::Ed25519Verifier;
use updates::volume_apply::{
    VolumeAssembler, VolumeDev, VolumeEvents, SECTOR, VOLUME_START_SECTOR,
};

const OS_SEED: [u8; 32] = [0x0b; 32];
const PUBLISHER_SEED: [u8; 32] = [0x07; 32];

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

fn trust() -> Vec<[u8; 32]> {
    vec![SigningKey::from_bytes(&PUBLISHER_SEED).verifying_key().to_bytes()]
}

// ------------------------------------------------------------ fixtures --

fn specs(a_version: &str, a_payload: &[u8]) -> Vec<PkgImgFileSpec> {
    vec![
        PkgImgFileSpec::new("alpha", a_version, "payload.elf", a_payload),
        PkgImgFileSpec::new("alpha", a_version, "manifest.nxb", b"manifest-alpha"),
        PkgImgFileSpec::new("beta", "1.0.0", "payload.elf", &[0x22; 7000]),
        PkgImgFileSpec::new("beta", "1.0.0", "manifest.nxb", b"manifest-beta"),
        PkgImgFileSpec::new("gamma", "1.0.0", "data.bin", &[0x33; 100]),
    ]
}

fn launch(a_version: &str) -> Vec<BundleLaunch> {
    vec![
        BundleLaunch {
            bundle: "alpha".into(),
            version: a_version.into(),
            stack_pages: 8,
            global_pointer: 0x1000,
        },
        BundleLaunch {
            bundle: "beta".into(),
            version: "1.0.0".into(),
            stack_pages: 4,
            global_pointer: 0x2000,
        },
    ]
}

fn volume(a_version: &str, a_payload: &[u8]) -> (Vec<u8>, VolumeIndex) {
    let bytes =
        build_volume(&specs(a_version, a_payload), &launch(a_version), &PkgImgCaps::default())
            .expect("volume");
    let index = parse_index(&bytes, &PkgImgCaps::default()).expect("index");
    (bytes, index)
}

fn kernel() -> Vec<u8> {
    (0..150_007u32).map(|i| (i % 253) as u8).collect()
}

fn signed_nxbd(kernel: &[u8], build: &str, rb: u32) -> [u8; 512] {
    let (_pk, id) = bootfmt::nxbd::pubkey_id_for_seed(&OS_SEED);
    bootfmt::nxbd::sign(
        &bootfmt::nxbd::Nxbd {
            rollback_index: rb,
            image_size: kernel.len() as u64,
            image_sha256: sha256(kernel),
            build_id: bootfmt::nxbd::Nxbd::build_id_from(build),
            load_addr: 0x8020_0000,
            pubkey_id: id,
        },
        &OS_SEED,
    )
}

fn signed_nxsv(
    vol: &[u8],
    index: &VolumeIndex,
    boot_sha: [u8; 32],
    build: &str,
    rb: u32,
) -> [u8; 512] {
    let (_pk, id) = bootfmt::nxbd::pubkey_id_for_seed(&OS_SEED);
    let index_len = index.superblock.index_end();
    bootfmt::nxsv::sign(
        &bootfmt::nxsv::Nxsv {
            rollback_index: rb,
            volume_size: vol.len() as u64,
            volume_sha256: sha256(vol),
            build_id: bootfmt::nxbd::Nxbd::build_id_from(build),
            pubkey_id: id,
            boot_image_sha256: boot_sha,
            index_len: index_len as u32,
            index_sha256: sha256(&vol[..index_len]),
        },
        &OS_SEED,
    )
}

/// (kind, name, payload path, payload, kind_data)
type Comp = (u8, String, String, Vec<u8>, Vec<u8>);

fn manifest_bytes(build: &str, rb: u32, comps: &[Comp]) -> Vec<u8> {
    let publisher_pub = SigningKey::from_bytes(&PUBLISHER_SEED).verifying_key().to_bytes();
    let mut builder = capnp::message::Builder::new_default();
    let mut root =
        builder.init_root::<updates::system_set_capnp::component_manifest::Builder<'_>>();
    root.set_schema_version(2);
    root.set_publisher_key_id(&publisher_pub[..8]);
    root.set_build_id(build);
    root.set_rollback_index(rb);
    let mut list = root.init_components(comps.len() as u32);
    for (i, (kind, name, path, payload, kd)) in comps.iter().enumerate() {
        let mut c = list.reborrow().get(i as u32);
        c.set_kind(*kind);
        c.set_name(name);
        c.set_size(payload.len() as u64);
        c.set_sha256(&sha256(payload));
        c.set_payload_path(path);
        c.set_kind_data(kd);
    }
    let mut out = Vec::new();
    capnp::serialize::write_message(&mut out, &builder).expect("capnp");
    out
}

fn tar_entry(out: &mut Vec<u8>, name: &str, data: &[u8]) {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    let size = format!("{:011o}\0", data.len());
    header[124..136].copy_from_slice(size.as_bytes());
    header[156] = b'0';
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    let chk = format!("{sum:06o}\0 ");
    header[148..156].copy_from_slice(chk.as_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    let pad = data.len().div_ceil(512) * 512 - data.len();
    out.extend(std::iter::repeat_n(0u8, pad));
}

fn container(build: &str, rb: u32, comps: &[Comp]) -> Vec<u8> {
    let manifest = manifest_bytes(build, rb, comps);
    let sig = SigningKey::from_bytes(&PUBLISHER_SEED).sign(&manifest).to_bytes();
    let mut tar = Vec::new();
    tar_entry(&mut tar, "manifest.nxo", &manifest);
    tar_entry(&mut tar, "manifest.sig.ed25519", &sig);
    for (_, _, path, payload, _) in comps {
        tar_entry(&mut tar, path, payload);
    }
    tar.extend(std::iter::repeat_n(0u8, 1024));
    tar
}

fn window(vol: &[u8], index: &VolumeIndex, name: &str) -> Vec<u8> {
    let b = index
        .bundle(name, index.bundles.iter().find(|x| x.bundle == name).unwrap().version.as_str())
        .unwrap();
    let start = index.superblock.data_offset + b.data_offset as usize;
    vol[start..start + b.data_len as usize].to_vec()
}

fn boot_comp(k: &[u8], build: &str, rb: u32) -> Comp {
    (
        KIND_BOOT_IMAGE,
        "boot-image".into(),
        "boot.img".into(),
        k.to_vec(),
        signed_nxbd(k, build, rb).to_vec(),
    )
}

fn volume_comp(vol: &[u8], index: &VolumeIndex, nxsv: &[u8; 512]) -> Comp {
    (
        KIND_SYSTEM_VOLUME,
        "system-volume".into(),
        "system.idx".into(),
        vol[..index.superblock.index_end()].to_vec(),
        nxsv.to_vec(),
    )
}

fn bundle_comp(vol: &[u8], index: &VolumeIndex, name: &str) -> Comp {
    (
        KIND_BUNDLE,
        format!("{name}@x"),
        format!("bundles/{name}.bin"),
        window(vol, index, name),
        Vec::new(),
    )
}

// --------------------------------------------------------------- device --

/// Sector fake with an op gate (power cut after N accepted ops).
struct VecDev {
    sectors: Vec<u8>,
    ops: usize,
    fail_after: Option<usize>,
}

impl VecDev {
    fn new(sectors: u64) -> Self {
        Self { sectors: vec![0u8; sectors as usize * SECTOR], ops: 0, fail_after: None }
    }
    fn with_volume(vol: &[u8], nxsv: &[u8; 512]) -> Self {
        let mut d = Self::new(4096);
        let start = VOLUME_START_SECTOR as usize * SECTOR;
        d.sectors[start..start + vol.len()].copy_from_slice(vol);
        d.sectors[..512].copy_from_slice(nxsv);
        d
    }
    fn gate(&mut self) -> Result<(), RejectReason> {
        if let Some(limit) = self.fail_after {
            if self.ops >= limit {
                return Err(RejectReason::Io);
            }
        }
        self.ops += 1;
        Ok(())
    }
    fn body(&self, len: usize) -> &[u8] {
        let start = VOLUME_START_SECTOR as usize * SECTOR;
        &self.sectors[start..start + len]
    }
}

impl VolumeDev for VecDev {
    fn block_count(&self) -> u64 {
        (self.sectors.len() / SECTOR) as u64
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), RejectReason> {
        self.gate()?;
        let off = lba as usize * SECTOR;
        buf.copy_from_slice(&self.sectors[off..off + buf.len()]);
        Ok(())
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), RejectReason> {
        self.gate()?;
        let off = lba as usize * SECTOR;
        self.sectors[off..off + buf.len()].copy_from_slice(buf);
        Ok(())
    }
    fn sync(&mut self) -> Result<(), RejectReason> {
        self.gate()
    }
}

#[derive(Default)]
struct Recorder {
    log: Vec<String>,
}
impl VolumeEvents for Recorder {
    fn volume_verified(&mut self, build_id: &str, bundles: usize) {
        self.log.push(format!("volume {build_id} {bundles}"));
    }
    fn bundle_verified(&mut self, bundle: &str, version: &str) {
        self.log.push(format!("bundle {bundle}@{version}"));
    }
    fn bundle_reused(&mut self, bundle: &str, version: &str, _sha8: &[u8; 8]) {
        self.log.push(format!("reused {bundle}@{version}"));
    }
}

/// The active volume (v1 everywhere) and the NEW set: alpha 2.0.0 shipped,
/// beta/gamma unchanged (reused from active).
struct Scene {
    k: Vec<u8>,
    new_vol: Vec<u8>,
    new_index: VolumeIndex,
    new_nxsv: [u8; 512],
    active: VecDev,
}

fn scene() -> Scene {
    let k = kernel();
    let (old_vol, old_index) = volume("1.0.0", &[0x11; 5000]);
    let old_nxsv = signed_nxsv(&old_vol, &old_index, [0xaa; 32], "old", 1);
    let (new_vol, new_index) = volume("2.0.0", &[0x44; 6000]);
    let new_nxsv = signed_nxsv(&new_vol, &new_index, sha256(&k), "newB", 2);
    Scene { k, new_vol, new_index, new_nxsv, active: VecDev::with_volume(&old_vol, &old_nxsv) }
}

/// The OS `StageSink` shape: kind 1 goes to a boot-slot sink (a discarding
/// fake here — the boot slot has its own proofs), kinds 2/6 and the set
/// commit go to the REAL assembler.
struct DualSink {
    volume: VolumeAssembler<VecDev, VecDev, Recorder>,
    boot_component: bool,
}

impl updates::component_set::ComponentSink for DualSink {
    fn begin(&mut self, meta: &updates::component_set::ComponentMeta) -> Result<(), RejectReason> {
        self.boot_component = meta.kind == KIND_BOOT_IMAGE;
        if self.boot_component {
            return Ok(());
        }
        self.volume.begin(meta)
    }
    fn chunk(&mut self, offset: u64, bytes: &[u8]) -> Result<(), RejectReason> {
        if self.boot_component {
            return Ok(());
        }
        self.volume.chunk(offset, bytes)
    }
    fn finish(&mut self, meta: &updates::component_set::ComponentMeta) -> Result<(), RejectReason> {
        if self.boot_component {
            return Ok(());
        }
        self.volume.finish(meta)
    }
    fn commit_set(&mut self) -> Result<(), RejectReason> {
        self.volume.commit_set()
    }
}

fn run(
    set: &[u8],
    inactive: VecDev,
    active: Option<VecDev>,
    pair: Option<[u8; 32]>,
) -> (Result<(), RejectReason>, VecDev, Vec<String>) {
    let mut sink = DualSink {
        volume: VolumeAssembler::new(inactive, active, pair, Recorder::default()),
        boot_component: false,
    };
    let outcome =
        verify_and_apply(set, &Ed25519Verifier, &trust(), 0, &mut sink, &mut || {}).map(|_| ());
    let log = sink.volume.events().log.clone();
    (outcome, sink.volume.into_inactive(), log)
}

#[test]
fn full_set_assembles_byte_identical_and_reuses_unshipped_bundles() {
    let s = scene();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
        ],
    );
    let (outcome, inactive, log) = run(&set, VecDev::new(4096), Some(s.active), None);
    assert_eq!(outcome, Ok(()));
    assert_eq!(inactive.body(s.new_vol.len()), &s.new_vol[..], "device volume == host build");
    assert_eq!(&inactive.sectors[..512], &s.new_nxsv[..], "NXSV landed last");
    assert_eq!(
        log,
        vec!["volume newB 3", "bundle alpha@2.0.0", "reused beta@1.0.0", "reused gamma@1.0.0"]
    );
    let back =
        parse_index(inactive.body(s.new_index.superblock.index_end()), &PkgImgCaps::default())
            .expect("index on disk parses");
    assert_eq!(back.bundles.len(), 3);
}

#[test]
fn test_reject_order() {
    let s = scene();
    // bundle before the volume
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
        ],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::Order));
    // boot image after the volume
    let set = container(
        "newB",
        2,
        &[volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv), boot_comp(&s.k, "newB", 2)],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::Order));
    // two volumes
    let set = container(
        "newB",
        2,
        &[
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
        ],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::Order));
}

#[test]
fn test_reject_bundle_not_in_index() {
    let s = scene();
    let mut foreign = bundle_comp(&s.new_vol, &s.new_index, "alpha");
    foreign.3.push(0x99); // not a window of the index
    let set = container(
        "newB",
        2,
        &[boot_comp(&s.k, "newB", 2), volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv), foreign],
    );
    let (outcome, inactive, _) = run(&set, VecDev::new(4096), None, None);
    assert_eq!(outcome, Err(RejectReason::BundleNotInIndex));
    assert!(inactive.sectors[..512].iter().all(|&b| b == 0), "NXSV never landed");
    // Reuse needs the bundle in the ACTIVE volume: an active without beta.
    let (lonely_vol, lonely_index) = {
        let specs = vec![PkgImgFileSpec::new("alpha", "1.0.0", "payload.elf", &[0x11; 5000])];
        let bytes = build_volume(&specs, &[], &PkgImgCaps::default()).unwrap();
        let idx = parse_index(&bytes, &PkgImgCaps::default()).unwrap();
        (bytes, idx)
    };
    let lonely_nxsv = signed_nxsv(&lonely_vol, &lonely_index, [0xaa; 32], "old", 1);
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
        ],
    );
    let (outcome, inactive, _) =
        run(&set, VecDev::new(4096), Some(VecDev::with_volume(&lonely_vol, &lonely_nxsv)), None);
    assert_eq!(outcome, Err(RejectReason::BundleNotInIndex));
    assert!(inactive.sectors[..512].iter().all(|&b| b == 0));
}

#[test]
fn test_reject_volume_digest_on_tampered_active_bytes() {
    let mut s = scene();
    // Flip one byte inside beta's window in the ACTIVE volume: the reuse
    // copy re-hashes against the NEW index and must refuse.
    let (old_vol, old_index) = volume("1.0.0", &[0x11; 5000]);
    let beta = old_index.bundle("beta", "1.0.0").unwrap();
    let off = VOLUME_START_SECTOR as usize * SECTOR
        + old_index.superblock.data_offset
        + beta.data_offset as usize
        + 10;
    s.active.sectors[off] ^= 0x01;
    let _ = old_vol;
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
        ],
    );
    let (outcome, inactive, _) = run(&set, VecDev::new(4096), Some(s.active), None);
    assert_eq!(outcome, Err(RejectReason::VolumeDigest));
    assert!(inactive.sectors[..512].iter().all(|&b| b == 0), "NXSV never landed");
}

#[test]
fn test_reject_volume_binding() {
    let s = scene();
    // Unpaired: the NXSV names a different boot image than the set carries.
    let wrong = signed_nxsv(&s.new_vol, &s.new_index, [0xee; 32], "newB", 2);
    let set = container(
        "newB",
        2,
        &[boot_comp(&s.k, "newB", 2), volume_comp(&s.new_vol, &s.new_index, &wrong)],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::VolumeBinding));
    // Build mismatch between NXSV and manifest.
    let other = signed_nxsv(&s.new_vol, &s.new_index, sha256(&s.k), "other", 2);
    let set = container(
        "newB",
        2,
        &[boot_comp(&s.k, "newB", 2), volume_comp(&s.new_vol, &s.new_index, &other)],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::VolumeBinding));
}

#[test]
fn volume_only_set_pairs_with_the_active_nxbd() {
    let s = scene();
    let set = container(
        "newB",
        2,
        &[
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
        ],
    );
    // The sink pairs with the digest the OS read from the ACTIVE NXBD.
    let active2 = {
        let (old_vol, old_index) = volume("1.0.0", &[0x11; 5000]);
        VecDev::with_volume(&old_vol, &signed_nxsv(&old_vol, &old_index, [0xaa; 32], "old", 1))
    };
    let ok = run(&set, VecDev::new(4096), Some(s.active), Some(sha256(&s.k)));
    assert_eq!(ok.0, Ok(()));
    let bad = run(&set, VecDev::new(4096), Some(active2), Some([0x01; 32]));
    assert_eq!(bad.0, Err(RejectReason::VolumeBinding));
}

#[test]
fn power_cut_matrix_never_leaves_a_valid_nxsv_and_restage_converges() {
    let s0 = scene();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s0.k, "newB", 2),
            volume_comp(&s0.new_vol, &s0.new_index, &s0.new_nxsv),
            bundle_comp(&s0.new_vol, &s0.new_index, "alpha"),
        ],
    );
    // Count the ops of a clean run, then cut at every point before it.
    let (outcome, done, _) = run(&set, VecDev::new(4096), Some(scene().active), None);
    assert_eq!(outcome, Ok(()));
    let total_ops = done.ops;
    assert!(total_ops > 10);
    for cut in 1..total_ops {
        let mut inactive = VecDev::new(4096);
        inactive.fail_after = Some(cut);
        let (outcome, torn, _) = run(&set, inactive, Some(scene().active), None);
        assert_eq!(outcome, Err(RejectReason::Io), "cut at {cut}");
        // Invariant: a VALID NXSV exists only over a COMPLETE, byte-identical
        // volume (the descriptor is written after every verification; only
        // the trailing sync can still fail). Any earlier cut leaves the
        // descriptor zeroed/invalid — never a half volume behind a valid NXSV.
        if bootfmt::nxsv::decode(&torn.sectors[..512]).is_ok() {
            assert_eq!(
                torn.body(s0.new_vol.len()),
                &s0.new_vol[..],
                "cut at {cut}: a valid NXSV must sit on the complete volume"
            );
            assert_eq!(&torn.sectors[..512], &s0.new_nxsv[..]);
        }
        // Restage on the torn device converges to the host build.
        let mut again = torn;
        again.fail_after = None;
        let (outcome, fixed, _) = run(&set, again, Some(scene().active), None);
        assert_eq!(outcome, Ok(()), "restage after cut at {cut}");
        assert_eq!(fixed.body(s0.new_vol.len()), &s0.new_vol[..]);
        assert_eq!(&fixed.sectors[..512], &s0.new_nxsv[..]);
    }
}
