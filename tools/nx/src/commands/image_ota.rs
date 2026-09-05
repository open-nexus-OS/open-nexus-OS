// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx image ota` (TASK-0260/0034/0321, RFC-0089 §3/§11/§12) —
//! emits the signed `.nxs` v2 component container: `boot-image` (or the
//! RFC-0090 `boot-image-delta`) first, then — with `--bundle-set` — the
//! `system-volume` + `bundle…` components of RFC-0089 §12.4. Split from
//! `image.rs` under the structure ratchet; deterministic tar (no wall
//! clock in any byte), publisher signature over the raw manifest.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/image_cli.rs, tests/image_volume_cli.rs
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use std::fs::File;
use std::io::Write;

use serde_json::json;

use crate::cli::ImageOtaArgs;
use crate::commands::image::{hex, make_nxbd, read_kernel, read_seed, sha256};
use crate::commands::image_volume as volume;
use crate::error::{ExecResult, ExitClass, NxError};

// ------------------------------------------------------------------- ota --

pub(crate) fn handle_ota(args: ImageOtaArgs) -> ExecResult {
    use ed25519_dalek::{Signer, SigningKey};

    let publisher_seed = read_seed(&args.sign_publisher)?;
    let os_seed = read_seed(&args.sign_os)?;
    let kernel = read_kernel(&args.kernel)?;
    let nxbd = make_nxbd(&kernel, &args.build_id, args.rollback_index, &os_seed)?;

    // RFC-0090 delta emission: the payload becomes the deterministic
    // `.nxdelta` stream base -> kernel; the component's size/sha256
    // describe the STREAM (signature-bound), the NXBD keeps target truth.
    let (kind, name, payload_path, payload) = match &args.delta_from {
        Some(base_path) => {
            let base = read_kernel(base_path)?;
            let delta = nxdelta::make::make(&base, &kernel);
            (
                updates::component_set::KIND_BOOT_IMAGE_DELTA,
                "boot-image-delta",
                "boot.img.nxdelta",
                delta,
            )
        }
        None => (updates::component_set::KIND_BOOT_IMAGE, "boot-image", "boot.img", kernel.clone()),
    };

    // Component list (RFC-0089 §3/§12.4 ordering): the boot image first;
    // with `--bundle-set` the system-volume + bundle components follow.
    let mut components = vec![volume::Component {
        kind,
        name: name.to_string(),
        payload_path: payload_path.to_string(),
        payload: payload.clone(),
        kind_data: nxbd.to_vec(),
    }];
    let mut bundle_set = serde_json::Value::Null;
    if let Some(dir) = &args.bundle_set {
        let bundles = volume::load_bundle_dirs(dir)?;
        let vol = volume::build_volume_bytes(&bundles)?;
        let index =
            storage::pkgimg_bundles::parse_index(&vol, &storage::pkgimg::PkgImgCaps::default())
                .map_err(|e| {
                    NxError::new(ExitClass::Internal, format!("image: volume index: {e}"))
                })?;
        let nxsv = volume::make_nxsv(
            &vol,
            &index,
            sha256(&kernel),
            &args.build_id,
            args.rollback_index,
            &os_seed,
        )?;
        // TASK-0035 P2: with `--reuse-from` only CHANGED windows ship; the
        // reuse manifest names both halves (what the device copies from its
        // active volume is exactly `reused`).
        // TASK-0035 P3: `--delta-from-volume` turns every changed bundle with a
        // same-named predecessor into a kind-4 stream against that window.
        let (set, reused, deltas) = if let Some(base) = &args.delta_from_volume {
            let (base_vol, base_index) = volume::active_volume_from(base)?;
            volume::bundle_set_components_delta(&vol, &index, &nxsv, &base_vol, &base_index)
        } else {
            let base_index = match &args.reuse_from {
                Some(base) => Some(volume::active_index_from(base)?),
                None => None,
            };
            let (set, reused) =
                volume::bundle_set_components_reusing(&vol, &index, &nxsv, base_index.as_ref());
            (set, reused, Vec::new())
        };
        bundle_set = json!({
            "bundles": index.bundles.iter().map(|b| format!("{}@{}", b.bundle, b.version)).collect::<Vec<_>>(),
            "shipped": set.iter().skip(1).map(|c| c.name.clone()).collect::<Vec<_>>(),
            "reused": reused,
            "delta": deltas,
            "volume_bytes": vol.len(),
            "volume_sha256": hex(&sha256(&vol)),
        });
        components.extend(set);
    }

    // manifest.nxo — capnp ComponentManifest (RFC-0089 §3); kindData
    // carries the signed NXBD / NXSV verbatim, so the publisher signature
    // transitively binds them.
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
        let mut list = root.init_components(components.len() as u32);
        for (i, comp) in components.iter().enumerate() {
            let mut c = list.reborrow().get(i as u32);
            c.set_kind(comp.kind);
            c.set_name(&comp.name);
            c.set_size(comp.payload.len() as u64);
            c.set_sha256(&sha256(&comp.payload));
            c.set_payload_path(&comp.payload_path);
            c.set_kind_data(&comp.kind_data);
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
    for comp in &components {
        append_tar(&mut tar, &comp.payload_path, &comp.payload)?;
    }
    tar.into_inner()
        .and_then(|f| f.sync_all())
        .map_err(|err| NxError::new(ExitClass::Internal, format!("image: tar: {err}")))?;

    Ok((
        ExitClass::Success,
        format!("image: ota container written to {} ({name})", args.out.display()),
        args.json,
        Some(json!({
            "out": args.out.display().to_string(),
            "build_id": args.build_id,
            "rollback_index": args.rollback_index,
            "publisher": hex(&publisher_pub),
            "kind": name,
            "payload_bytes": payload.len(),
            "kernel_bytes": kernel.len(),
            "kernel_sha256": hex(&sha256(&kernel)),
            "components": components.len(),
            "bundle_set": bundle_set,
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
