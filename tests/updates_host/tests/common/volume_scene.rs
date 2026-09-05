// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared scene for the bundle-set engine tests (TASK-0321 P3,
//! TASK-0035): v1/v2 host volumes, signed NXBD/NXSV, `.nxs` containers,
//! the `VecDev` sector fake with an op gate (power-cut matrix), the
//! `Recorder` events sink and the `DualSink` (kind 1 discarded, kinds 2/6
//! to the real assembler). Used via `#[path]` by component_set_volume.rs
//! and component_set_bundle_delta.rs.
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: helper module (no tests of its own)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

#![allow(dead_code, unused_imports)]

pub use ed25519_dalek::{Signer, SigningKey};
pub use sha2::{Digest, Sha256};
pub use storage::pkgimg::{PkgImgCaps, PkgImgFileSpec};
pub use storage::pkgimg_bundles::{build_volume, parse_index, BundleLaunch, VolumeIndex};
pub use updates::component_set::{
    verify_and_apply, RejectReason, KIND_BOOT_IMAGE, KIND_BUNDLE, KIND_BUNDLE_DELTA,
    KIND_SYSTEM_VOLUME,
};
pub use updates::system_set::Ed25519Verifier;
pub use updates::volume_apply::{
    VolumeAssembler, VolumeDev, VolumeEvents, SECTOR, VOLUME_START_SECTOR,
};

pub const OS_SEED: [u8; 32] = [0x0b; 32];
pub const PUBLISHER_SEED: [u8; 32] = [0x07; 32];

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

pub fn trust() -> Vec<[u8; 32]> {
    vec![SigningKey::from_bytes(&PUBLISHER_SEED).verifying_key().to_bytes()]
}

// ------------------------------------------------------------ fixtures --

pub fn specs(a_version: &str, a_payload: &[u8]) -> Vec<PkgImgFileSpec> {
    vec![
        PkgImgFileSpec::new("alpha", a_version, "payload.elf", a_payload),
        PkgImgFileSpec::new("alpha", a_version, "manifest.nxb", b"manifest-alpha"),
        PkgImgFileSpec::new("beta", "1.0.0", "payload.elf", &[0x22; 7000]),
        PkgImgFileSpec::new("beta", "1.0.0", "manifest.nxb", b"manifest-beta"),
        PkgImgFileSpec::new("gamma", "1.0.0", "data.bin", &[0x33; 100]),
    ]
}

pub fn launch(a_version: &str) -> Vec<BundleLaunch> {
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

pub fn volume(a_version: &str, a_payload: &[u8]) -> (Vec<u8>, VolumeIndex) {
    let bytes =
        build_volume(&specs(a_version, a_payload), &launch(a_version), &PkgImgCaps::default())
            .expect("volume");
    let index = parse_index(&bytes, &PkgImgCaps::default()).expect("index");
    (bytes, index)
}

pub fn kernel() -> Vec<u8> {
    (0..150_007u32).map(|i| (i % 253) as u8).collect()
}

pub fn signed_nxbd(kernel: &[u8], build: &str, rb: u32) -> [u8; 512] {
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

pub fn signed_nxsv(
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
pub type Comp = (u8, String, String, Vec<u8>, Vec<u8>);

pub fn manifest_bytes(build: &str, rb: u32, comps: &[Comp]) -> Vec<u8> {
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

pub fn tar_entry(out: &mut Vec<u8>, name: &str, data: &[u8]) {
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

pub fn container(build: &str, rb: u32, comps: &[Comp]) -> Vec<u8> {
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

pub fn window(vol: &[u8], index: &VolumeIndex, name: &str) -> Vec<u8> {
    let b = index
        .bundle(name, index.bundles.iter().find(|x| x.bundle == name).unwrap().version.as_str())
        .unwrap();
    let start = index.superblock.data_offset + b.data_offset as usize;
    vol[start..start + b.data_len as usize].to_vec()
}

pub fn boot_comp(k: &[u8], build: &str, rb: u32) -> Comp {
    (
        KIND_BOOT_IMAGE,
        "boot-image".into(),
        "boot.img".into(),
        k.to_vec(),
        signed_nxbd(k, build, rb).to_vec(),
    )
}

pub fn volume_comp(vol: &[u8], index: &VolumeIndex, nxsv: &[u8; 512]) -> Comp {
    (
        KIND_SYSTEM_VOLUME,
        "system-volume".into(),
        "system.idx".into(),
        vol[..index.superblock.index_end()].to_vec(),
        nxsv.to_vec(),
    )
}

pub fn bundle_comp(vol: &[u8], index: &VolumeIndex, name: &str) -> Comp {
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
#[derive(Clone)]
pub struct VecDev {
    pub sectors: Vec<u8>,
    pub ops: usize,
    pub fail_after: Option<usize>,
}

impl VecDev {
    pub fn new(sectors: u64) -> Self {
        Self { sectors: vec![0u8; sectors as usize * SECTOR], ops: 0, fail_after: None }
    }
    pub fn with_volume(vol: &[u8], nxsv: &[u8; 512]) -> Self {
        let mut d = Self::new(4096);
        let start = VOLUME_START_SECTOR as usize * SECTOR;
        d.sectors[start..start + vol.len()].copy_from_slice(vol);
        d.sectors[..512].copy_from_slice(nxsv);
        d
    }
    pub fn gate(&mut self) -> Result<(), RejectReason> {
        if let Some(limit) = self.fail_after {
            if self.ops >= limit {
                return Err(RejectReason::Io);
            }
        }
        self.ops += 1;
        Ok(())
    }
    pub fn body(&self, len: usize) -> &[u8] {
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
pub struct Recorder {
    pub log: Vec<String>,
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
    fn restage_resume(&mut self, completed: usize, total: usize) {
        self.log.push(format!("resume {completed}/{total}"));
    }
}

/// The active volume (v1 everywhere) and the NEW set: alpha 2.0.0 shipped,
/// beta/gamma unchanged (reused from active).
pub struct Scene {
    pub k: Vec<u8>,
    pub new_vol: Vec<u8>,
    pub new_index: VolumeIndex,
    pub new_nxsv: [u8; 512],
    pub active: VecDev,
}

pub fn scene() -> Scene {
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
pub struct DualSink {
    pub volume: VolumeAssembler<VecDev, VecDev, Recorder>,
    pub boot_component: bool,
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

pub fn run(
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
