// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx image fixtures` (TASK-0179) — the deterministic OTA
//! fixture authority for the QEMU lanes. Emits the small trusted/
//! untrusted/tampered/downgrade `.nxs` containers (real signatures, real
//! rejects — the payloads are patterned bytes because those lanes prove
//! the VERIFY path, never a boot) plus the REAL-kernel `os-B.nxs` (the
//! crown-proof flip boots it; a build-id trailer sector makes the bytes
//! genuinely different from the running image), then packs everything
//! into an nxfs data-partition seed under `/updates/` so
//! `image build --data` ships them at factory time.
//! OWNERS: @tools-team @runtime
//! STATUS: Experimental (TASK-0179)
//! TEST_COVERAGE: tests/image_cli.rs fixtures roundtrip; QEMU ota lanes
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;

use crate::cli_image::ImageFixturesArgs;
use crate::commands::image::{read_seed, sha256, FileBlockDevice};
use crate::commands::image_ota::append_tar;
use crate::error::{ExecResult, ExitClass, NxError};

/// Well-known UNTRUSTED publisher seed (test key, deliberately public —
/// its verifying key is NOT in policies/update-trust.toml, which is the
/// entire point of the deny lane).
const UNTRUSTED_PUBLISHER_SEED: [u8; 32] = [9u8; 32];

/// Small patterned payload for the verify-path fixtures (never booted).
fn fixture_payload() -> Vec<u8> {
    (0..256 * 1024u32).map(|i| (i % 249) as u8).collect()
}

struct ContainerSpec<'a> {
    name: &'a str,
    build_id: String,
    rollback_index: u32,
    payload: Vec<u8>,
    publisher_seed: [u8; 32],
    /// Flip one payload byte AFTER signing (digest deny lane).
    tamper: bool,
    /// NXBD `load_addr`. The small verify-path fixtures deliberately carry
    /// an INVALID address: they are staged into a real slot partition, so
    /// if a lane ever left one selected the loader MUST refuse it loudly
    /// (`nxboot: verify FAIL (slot=<s> nxbd)` → fallback) instead of
    /// jumping into 256 KiB of pattern bytes. Only `os-B.nxs` — the real
    /// kernel the crown lane flips to — carries the true entry address.
    load_addr: u64,
    /// RFC-0090 (TASK-0034): `Some(base)` emits a `boot-image-delta`
    /// container (kind 3) whose payload is the `.nxdelta` stream
    /// base -> `payload`; the NXBD still describes the TARGET. The device
    /// binds the stream to its ACTIVE NXBD, so a base other than the
    /// RUNNING image is the `delta-base` deny fixture.
    delta_from: Option<Vec<u8>>,
}

/// The kernel entry contract (RFC-0089 §5 / ADR-0059).
const REAL_LOAD_ADDR: u64 = 0x8020_0000;
/// Deliberately not a load address (see `ContainerSpec::load_addr`).
const UNBOOTABLE_LOAD_ADDR: u64 = 0;

