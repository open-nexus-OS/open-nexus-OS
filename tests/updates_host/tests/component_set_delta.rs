// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `boot-image-delta` engine + adapter proof floor (RFC-0090,
//! TASK-0034) over the REAL pipeline: `verify_and_apply` drives the
//! generic `DeltaAdapter` (the exact type the OS stage path wraps around
//! its block-device sink) against an in-memory base and the `SlotFake`
//! slot model. Proves reconstruction byte-identity, the `delta-base`
//! deny BEFORE any body write, tampered-stream ⇒ `digest` (the manifest
//! signature covers the delta bytes), truncation ⇒ `delta-format`,
//! NXBD binding, and power-cut restage convergence.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 6 integration tests
//! ADR: docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use updates::component_set::{
    verify_and_apply, ComponentMeta, ComponentSink, RejectReason, KIND_BOOT_IMAGE_DELTA,
};
use updates::delta_apply::{BaseIdentity, BaseRead, DeltaAdapter};
use updates::Ed25519Verifier;

const OS_SEED: [u8; 32] = [0x0b; 32];
const PUBLISHER_SEED: [u8; 32] = [0x07; 32];

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn trust_of(seed: &[u8; 32]) -> [[u8; 32]; 1] {
    [SigningKey::from_bytes(seed).verifying_key().to_bytes()]
}

fn signed_nxbd(target: &[u8], build: &str, rollback_index: u32) -> [u8; 512] {
    let (_pk, id) = bootfmt::nxbd::pubkey_id_for_seed(&OS_SEED);
    bootfmt::nxbd::sign(
        &bootfmt::nxbd::Nxbd {
            rollback_index,
            image_size: target.len() as u64,
            image_sha256: sha256(target),
            build_id: bootfmt::nxbd::Nxbd::build_id_from(build),
            load_addr: 0x8020_0000,
            pubkey_id: id,
        },
        &OS_SEED,
    )
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

/// A kind-3 container: manifest size/sha describe the DELTA payload, the
/// NXBD describes the TARGET (the RFC-0090 reinterpretation, verbatim).
fn delta_container(build: &str, rollback_index: u32, target: &[u8], delta: &[u8]) -> Vec<u8> {
    let nxbd = signed_nxbd(target, build, rollback_index);
    let publisher = SigningKey::from_bytes(&PUBLISHER_SEED);
    let publisher_pub = publisher.verifying_key().to_bytes();
    let mut builder = capnp::message::Builder::new_default();
    {
        let mut root =
            builder.init_root::<updates::system_set_capnp::component_manifest::Builder<'_>>();
        root.set_schema_version(2);
        root.set_publisher_key_id(&publisher_pub[..8]);
        root.set_build_id(build);
        root.set_rollback_index(rollback_index);
        let mut list = root.init_components(1);
        let mut c = list.reborrow().get(0);
        c.set_kind(KIND_BOOT_IMAGE_DELTA);
        c.set_name("boot-image-delta");
        c.set_size(delta.len() as u64);
        c.set_sha256(&sha256(delta));
        c.set_payload_path("boot.img.nxdelta");
        c.set_kind_data(&nxbd);
    }
    let mut manifest = Vec::new();
    capnp::serialize::write_message(&mut manifest, &builder).expect("capnp");
    let sig = publisher.sign(&manifest).to_bytes();
    let mut tar = Vec::new();
    tar_entry(&mut tar, "manifest.nxo", &manifest);
    tar_entry(&mut tar, "manifest.sig.ed25519", &sig);
    tar_entry(&mut tar, "boot.img.nxdelta", delta);
    tar.extend(std::iter::repeat_n(0u8, 1024));
    tar
}

/// In-memory base (the ACTIVE slot's image bytes).
struct VecBase(Vec<u8>);
impl BaseRead for VecBase {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), RejectReason> {
        let start = off as usize;
        let end = start.checked_add(buf.len()).ok_or(RejectReason::Io)?;
        let src = self.0.get(start..end).ok_or(RejectReason::Io)?;
        buf.copy_from_slice(src);
        Ok(())
    }
}

