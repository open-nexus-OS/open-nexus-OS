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
use nexus_idl_runtime::bundlemgr_capnp::{
    install_request, install_response, query_request, query_response, InstallError,
};
#[cfg(test)]
use nexus_idl_runtime::policyd_capnp::{check_request, check_response};
#[cfg(test)]
use nexus_ipc::{Client, Wait};
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

    let (samgr_client, mut samgr_server) = samgrd::loopback_transport();
    let samgr_handle = thread::spawn(move || {
        samgrd::run_with_transport(&mut samgr_server).expect("samgrd exits cleanly");
    });
    drop(samgr_client);

    // Keystore is not required for host-side manifest installation in this test,
    // because signature verification is skipped when no keystore is wired.

    let (bundle_client, mut bundle_server) = bundlemgrd::loopback_transport();
    let store = bundlemgrd::ArtifactStore::new();
    let store_clone = store.clone();
    let bundle_handle = thread::spawn(move || {
        bundlemgrd::run_with_transport(&mut bundle_server, store_clone, None, None)
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
    policy_ready_rx
        .recv_timeout(Duration::from_secs(2))
        .context("wait policyd ready")?;

    // Install manifests and query required capabilities from bundlemgrd
    let allowed_manifest = allowed_manifest();
    let denied_manifest = denied_manifest();
    store.insert(1, allowed_manifest.clone().into_bytes());
    store.stage_payload(1, Vec::new());
    store.insert(2, denied_manifest.clone().into_bytes());
    store.stage_payload(2, Vec::new());
    install_bundle(&bundle_client, "samgrd", 1, allowed_manifest.len() as u32)?;
    install_bundle(
        &bundle_client,
        "demo.testsvc",
        2,
        denied_manifest.len() as u32,
    )?;
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
    samgr_handle.join().expect("samgrd thread joins");
    std::env::remove_var("NEXUS_POLICY_DIR");
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
    client
        .send(&frame, Wait::Blocking)
        .context("send install")?;
    let response = client.recv(Wait::Blocking).context("recv install")?;
    let (opcode, payload) = response.split_first().context("install opcode")?;
    if *opcode != BUNDLE_OPCODE_INSTALL {
        anyhow::bail!("unexpected opcode {opcode}");
    }
    let mut cursor = Cursor::new(payload);
    let message =
        serialize::read_message(&mut cursor, ReaderOptions::new()).context("install decode")?;
    let resp = message
        .get_root::<install_response::Reader<'_>>()
        .context("install root")?;
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
    let resp = message
        .get_root::<query_response::Reader<'_>>()
        .context("query root")?;
    if !resp.get_installed() {
        anyhow::bail!("bundle {name} not installed");
    }
    let mut caps = Vec::new();
    let list = resp.get_required_caps().context("required caps")?;
    for idx in 0..list.len() {
        let text = list
            .get(idx)
            .context("cap entry")?
            .to_str()
            .context("cap utf8")?;
        caps.push(text.to_string());
    }
    Ok(caps)
}

#[cfg(test)]
fn allowed_manifest() -> String {
    let publisher = "0123456789abcdef0123456789abcdef"; // 32 hex chars
    let sig_hex = "11".repeat(64); // 64 bytes as 128 hex chars
    format!(
        "name = \"samgrd\"\nversion = \"1.0.0\"\nabilities = [\"core\"]\ncaps = [\"ipc.core\"]\nmin_sdk = \"0.1.0\"\npublisher = \"{}\"\nsig = \"{}\"\n",
        publisher, sig_hex
    )
}

#[cfg(test)]
fn denied_manifest() -> String {
    let publisher = "0123456789abcdef0123456789abcdef"; // 32 hex chars
    let sig_hex = "22".repeat(64); // 64 bytes as 128 hex chars
    format!(
        "name = \"demo.testsvc\"\nversion = \"1.0.0\"\nabilities = [\"demo\"]\ncaps = [\"net.client\"]\nmin_sdk = \"0.1.0\"\npublisher = \"{}\"\nsig = \"{}\"\n",
        publisher, sig_hex
    )
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
    let response = message
        .get_root::<check_response::Reader<'_>>()
        .context("check response root")?;
    let allowed = response.get_allowed();
    let mut missing = Vec::new();
    if let Ok(list) = response.get_missing() {
        for idx in 0..list.len() {
            let text = list
                .get(idx)
                .context("missing entry")?
                .to_str()
                .context("missing utf8")?;
            missing.push(text.to_string());
        }
    }
    Ok((allowed, missing))
}