pub(crate) fn handle_fixtures(args: ImageFixturesArgs) -> ExecResult {
    let os_seed = read_seed(&args.sign_os)?;
    let publisher_seed = read_seed(&args.sign_publisher)?;

    // os-B: the running kernel + a 512-byte build-id trailer — genuinely
    // different image bytes with a genuinely different digest.
    let kernel = std::fs::read(&args.kernel).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("fixtures: read {}: {err}", args.kernel.display()),
        )
    })?;
    // The crown proof asserts a DIFFERENT build id in the uart, and the
    // loader prints only the first 8 chars — so os-B must differ inside
    // that window. Swap the `dev-` prefix for `otaB` (deterministic, still
    // tied to the kernel hash).
    let suffix = args.build_id.strip_prefix("dev-").unwrap_or(&args.build_id);
    let build_b = format!("otaB{}", truncate_id(suffix, 26));
    // TASK-0034: the delta fixtures bind to the RUNNING image's bytes.
    let kernel_for_delta = kernel.clone();
    let mut kernel_b = kernel;
    let mut trailer = [0u8; 512];
    trailer[..7].copy_from_slice(b"NXBUILD");
    let id_bytes = build_b.as_bytes();
    trailer[16..16 + id_bytes.len()].copy_from_slice(id_bytes);
    kernel_b.extend_from_slice(&trailer);

    let base = fixture_payload();
    let specs = [
        ContainerSpec {
            name: "os-fixture-b.nxs",
            build_id: "fixt-b".into(),
            rollback_index: 1,
            payload: base.clone(),
            publisher_seed,
            tamper: false,
            load_addr: UNBOOTABLE_LOAD_ADDR,
            delta_from: None,
        },
        ContainerSpec {
            name: "os-fixture-untrusted.nxs",
            build_id: "fixt-ut".into(),
            rollback_index: 1,
            payload: base.clone(),
            publisher_seed: UNTRUSTED_PUBLISHER_SEED,
            tamper: false,
            load_addr: UNBOOTABLE_LOAD_ADDR,
            delta_from: None,
        },
        ContainerSpec {
            name: "os-fixture-tampered.nxs",
            build_id: "fixt-tam".into(),
            rollback_index: 1,
            payload: base.clone(),
            publisher_seed,
            tamper: true,
            load_addr: UNBOOTABLE_LOAD_ADDR,
            delta_from: None,
        },
        ContainerSpec {
            name: "os-fixture-downgrade.nxs",
            build_id: "fixt-old".into(),
            // BELOW the factory floor (see `--factory-floor`): this is what
            // makes it a real downgrade instead of an equal index.
            rollback_index: 0,
            payload: base,
            publisher_seed,
            tamper: false,
            load_addr: UNBOOTABLE_LOAD_ADDR,
            delta_from: None,
        },
        // TASK-0034 (RFC-0090) delta lane fixtures. The happy-path target
        // is a 64 KiB prefix of the RUNNING image plus a small literal
        // tail — one real COPY window read back from the active slot and
        // one ADD, small enough for the headless time budget; the
        // unbootable load_addr keeps a stray selection loudly refused.
        ContainerSpec {
            name: "os-fixture-delta.nxs",
            build_id: "fixt-dl".into(),
            rollback_index: 1,
            payload: Vec::new(), // filled below (needs the kernel bytes)
            publisher_seed,
            tamper: false,
            load_addr: UNBOOTABLE_LOAD_ADDR,
            delta_from: None, // filled below
        },
        // Deny fixture: a delta whose base is NOT the running image — the
        // device must reject `delta-base` before any write.
        ContainerSpec {
            name: "os-fixture-deltabase.nxs",
            build_id: "fixt-dbx".into(),
            rollback_index: 1,
            payload: Vec::new(), // filled below
            publisher_seed,
            tamper: false,
            load_addr: UNBOOTABLE_LOAD_ADDR,
            delta_from: None, // filled below
        },
        ContainerSpec {
            name: "os-B.nxs",
            build_id: build_b.clone(),
            // ABOVE the factory floor so the crown lane's health commit
            // RAISES it (RFC-0089 §10 commit rung).
            rollback_index: 2,
            payload: kernel_b,
            publisher_seed,
            tamper: false,
            load_addr: REAL_LOAD_ADDR,
            delta_from: None,
        },
    ];

    // Fill the delta specs (they need the kernel bytes read above).
    let mut specs = specs;
    {
        let mut delta_target = kernel_for_delta[..kernel_for_delta.len().min(64 * 1024)].to_vec();
        delta_target.extend_from_slice(&fixture_payload()[..4096]);
        for spec in specs.iter_mut() {
            if spec.name == "os-fixture-delta.nxs" {
                spec.payload = delta_target.clone();
                spec.delta_from = Some(kernel_for_delta.clone());
            } else if spec.name == "os-fixture-deltabase.nxs" {
                spec.payload = delta_target.clone();
                // Deliberately the WRONG base: the stream binds to these
                // bytes, the device's active NXBD names the real kernel.
                spec.delta_from = Some(fixture_payload());
            }
        }
    }

    // Assemble the nxfs seed (identical geometry to the mounted data
    // partition: 512-byte sectors, layout-SSOT size).
    let bytes = args
        .data_mib
        .checked_mul(1024 * 1024)
        .ok_or_else(|| NxError::new(ExitClass::ValidationReject, "fixtures: data size overflow"))?;
    if let Some(parent) = args.data_out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let dev = FileBlockDevice::create(&args.data_out, bytes).map_err(|err| {
        NxError::new(
            ExitClass::Internal,
            format!("fixtures: create {}: {err}", args.data_out.display()),
        )
    })?;
    let mut fs =
        nxfs::Nxfs::mkfs(dev, nxfs::MkfsOptions { uuid: *b"nexus-data-vol01", journal_blocks: 64 })
            .map_err(|err| {
                NxError::new(ExitClass::Internal, format!("fixtures: mkfs ({err:?})"))
            })?;
    fs.mkdir("/updates")
        .map_err(|err| NxError::new(ExitClass::Internal, format!("fixtures: mkdir ({err:?})")))?;

    let mut emitted = Vec::new();
    for spec in specs {
        let container = build_container(&spec, &os_seed)?;
        // SELF-VERIFY at factory time: run the container through the REAL
        // device-side engine against the REAL baked anchor. A tool that
        // emits containers the device rejects is worse than no tool — this
        // turns "the OS said sig" (a 10-minute QEMU round trip) into a
        // build error with the exact reason. The untrusted fixture is the
        // one that MUST be rejected; every other one MUST verify.
        let expect_reject = spec.publisher_seed == UNTRUSTED_PUBLISHER_SEED || spec.tamper;
        let outcome = self_verify(&container);
        match (expect_reject, outcome) {
            (false, Ok(())) | (true, Err(_)) => {}
            (false, Err(reason)) => {
                return Err(NxError::new(
                    ExitClass::Internal,
                    format!(
                        "fixtures: {} does not verify against the device anchor ({})",
                        spec.name,
                        reason.label()
                    ),
                ))
            }
            (true, Ok(())) => {
                return Err(NxError::new(
                    ExitClass::Internal,
                    format!("fixtures: {} was ACCEPTED but must be rejected", spec.name),
                ))
            }
        }
        let path = format!("/updates/{}", spec.name);
        fs.create(&path).map_err(|err| {
            NxError::new(ExitClass::Internal, format!("fixtures: create {path} ({err:?})"))
        })?;
        fs.write(&path, 0, &container).map_err(|err| {
            NxError::new(ExitClass::Internal, format!("fixtures: write {path} ({err:?})"))
        })?;
        // Same three signing inputs the device prints on a `sig` reject —
        // side by side they turn "who changed the bytes?" into a diff.
        let inputs = updates::component_set::describe(&container);
        emitted.push(json!({
            "name": spec.name,
            "bytes": container.len(),
            "sha256_8": crate::commands::image::hex(&sha256(&container))[..16].to_string(),
            "sig_inputs": inputs.map(|(len, m4, s4, hint)| json!({
                "msg": len,
                "m": crate::commands::image::hex(&m4),
                "s": crate::commands::image::hex(&s4),
                "k": crate::commands::image::hex(&hint),
            })),
        }));
    }
    fs.sync()
        .map_err(|err| NxError::new(ExitClass::Internal, format!("fixtures: sync ({err:?})")))?;
    fs.write_checkpoint().map_err(|err| {
        NxError::new(ExitClass::Internal, format!("fixtures: checkpoint ({err:?})"))
    })?;

    Ok((
        ExitClass::Success,
        format!("image: fixtures packed into {} (5 containers)", args.data_out.display()),
        args.json,
        Some(json!({
            "data_out": args.data_out.display().to_string(),
            "build_b": build_b,
            "containers": emitted,
        })),
    ))
}