/// Mirror of component_set.rs's SlotFake (per-file test helpers by repo
/// convention): descriptor-last + fault injection.
#[derive(Default)]
struct SlotFake {
    descriptor: Option<Vec<u8>>,
    body: Vec<u8>,
    ops_accepted: usize,
    fail_after: Option<usize>,
    log: Vec<&'static str>,
}

impl SlotFake {
    fn gate(&mut self) -> Result<(), RejectReason> {
        if let Some(limit) = self.fail_after {
            if self.ops_accepted >= limit {
                return Err(RejectReason::Io);
            }
        }
        self.ops_accepted += 1;
        Ok(())
    }
}

impl ComponentSink for SlotFake {
    fn begin(&mut self, _meta: &ComponentMeta) -> Result<(), RejectReason> {
        self.gate()?;
        self.descriptor = None;
        self.body.clear();
        self.log.push("begin");
        Ok(())
    }
    fn chunk(&mut self, offset: u64, bytes: &[u8]) -> Result<(), RejectReason> {
        self.gate()?;
        assert_eq!(offset as usize, self.body.len(), "sequential offsets");
        self.body.extend_from_slice(bytes);
        self.log.push("chunk");
        Ok(())
    }
    fn finish(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        self.gate()?;
        assert_eq!(sha256(&self.body), meta.sha256, "sink readback");
        self.descriptor = Some(meta.kind_data.clone());
        self.log.push("finish");
        Ok(())
    }
}

fn base_fixture() -> Vec<u8> {
    (0..200_003u32).map(|i| (i % 251) as u8).collect()
}

fn target_fixture(base: &[u8]) -> Vec<u8> {
    // The QEMU fixture shape: a COPY window from the base + a literal tail.
    let mut t = base[..64 * 1024].to_vec();
    t.extend((0..4096u32).map(|i| (i % 241) as u8));
    t
}

fn adapter_for(base: &[u8]) -> DeltaAdapter<VecBase, SlotFake> {
    let identity = BaseIdentity { image_sha256: sha256(base), image_size: base.len() as u64 };
    DeltaAdapter::new(VecBase(base.to_vec()), SlotFake::default(), identity)
}

fn apply(
    container: &[u8],
    floor: u32,
    sink: &mut DeltaAdapter<VecBase, SlotFake>,
) -> Result<updates::component_set::ManifestV2, RejectReason> {
    verify_and_apply(
        container,
        &Ed25519Verifier,
        &trust_of(&PUBLISHER_SEED),
        floor,
        sink,
        &mut || {},
    )
}

#[test]
fn delta_container_reconstructs_through_the_engine() {
    let base = base_fixture();
    let target = target_fixture(&base);
    let delta = nxdelta::make::make(&base, &target);
    assert!(delta.len() < target.len() / 4, "the COPY window must actually pay off");
    let nxs = delta_container("build-D", 1, &target, &delta);
    let mut sink = adapter_for(&base);
    let manifest = apply(&nxs, 0, &mut sink).expect("delta stage");
    assert_eq!(manifest.component_kind, KIND_BOOT_IMAGE_DELTA);
    let slot = sink.into_inner();
    assert_eq!(slot.body, target, "reconstruction must be byte-identical");
    let descriptor = slot.descriptor.expect("NXBD landed last");
    let (nxbd, _) = bootfmt::nxbd::decode(&descriptor).expect("target NXBD");
    assert_eq!(nxbd.image_sha256, sha256(&target));
    assert_eq!(slot.log.last(), Some(&"finish"));
}

#[test]
fn test_reject_delta_base_mismatch_before_any_write() {
    let base = base_fixture();
    let target = target_fixture(&base);
    let delta = nxdelta::make::make(&base, &target);
    let nxs = delta_container("build-D", 1, &target, &delta);
    // The device is RUNNING different bytes than the stream was made from.
    let other: Vec<u8> = (0..200_003u32).map(|i| (i % 199) as u8).collect();
    let identity = BaseIdentity { image_sha256: sha256(&other), image_size: other.len() as u64 };
    let mut sink = DeltaAdapter::new(VecBase(other), SlotFake::default(), identity);
    let err = apply(&nxs, 0, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::DeltaBase);
    let slot = sink.into_inner();
    assert_eq!(slot.log, vec!["begin"], "commit point invalidated, but NO body write");
    assert!(slot.body.is_empty());
}

