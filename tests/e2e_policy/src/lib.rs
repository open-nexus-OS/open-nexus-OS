//! CONTEXT: Policy end-to-end test harness library
//! INTENT: Policy enforcement testing with bundlemgrd/policyd integration
//! IDL (target): checkCaps(subject,caps), installBundle(name,handle,len), queryCaps(name)
//! DEPS: policyd, bundlemgrd, samgrd (service integration)
//! READINESS: Host backend ready; policy directory configured
//! TESTS: Allow/deny policy checks; bundle capability queries; policy enforcement
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(nexus_env = "host")]
#![forbid(unsafe_code)]

#[cfg(test)]
use std::io::Cursor;
#[cfg(test)]
use std::sync::mpsc;
#[cfg(test)]
use std::thread;
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use anyhow::{Context, Result};
#[cfg(test)]
use capnp::message::{Builder, HeapAllocator, ReaderOptions};
#[cfg(test)]
use capnp::serialize;
#[cfg(test)]
use ed25519_dalek::{Signer, SigningKey};
#[cfg(test)]
use nexus_idl_runtime::bundlemgr_capnp::{
    install_request, install_response, query_request, query_response, InstallError,
};
#[cfg(test)]
use nexus_idl_runtime::manifest_capnp::bundle_manifest;
#[cfg(test)]
use nexus_idl_runtime::policyd_capnp::{check_request, check_response};
#[cfg(test)]
use nexus_ipc::{Client, Wait};
#[cfg(test)]
use repro::capture_bundle_repro_json_with_manifest_digest;
#[cfg(test)]
use sbom::{generate_bundle_sbom_json, BundleSbomInput};
#[cfg(test)]
use sha2::{Digest, Sha256};
#[cfg(test)]
use tempfile::TempDir;

#[cfg(test)]
const POLICY_OPCODE_CHECK: u8 = 1;
#[cfg(test)]
const BUNDLE_OPCODE_INSTALL: u8 = 1;
#[cfg(test)]
const BUNDLE_OPCODE_QUERY: u8 = 2;

