// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `.nxs` v2 verify+apply host proofs (TASK-0179 PR-1/PR-3):
//! accept vector, the full stable-reject matrix (RFC-0089 §8 vocabulary),
//! and the power-cut/idempotency matrix against a block-fake sink that
//! enforces the NXBD-last commit discipline.
//! OWNERS: @runtime @security

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use updates::component_set::{
    verify_and_apply, ComponentMeta, ComponentSink, RejectReason, KIND_BOOT_IMAGE,
};
use updates::system_set::Ed25519Verifier;

const PUBLISHER_SEED: [u8; 32] = [7u8; 32];
const OS_SEED: [u8; 32] = [11u8; 32];
const STRANGER_SEED: [u8; 32] = [9u8; 32];

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    out
}

fn trust_of(seed: &[u8; 32]) -> Vec<[u8; 32]> {
    vec![SigningKey::from_bytes(seed).verifying_key().to_bytes()]
}

fn signed_nxbd(kernel: &[u8], build: &str, rollback_index: u32) -> [u8; 512] {
    let (_pk, id) = bootfmt::nxbd::pubkey_id_for_seed(&OS_SEED);
    bootfmt::nxbd::sign(
        &bootfmt::nxbd::Nxbd {
            rollback_index,
            image_size: kernel.len() as u64,
            image_sha256: sha256(kernel),
            build_id: bootfmt::nxbd::Nxbd::build_id_from(build),
            load_addr: 0x8020_0000,
            pubkey_id: id,
        },
        &OS_SEED,
    )
}

fn manifest_bytes(build: &str, rollback_index: u32, kernel: &[u8], nxbd: &[u8]) -> Vec<u8> {
    let publisher_pub = SigningKey::from_bytes(&PUBLISHER_SEED).verifying_key().to_bytes();
    let mut builder = capnp::message::Builder::new_default();
    let mut root =
        builder.init_root::<updates::system_set_capnp::component_manifest::Builder<'_>>();
    root.set_schema_version(2);
    root.set_publisher_key_id(&publisher_pub[..8]);
    root.set_build_id(build);
    root.set_rollback_index(rollback_index);
    let mut list = root.init_components(1);
    {
        let mut c = list.reborrow().get(0);
        c.set_kind(KIND_BOOT_IMAGE);
        c.set_name("boot-image");
        c.set_size(kernel.len() as u64);
        c.set_sha256(&sha256(kernel));
        c.set_payload_path("boot.img");
        c.set_kind_data(nxbd);
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
    // Checksum field: spaces while summing, then the octal sum.
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    let chk = format!("{sum:06o}\0 ");
    header[148..156].copy_from_slice(chk.as_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    let pad = data.len().div_ceil(512) * 512 - data.len();
    out.extend(std::iter::repeat_n(0u8, pad));
}

fn container(build: &str, rollback_index: u32, kernel: &[u8]) -> Vec<u8> {
    let nxbd = signed_nxbd(kernel, build, rollback_index);
    let manifest = manifest_bytes(build, rollback_index, kernel, &nxbd);
    let sig = SigningKey::from_bytes(&PUBLISHER_SEED).sign(&manifest).to_bytes();
    let mut tar = Vec::new();
    tar_entry(&mut tar, "manifest.nxo", &manifest);
    tar_entry(&mut tar, "manifest.sig.ed25519", &sig);
    tar_entry(&mut tar, "boot.img", kernel);
    tar.extend(std::iter::repeat_n(0u8, 1024));
    tar
}

fn kernel_fixture() -> Vec<u8> {
    (0..200_003u32).map(|i| (i % 251) as u8).collect()
}

/// Block-fake sink modelling a slot partition: sector 0 = descriptor,
/// body from sector 8. Enforces NXBD-last and supports fault injection
/// (power cut after N accepted operations).
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
        self.descriptor = None; // invalidate the commit point FIRST
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
        // Readback verification then descriptor LAST.
        assert_eq!(sha256(&self.body), meta.sha256, "sink readback");
        self.descriptor = Some(meta.kind_data.clone());
        self.log.push("finish");
        Ok(())
    }
}

fn apply(
    container: &[u8],
    trust: &[[u8; 32]],
    floor: u32,
    sink: &mut SlotFake,
) -> Result<updates::component_set::ManifestV2, RejectReason> {
    verify_and_apply(container, &Ed25519Verifier, trust, floor, sink, &mut || {})
}