/// Runs a container through the device-side verify+apply engine with a
/// discarding sink — proves the bytes the device will see, not a
/// host-only approximation.
fn self_verify(container: &[u8]) -> Result<(), updates::component_set::RejectReason> {
    struct NullSink;
    impl updates::component_set::ComponentSink for NullSink {
        fn begin(
            &mut self,
            _meta: &updates::component_set::ComponentMeta,
        ) -> Result<(), updates::component_set::RejectReason> {
            Ok(())
        }
        fn chunk(
            &mut self,
            _offset: u64,
            _bytes: &[u8],
        ) -> Result<(), updates::component_set::RejectReason> {
            Ok(())
        }
        fn finish(
            &mut self,
            _meta: &updates::component_set::ComponentMeta,
        ) -> Result<(), updates::component_set::RejectReason> {
            Ok(())
        }
    }
    updates::component_set::verify_and_apply(
        container,
        &updates::system_set::Ed25519Verifier,
        updates::trust::BAKED_PUBLISHERS,
        0,
        &mut NullSink,
        &mut || {},
    )
    .map(|_| ())
}

/// One deterministic `.nxs` v2 container (mirrors `nx image ota`).
fn build_container(spec: &ContainerSpec<'_>, os_seed: &[u8; 32]) -> Result<Vec<u8>, NxError> {
    let (_pk, pubkey_id) = bootfmt::nxbd::pubkey_id_for_seed(os_seed);
    let nxbd = bootfmt::nxbd::sign(
        &bootfmt::nxbd::Nxbd {
            rollback_index: spec.rollback_index,
            image_size: spec.payload.len() as u64,
            image_sha256: sha256(&spec.payload),
            build_id: bootfmt::nxbd::Nxbd::build_id_from(&spec.build_id),
            load_addr: spec.load_addr,
            pubkey_id,
        },
        os_seed,
    );

    // RFC-0090: a delta spec ships the `.nxdelta` stream as its payload
    // entry; the NXBD above still describes the TARGET (spec.payload).
    let (kind, name, payload_path, entry_payload) = match &spec.delta_from {
        Some(base) => (
            updates::component_set::KIND_BOOT_IMAGE_DELTA,
            "boot-image-delta",
            "boot.img.nxdelta",
            nxdelta::make::make(base, &spec.payload),
        ),
        None => (
            updates::component_set::KIND_BOOT_IMAGE,
            "boot-image",
            "boot.img",
            spec.payload.clone(),
        ),
    };

    let publisher_key = SigningKey::from_bytes(&spec.publisher_seed);
    let publisher_pub = publisher_key.verifying_key().to_bytes();
    let manifest_bytes = {
        let mut builder = capnp::message::Builder::new_default();
        let mut root =
            builder.init_root::<updates::system_set_capnp::component_manifest::Builder<'_>>();
        root.set_schema_version(2);
        root.set_publisher_key_id(&publisher_pub[..8]);
        root.set_build_id(&spec.build_id);
        root.set_rollback_index(spec.rollback_index);
        let mut list = root.init_components(1);
        {
            let mut c = list.reborrow().get(0);
            c.set_kind(kind);
            c.set_name(name);
            c.set_size(entry_payload.len() as u64);
            c.set_sha256(&sha256(&entry_payload));
            c.set_payload_path(payload_path);
            c.set_kind_data(&nxbd);
        }
        let mut out = Vec::new();
        capnp::serialize::write_message(&mut out, &builder)
            .map_err(|err| NxError::new(ExitClass::Internal, format!("fixtures: capnp: {err}")))?;
        out
    };
    let signature = publisher_key.sign(&manifest_bytes).to_bytes();

    let mut payload = entry_payload;
    if spec.tamper {
        // Post-signature flip: signature + manifest stay valid, the
        // streamed digest gate must catch it.
        let mid = payload.len() / 2;
        payload[mid] ^= 0x40;
    }

    let mut tar_bytes = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_bytes);
        append_tar(&mut tar, "manifest.nxo", &manifest_bytes)?;
        append_tar(&mut tar, "manifest.sig.ed25519", &signature)?;
        append_tar(&mut tar, payload_path, &payload)?;
        tar.finish()
            .map_err(|err| NxError::new(ExitClass::Internal, format!("fixtures: tar: {err}")))?;
    }
    Ok(tar_bytes)
}

fn truncate_id(id: &str, max: usize) -> &str {
    &id[..id.len().min(max)]
}