#[test]
fn policy_allow_and_deny_roundtrip() -> Result<()> {
    let temp = TempDir::new().context("temp policy dir")?;
    std::fs::write(
        temp.path().join("base.toml"),
        "[allow]\nsamgrd = [\"ipc.core\"]\n\"demo.testsvc\" = []\n",
    )
    .context("write policy")?;
    std::env::set_var("NEXUS_POLICY_DIR", temp.path());

    let anchors = TempDir::new().context("temp anchors dir")?;
    std::env::set_var("NEXUS_ANCHORS_DIR", anchors.path());
    let signing_key = SigningKey::from_bytes(&[3u8; 32]);
    let verifying_key = signing_key.verifying_key();
    std::fs::write(anchors.path().join("policy-e2e.pub"), hex::encode(verifying_key.to_bytes()))
        .context("write anchor key")?;

    let publisher_hex = keystore::device_id(&verifying_key);
    let allowlist_path = temp.path().join("publishers.toml");
    std::fs::write(
        &allowlist_path,
        format!(
            "version = 1\n\n[[publishers]]\nid = \"{publisher}\"\nenabled = true\nallowed_algs = [\"ed25519\"]\nkeys = [\"{pubkey}\"]\n",
            publisher = publisher_hex,
            pubkey = publisher_hex
        ),
    )
    .context("write signing allowlist")?;
    std::env::set_var("NEXUS_SIGNING_ALLOWLIST", &allowlist_path);

    let publisher_bytes: [u8; 16] = hex::decode(&publisher_hex)
        .context("decode publisher id")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("publisher id must be 16 bytes"))?;
    let payload: [u8; 0] = [];
    let signature: [u8; 64] = signing_key.sign(&payload).to_bytes();

    let (samgr_client, mut samgr_server) = samgrd::loopback_transport();
    let samgr_handle = thread::spawn(move || {
        samgrd::run_with_transport(&mut samgr_server).expect("samgrd exits cleanly");
    });
    drop(samgr_client);

    let (bundle_client, mut bundle_server) = bundlemgrd::loopback_transport();
    let store = bundlemgrd::ArtifactStore::new();
    let store_clone = store.clone();
    let (keystore_client, keystore_server) = nexus_ipc::loopback_channel();
    let keystore_handle = thread::spawn(move || {
        let mut transport = keystored::IpcTransport::new(keystore_server);
        keystored::run_with_transport_default_anchors(&mut transport)
            .expect("keystored exits cleanly");
    });
    let bundle_handle = thread::spawn(move || {
        let keystore = Some(bundlemgrd::KeystoreHandle::from_loopback(keystore_client));
        bundlemgrd::run_with_transport(&mut bundle_server, store_clone, keystore, None)
            .expect("bundlemgrd exits cleanly");
    });

    let (policy_client, mut policy_server) = policyd::loopback_transport();
    let (policy_ready_tx, policy_ready_rx) = mpsc::channel();
    let policy_handle = thread::spawn(move || {
        let notifier = policyd::ReadyNotifier::new(move || {
            let _ = policy_ready_tx.send(());
        });
        policyd::run_with_transport_ready(&mut policy_server, notifier)
            .expect("policyd exits cleanly");
    });
    policy_ready_rx.recv_timeout(Duration::from_secs(2)).context("wait policyd ready")?;

    // Install manifests and query required capabilities from bundlemgrd
    let allowed_manifest = allowed_manifest(&publisher_hex, &publisher_bytes, &signature)?;
    let denied_manifest = denied_manifest(&publisher_hex, &publisher_bytes, &signature)?;
    store.insert(1, allowed_manifest.clone());
    store.stage_payload(1, Vec::new());
    store.insert(2, denied_manifest.clone());
    store.stage_payload(2, Vec::new());
    install_bundle(&bundle_client, "samgrd", 1, allowed_manifest.len() as u32)?;
    install_bundle(&bundle_client, "demo.testsvc", 2, denied_manifest.len() as u32)?;
    let allowed_caps = query_caps(&bundle_client, "samgrd")?;
    let denied_caps = query_caps(&bundle_client, "demo.testsvc")?;

    // Allowed subject with matching capability
    let cap_refs: Vec<&str> = allowed_caps.iter().map(String::as_str).collect();
    let (allowed, missing) = check_caps(&policy_client, "samgrd", &cap_refs)?;
    assert!(allowed, "samgrd should be permitted");
    assert!(missing.is_empty());

    // Denied subject with missing capability
    let denied_refs: Vec<&str> = denied_caps.iter().map(String::as_str).collect();
    let (allowed, missing) = check_caps(&policy_client, "demo.testsvc", &denied_refs)?;
    assert!(!allowed, "demo.testsvc should be denied");
    assert!(missing.contains(&"net.client".to_string()));

    // Empty requirements always allowed
    let (allowed, missing) = check_caps(&policy_client, "demo.testsvc", &[])?;
    assert!(allowed, "empty requirements must pass");
    assert!(missing.is_empty());

    // Unknown subject denies any non-empty requirement
    let (allowed, missing) = check_caps(&policy_client, "unknown.service", &["ipc.core"])?;
    assert!(!allowed, "unknown service should be denied");
    assert_eq!(missing, vec!["ipc.core".to_string()]);

    drop(bundle_client);
    drop(policy_client);
    policy_handle.join().expect("policyd thread joins");
    bundle_handle.join().expect("bundlemgrd thread joins");
    keystore_handle.join().expect("keystored thread joins");
    samgr_handle.join().expect("samgrd thread joins");
    std::env::remove_var("NEXUS_POLICY_DIR");
    std::env::remove_var("NEXUS_ANCHORS_DIR");
    std::env::remove_var("NEXUS_SIGNING_ALLOWLIST");
    Ok(())
}

#[cfg(test)]
fn check_caps(
    client: &nexus_ipc::LoopbackClient,
    subject: &str,
    caps: &[&str],
) -> Result<(bool, Vec<String>)> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<check_request::Builder<'_>>();
        request.set_subject(subject);
        let mut list = request.init_required_caps(caps.len() as u32);
        for (idx, cap) in caps.iter().enumerate() {
            list.set(idx as u32, cap);
        }
    }
    let frame = encode_frame(POLICY_OPCODE_CHECK, &message)?;
    client.send(&frame, Wait::Blocking).context("send check")?;
    let response = client.recv(Wait::Blocking).context("recv check")?;
    decode_response(&response)
}