#[test]
fn accept_vector_applies_body_then_descriptor() {
    let kernel = kernel_fixture();
    let nxs = container("build-B", 1, &kernel);
    let mut sink = SlotFake::default();
    let manifest = apply(&nxs, &trust_of(&PUBLISHER_SEED), 0, &mut sink).expect("accept");
    assert_eq!(manifest.build_id, "build-B");
    assert_eq!(manifest.rollback_index, 1);
    assert_eq!(manifest.component_count, 1);
    assert_eq!(sink.body, kernel);
    let descriptor = sink.descriptor.expect("descriptor written");
    let (nxbd, _sig) = bootfmt::nxbd::decode(&descriptor).expect("verbatim NXBD");
    assert_eq!(nxbd.build_id_str(), "build-B");
    assert_eq!(sink.log.first().copied(), Some("begin"));
    assert_eq!(sink.log.last().copied(), Some("finish"));
    // The digest gate sits BETWEEN the last chunk and finish.
    assert!(sink.log[1..sink.log.len() - 1].iter().all(|s| *s == "chunk"));
}

#[test]
fn test_reject_untrusted_publisher() {
    let nxs = container("build-B", 1, &kernel_fixture());
    let mut sink = SlotFake::default();
    // Device anchor holds a DIFFERENT key: the hint matches nothing.
    let err = apply(&nxs, &trust_of(&STRANGER_SEED), 0, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::UntrustedPublisher);
    assert!(sink.log.is_empty(), "no byte may move");
}

#[test]
fn test_reject_sig() {
    // Well-formed manifest with the RIGHT publisher hint but a signature
    // from a stranger key: the anchor is selected, the verify fails.
    let kernel = kernel_fixture();
    let nxbd = signed_nxbd(&kernel, "build-B", 1);
    let manifest = manifest_bytes("build-B", 1, &kernel, &nxbd);
    let sig = SigningKey::from_bytes(&STRANGER_SEED).sign(&manifest).to_bytes();
    let mut tar = Vec::new();
    tar_entry(&mut tar, "manifest.nxo", &manifest);
    tar_entry(&mut tar, "manifest.sig.ed25519", &sig);
    tar_entry(&mut tar, "boot.img", &kernel);
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let mut sink = SlotFake::default();
    let err = apply(&tar, &trust_of(&PUBLISHER_SEED), 0, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::Sig);
    assert!(sink.log.is_empty());
}

#[test]
fn test_reject_digest_and_no_commit_point() {
    let kernel = kernel_fixture();
    let mut nxs = container("build-B", 1, &kernel);
    // Flip a byte in the LAST payload chunk (tar layout: payload is the
    // final entry) — the manifest signature does not cover payload bytes,
    // so this must die at the streamed-digest gate.
    let len = nxs.len();
    nxs[len - 1400] ^= 0x40;
    let mut sink = SlotFake::default();
    let err = apply(&nxs, &trust_of(&PUBLISHER_SEED), 0, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::Digest);
    assert!(sink.descriptor.is_none(), "commit point never written");
    assert!(!sink.body.is_empty(), "body bytes may land pre-digest (NXBD-last covers them)");
}

#[test]
fn test_reject_downgrade() {
    let nxs = container("build-old", 0, &kernel_fixture());
    let mut sink = SlotFake::default();
    let err = apply(&nxs, &trust_of(&PUBLISHER_SEED), 1, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::Downgrade);
    assert!(sink.log.is_empty());
}

#[test]
fn test_reject_component_kind_unsupported() {
    let kernel = kernel_fixture();
    let nxbd = signed_nxbd(&kernel, "build-B", 1);
    let publisher = SigningKey::from_bytes(&PUBLISHER_SEED);
    let publisher_pub = publisher.verifying_key().to_bytes();
    let mut builder = capnp::message::Builder::new_default();
    {
        let mut root =
            builder.init_root::<updates::system_set_capnp::component_manifest::Builder<'_>>();
        root.set_schema_version(2);
        root.set_publisher_key_id(&publisher_pub[..8]);
        root.set_build_id("build-B");
        root.set_rollback_index(1);
        let mut list = root.init_components(1);
        let mut c = list.reborrow().get(0);
        // Kind 5 (`rotation-record`) is reserved (§4) and has no dispatch:
        // kinds 2/4/6 are the Phase B seam (TASK-0321 P3, see
        // component_set_volume.rs), 3 the RFC-0090 delta.
        c.set_kind(5);
        c.set_name("delta");
        c.set_size(kernel.len() as u64);
        c.set_sha256(&sha256(&kernel));
        c.set_payload_path("boot.img");
        c.set_kind_data(&nxbd);
    }
    let mut manifest = Vec::new();
    capnp::serialize::write_message(&mut manifest, &builder).expect("capnp");
    let sig = publisher.sign(&manifest).to_bytes();
    let mut tar = Vec::new();
    tar_entry(&mut tar, "manifest.nxo", &manifest);
    tar_entry(&mut tar, "manifest.sig.ed25519", &sig);
    tar_entry(&mut tar, "boot.img", &kernel);
    tar.extend(std::iter::repeat_n(0u8, 1024));

    let mut sink = SlotFake::default();
    let err = apply(&tar, &trust_of(&PUBLISHER_SEED), 0, &mut sink).expect_err("deny");
    assert_eq!(err, RejectReason::KindUnsupported);
    assert!(sink.log.is_empty());
}