#[test]
fn test_reject_tampered_delta_stream_is_digest() {
    let base = base_fixture();
    let target = target_fixture(&base);
    let mut delta = nxdelta::make::make(&base, &target);
    let nxs_good = delta_container("build-D", 1, &target, &delta);
    let mid = delta.len() / 2;
    delta[mid] ^= 0x40;
    // Splice the tampered payload behind the ORIGINAL (signed) manifest:
    // rebuild the tar with the good manifest bytes but flipped payload.
    let mut nxs = nxs_good.clone();
    // The payload entry sits last; flip the same byte inside the archive.
    let payload_start = nxs.len() - 1024 - delta.len().div_ceil(512) * 512;
    nxs[payload_start + mid] ^= 0x40;
    let mut sink = adapter_for(&base);
    let err = apply(&nxs, 0, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::Digest, "the manifest signature covers the delta bytes");
}

#[test]
fn test_reject_truncated_delta_is_delta_format() {
    let base = base_fixture();
    let target = target_fixture(&base);
    let delta = nxdelta::make::make(&base, &target);
    // Honest truncation: the manifest describes the SHORT payload, so the
    // digest gate passes and the FORMAT gate must be the one that fires.
    let truncated = &delta[..delta.len() - 5];
    let nxs = delta_container("build-D", 1, &target, truncated);
    let mut sink = adapter_for(&base);
    let err = apply(&nxs, 0, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::DeltaFormat);
}

#[test]
fn test_reject_nxbd_binding_mismatch() {
    let base = base_fixture();
    let target = target_fixture(&base);
    let delta = nxdelta::make::make(&base, &target);
    // NXBD build id disagrees with the manifest's.
    let nxbd = signed_nxbd(&target, "other-B", 1);
    let publisher = SigningKey::from_bytes(&PUBLISHER_SEED);
    let publisher_pub = publisher.verifying_key().to_bytes();
    let mut builder = capnp::message::Builder::new_default();
    {
        let mut root =
            builder.init_root::<updates::system_set_capnp::component_manifest::Builder<'_>>();
        root.set_schema_version(2);
        root.set_publisher_key_id(&publisher_pub[..8]);
        root.set_build_id("build-D");
        root.set_rollback_index(1);
        let mut list = root.init_components(1);
        let mut c = list.reborrow().get(0);
        c.set_kind(KIND_BOOT_IMAGE_DELTA);
        c.set_name("boot-image-delta");
        c.set_size(delta.len() as u64);
        c.set_sha256(&sha256(&delta));
        c.set_payload_path("boot.img.nxdelta");
        c.set_kind_data(&nxbd);
    }
    let mut manifest = Vec::new();
    capnp::serialize::write_message(&mut manifest, &builder).expect("capnp");
    let sig = publisher.sign(&manifest).to_bytes();
    let mut tar = Vec::new();
    tar_entry(&mut tar, "manifest.nxo", &manifest);
    tar_entry(&mut tar, "manifest.sig.ed25519", &sig);
    tar_entry(&mut tar, "boot.img.nxdelta", &delta);
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let mut sink = adapter_for(&base);
    let err = apply(&tar, 0, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::Bounds);
    assert!(sink.into_inner().log.is_empty(), "binding fails before begin");
}

#[test]
fn power_cut_restage_converges() {
    let base = base_fixture();
    let target = target_fixture(&base);
    let delta = nxdelta::make::make(&base, &target);
    let nxs = delta_container("build-D", 1, &target, &delta);
    // Cut mid-reconstruction: the slot stays descriptor-less.
    let identity = BaseIdentity { image_sha256: sha256(&base), image_size: base.len() as u64 };
    let cut = SlotFake { fail_after: Some(3), ..SlotFake::default() };
    let mut sink = DeltaAdapter::new(VecBase(base.clone()), cut, identity);
    let err = apply(&nxs, 0, &mut sink).expect_err("cut");
    assert_eq!(err, RejectReason::Io);
    assert!(sink.into_inner().descriptor.is_none(), "torn stage = invalid slot");
    // Restage from scratch converges (RFC-0089 §8 idempotency = resume).
    let mut fresh = adapter_for(&base);
    apply(&nxs, 0, &mut fresh).expect("restage");
    assert_eq!(fresh.into_inner().body, target);
}