#[cfg(test)]
fn install_bundle(
    client: &nexus_ipc::LoopbackClient,
    name: &str,
    handle: u32,
    len: u32,
) -> Result<()> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<install_request::Builder<'_>>();
        request.set_name(name);
        request.set_bytes_len(len);
        request.set_vmo_handle(handle);
    }
    let frame = encode_frame(BUNDLE_OPCODE_INSTALL, &message)?;
    client.send(&frame, Wait::Blocking).context("send install")?;
    let response = client.recv(Wait::Blocking).context("recv install")?;
    let (opcode, payload) = response.split_first().context("install opcode")?;
    if *opcode != BUNDLE_OPCODE_INSTALL {
        anyhow::bail!("unexpected opcode {opcode}");
    }
    let mut cursor = Cursor::new(payload);
    let message =
        serialize::read_message(&mut cursor, ReaderOptions::new()).context("install decode")?;
    let resp = message.get_root::<install_response::Reader<'_>>().context("install root")?;
    if !resp.get_ok() {
        let err = resp.get_err().unwrap_or(InstallError::Einval);
        anyhow::bail!("install failed: {err:?}");
    }
    Ok(())
}

#[cfg(test)]
fn query_caps(client: &nexus_ipc::LoopbackClient, name: &str) -> Result<Vec<String>> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<query_request::Builder<'_>>();
        request.set_name(name);
    }
    let frame = encode_frame(BUNDLE_OPCODE_QUERY, &message)?;
    client.send(&frame, Wait::Blocking).context("send query")?;
    let response = client.recv(Wait::Blocking).context("recv query")?;
    let (opcode, payload) = response.split_first().context("query opcode")?;
    if *opcode != BUNDLE_OPCODE_QUERY {
        anyhow::bail!("unexpected opcode {opcode}");
    }
    let mut cursor = Cursor::new(payload);
    let message =
        serialize::read_message(&mut cursor, ReaderOptions::new()).context("query decode")?;
    let resp = message.get_root::<query_response::Reader<'_>>().context("query root")?;
    if !resp.get_installed() {
        anyhow::bail!("bundle {name} not installed");
    }
    let mut caps = Vec::new();
    let list = resp.get_required_caps().context("required caps")?;
    for idx in 0..list.len() {
        let text = list.get(idx).context("cap entry")?.to_str().context("cap utf8")?;
        caps.push(text.to_string());
    }
    Ok(caps)
}

#[cfg(test)]
fn allowed_manifest(
    publisher_hex: &str,
    publisher: &[u8; 16],
    signature: &[u8; 64],
) -> Result<Vec<u8>> {
    build_signed_manifest_nxb("samgrd", &["ipc.core"], publisher_hex, publisher, signature)
}

#[cfg(test)]
fn denied_manifest(
    publisher_hex: &str,
    publisher: &[u8; 16],
    signature: &[u8; 64],
) -> Result<Vec<u8>> {
    build_signed_manifest_nxb("demo.testsvc", &["net.client"], publisher_hex, publisher, signature)
}

#[cfg(test)]
fn build_manifest_nxb(
    name: &str,
    caps: &[&str],
    publisher: &[u8; 16],
    signature: &[u8; 64],
) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut m = message.init_root::<bundle_manifest::Builder<'_>>();
        m.set_schema_version(1);
        m.set_name(name);
        m.set_semver("1.0.0");
        m.set_min_sdk("0.1.0");
        m.set_publisher(publisher);
        m.set_signature(signature);
        let mut abilities = m.reborrow().init_abilities(1);
        abilities.set(0, "core");
        let mut c = m.reborrow().init_capabilities(caps.len() as u32);
        for (i, cap) in caps.iter().enumerate() {
            c.set(i as u32, cap);
        }
    }
    let mut out = Vec::new();
    serialize::write_message(&mut out, &message).expect("serialize manifest");
    out
}