#[test]
fn test_reject_path_extras_and_order() {
    let kernel = kernel_fixture();
    // Unknown extra entry after the payload.
    let mut nxs = container("build-B", 1, &kernel);
    let end = nxs.len() - 1024;
    let mut extra = Vec::new();
    tar_entry(&mut extra, "sneaky.bin", b"x");
    nxs.splice(end..end, extra);
    let mut sink = SlotFake::default();
    assert_eq!(
        apply(&nxs, &trust_of(&PUBLISHER_SEED), 0, &mut sink).expect_err("deny"),
        RejectReason::Path
    );

    // Absolute/parent traversal in an entry name.
    let nxbd = signed_nxbd(&kernel, "build-B", 1);
    let manifest = manifest_bytes("build-B", 1, &kernel, &nxbd);
    let sig = SigningKey::from_bytes(&PUBLISHER_SEED).sign(&manifest).to_bytes();
    let mut tar = Vec::new();
    tar_entry(&mut tar, "manifest.nxo", &manifest);
    tar_entry(&mut tar, "manifest.sig.ed25519", &sig);
    tar_entry(&mut tar, "../boot.img", &kernel);
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let mut sink = SlotFake::default();
    assert_eq!(
        apply(&tar, &trust_of(&PUBLISHER_SEED), 0, &mut sink).expect_err("deny"),
        RejectReason::Path
    );
}

#[test]
fn test_reject_bounds_nxbd_binding() {
    let kernel = kernel_fixture();
    // NXBD signed for a DIFFERENT build id than the manifest claims.
    let nxbd = signed_nxbd(&kernel, "other-build", 1);
    let manifest = manifest_bytes("build-B", 1, &kernel, &nxbd);
    let sig = SigningKey::from_bytes(&PUBLISHER_SEED).sign(&manifest).to_bytes();
    let mut tar = Vec::new();
    tar_entry(&mut tar, "manifest.nxo", &manifest);
    tar_entry(&mut tar, "manifest.sig.ed25519", &sig);
    tar_entry(&mut tar, "boot.img", &kernel);
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let mut sink = SlotFake::default();
    assert_eq!(
        apply(&tar, &trust_of(&PUBLISHER_SEED), 0, &mut sink).expect_err("deny"),
        RejectReason::Bounds
    );
    assert!(sink.log.is_empty(), "binding checked before any byte moves");
}

#[test]
fn test_reject_io_surfaces_sink_errors() {
    let nxs = container("build-B", 1, &kernel_fixture());
    let mut sink = SlotFake { fail_after: Some(0), ..Default::default() };
    assert_eq!(
        apply(&nxs, &trust_of(&PUBLISHER_SEED), 0, &mut sink).expect_err("deny"),
        RejectReason::Io
    );
}

#[test]
fn power_cut_matrix_restage_converges_and_slot_never_half_valid() {
    let kernel = kernel_fixture();
    let nxs = container("build-B", 1, &kernel);
    let trust = trust_of(&PUBLISHER_SEED);

    // Probe the op count of a clean run.
    let mut probe = SlotFake::default();
    apply(&nxs, &trust, 0, &mut probe).expect("clean");
    let total_ops = probe.ops_accepted;
    assert!(total_ops >= 3, "begin + chunks + finish");

    for cut_after in 0..total_ops {
        // Run 1: power cut after `cut_after` accepted sink ops.
        let mut sink = SlotFake { fail_after: Some(cut_after), ..Default::default() };
        let err = apply(&nxs, &trust, 0, &mut sink).expect_err("cut run fails");
        assert_eq!(err, RejectReason::Io);
        // Invariant: the commit point is NEVER present after a torn stage
        // (descriptor only lands in finish, the final op).
        assert!(sink.descriptor.is_none(), "cut_after={cut_after}: half-valid slot");

        // Run 2 (restage on the SAME sink state): converges to the exact
        // clean-run outcome.
        sink.fail_after = None;
        let manifest = apply(&nxs, &trust, 0, &mut sink).expect("restage");
        assert_eq!(manifest.build_id, "build-B");
        assert_eq!(sink.body, kernel, "cut_after={cut_after}: body converged");
        assert!(sink.descriptor.is_some(), "cut_after={cut_after}: descriptor landed");
    }
}