#[cfg(test)]
fn build_signed_manifest_nxb(
    name: &str,
    caps: &[&str],
    publisher_hex: &str,
    publisher: &[u8; 16],
    signature: &[u8; 64],
) -> Result<Vec<u8>> {
    let payload = Vec::new();
    let base_manifest = build_manifest_nxb(name, caps, publisher, signature);
    let binding_manifest = manifest_with_digests(&base_manifest, &payload, None, None)?;
    let binding_sha = sha256_hex(&binding_manifest);
    let sbom = generate_bundle_sbom_json(&BundleSbomInput {
        bundle_name: name.to_string(),
        bundle_version: "1.0.0".to_string(),
        publisher_hex: publisher_hex.to_string(),
        payload_sha256: sha256_hex(&payload),
        payload_size: payload.len() as u64,
        manifest_sha256: binding_sha.clone(),
        source_date_epoch: 0,
        components: Vec::new(),
    })
    .context("generate sbom")?;
    let repro = capture_bundle_repro_json_with_manifest_digest(&binding_sha, &payload, &sbom)
        .context("generate repro metadata")?;
    manifest_with_digests(&base_manifest, &payload, Some(&sbom), Some(&repro))
}

#[cfg(test)]
fn manifest_with_digests(
    manifest: &[u8],
    payload: &[u8],
    sbom: Option<&[u8]>,
    repro_json: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(manifest);
    let message =
        serialize::read_message(&mut cursor, ReaderOptions::new()).context("read manifest")?;
    let src = message.get_root::<bundle_manifest::Reader<'_>>().context("manifest root")?;
    let mut out_builder = Builder::new_default();
    {
        let mut dst = out_builder.init_root::<bundle_manifest::Builder<'_>>();
        dst.set_schema_version(src.get_schema_version());
        dst.set_name(src.get_name().context("name")?.to_str().context("name utf8")?);
        dst.set_semver(src.get_semver().context("semver")?.to_str().context("semver utf8")?);
        dst.set_min_sdk(src.get_min_sdk().context("minSdk")?.to_str().context("minSdk utf8")?);
        dst.set_publisher(src.get_publisher().context("publisher")?);
        dst.set_signature(src.get_signature().context("signature")?);
        dst.set_payload_digest(&hex::decode(sha256_hex(payload)).context("payload digest decode")?);
        dst.set_payload_size(payload.len() as u64);
        if let Some(sbom_bytes) = sbom {
            dst.set_sbom_digest(
                &hex::decode(sha256_hex(sbom_bytes)).context("sbom digest decode")?,
            );
        } else {
            dst.set_sbom_digest(&[]);
        }
        if let Some(repro_bytes) = repro_json {
            dst.set_repro_digest(
                &hex::decode(sha256_hex(repro_bytes)).context("repro digest decode")?,
            );
        } else {
            dst.set_repro_digest(&[]);
        }
        let src_abilities = src.get_abilities().context("abilities")?;
        let mut dst_abilities = dst.reborrow().init_abilities(src_abilities.len());
        for idx in 0..src_abilities.len() {
            dst_abilities.set(idx, src_abilities.get(idx).context("ability entry")?);
        }
        let src_caps = src.get_capabilities().context("caps")?;
        let mut dst_caps = dst.reborrow().init_capabilities(src_caps.len());
        for idx in 0..src_caps.len() {
            dst_caps.set(idx, src_caps.get(idx).context("cap entry")?);
        }
    }
    let mut out = Vec::new();
    serialize::write_message(&mut out, &out_builder).context("serialize manifest with digests")?;
    Ok(out)
}

#[cfg(test)]
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
fn encode_frame(opcode: u8, message: &Builder<HeapAllocator>) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    serialize::write_message(&mut payload, message).context("encode message")?;
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(opcode);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

#[cfg(test)]
fn decode_response(frame: &[u8]) -> Result<(bool, Vec<String>)> {
    let (opcode, payload) = frame.split_first().context("missing opcode")?;
    if *opcode != POLICY_OPCODE_CHECK {
        anyhow::bail!("unexpected opcode {opcode}");
    }
    let mut cursor = Cursor::new(payload);
    let message =
        serialize::read_message(&mut cursor, ReaderOptions::new()).context("decode message")?;
    let response =
        message.get_root::<check_response::Reader<'_>>().context("check response root")?;
    let allowed = response.get_allowed();
    let mut missing = Vec::new();
    if let Ok(list) = response.get_missing() {
        for idx in 0..list.len() {
            let text = list.get(idx).context("missing entry")?.to_str().context("missing utf8")?;
            missing.push(text.to_string());
        }
    }
    Ok((allowed, missing))
}
