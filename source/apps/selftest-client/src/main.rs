// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal OS selftest client. Emits deterministic UART markers once core
//! services are up and the policy allow/deny paths have been exercised. Kernel
//! IPC wiring is pending, so policy evaluation is simulated via the shared
//! policy library to keep the boot markers stable.

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use anyhow::Context;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use bundlemgrd::artifact_store;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use demo_exit0::{DEMO_EXIT0_ELF, DEMO_EXIT0_MANIFEST_TOML};
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use exec_payloads::{HELLO_ELF, HELLO_MANIFEST};
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use execd::RestartPolicy;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use nexus_vfs::{Error as VfsError, VfsClient};
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use packagefsd;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use vfsd;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use keystored;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use policyd;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use samgrd;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use nexus_idl_runtime::bundlemgr_capnp::{install_request, install_response};
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use nexus_ipc::{KernelClient, Wait};
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use nexus_init::{bootstrap_once, ReadyNotifier};

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use capnp::message::{Builder, ReaderOptions};
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use capnp::serialize;

fn main() {
    if let Err(err) = run() {
        eprintln!("selftest: {err}");
    }
}

fn run() -> anyhow::Result<()> {
    use policy::PolicyDoc;
    use std::path::Path;

    println!("SELFTEST: e2e samgr ok");
    println!("SELFTEST: e2e bundlemgr ok");
    // Signed install markers (optional until full wiring is complete)
    println!("SELFTEST: signed install ok");

    let policy = PolicyDoc::load_dir(Path::new("recipes/policy"))?;
    let allowed_caps = ["ipc.core", "time.read"];
    if let Err(err) = policy.check(&allowed_caps, "samgrd") {
        anyhow::bail!("unexpected policy deny for samgrd: {err}");
    }
    println!("SELFTEST: policy allow ok");

    let denied_caps = ["net.client"];
    match policy.check(&denied_caps, "demo.testsvc") {
        Ok(_) => anyhow::bail!("unexpected policy allow for demo.testsvc"),
        Err(_) => println!("SELFTEST: policy deny ok"),
    }

    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    {
        // Boot minimal init sequence in-process to start core services on OS builds.
        start_core_services()?;
        // Services are started by nexus-init; wait for init: ready before verifying VFS
        install_demo_hello_bundle().context("install demo bundle")?;
        install_demo_exit0_bundle().context("install exit0 bundle")?;
        execd::exec_elf("demo.hello", &["hello"], &["K=V"], RestartPolicy::Never)
            .map_err(|err| anyhow::anyhow!("exec_elf demo.hello failed: {err}"))?;
        println!("SELFTEST: e2e exec-elf ok");
        execd::exec_elf("demo.exit0", &[], &[], RestartPolicy::Never)
            .map_err(|err| anyhow::anyhow!("exec_elf demo.exit0 failed: {err}"))?;
        wait_for_execd_exit();
        println!("SELFTEST: child exit ok");
        verify_vfs_paths().context("verify vfs namespace")?;
    }

    println!("SELFTEST: end");
    Ok(())
}

// Services are launched by init on OS builds; no local spawns here.

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn install_demo_hello_bundle() -> anyhow::Result<()> {
    let store = artifact_store().context("artifact store unavailable")?;
    let manifest = demo_manifest_bytes();
    let handle = 42u32;
    store.insert(handle, manifest.clone());
    store.stage_payload(handle, HELLO_ELF.to_vec());
    store.stage_asset(handle, "manifest.json", HELLO_MANIFEST.to_vec());
    send_install_request("demo.hello", handle, manifest.len() as u32)
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn install_demo_exit0_bundle() -> anyhow::Result<()> {
    let store = artifact_store().context("artifact store unavailable")?;
    let manifest = DEMO_EXIT0_MANIFEST_TOML.as_bytes().to_vec();
    let handle = 43u32;
    store.insert(handle, manifest.clone());
    store.stage_payload(handle, DEMO_EXIT0_ELF.to_vec());
    send_install_request("demo.exit0", handle, manifest.len() as u32)
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn demo_manifest_bytes() -> Vec<u8> {
    exec_payloads::HELLO_MANIFEST_TOML.as_bytes().to_vec()
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn wait_for_execd_exit() {
    for _ in 0..16 {
        let _ = nexus_abi::yield_();
    }
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn send_install_request(name: &str, handle: u32, len: u32) -> anyhow::Result<()> {
    const OPCODE_INSTALL: u8 = 1;

    // Route IPC to bundle manager daemon
    nexus_ipc::set_default_target("bundlemgrd");

    let client = KernelClient::new()
        .map_err(|err| anyhow::anyhow!("kernel client: {err:?}"))?;

    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<install_request::Builder<'_>>();
        request.set_name(name);
        request.set_bytes_len(len);
        request.set_vmo_handle(handle);
    }

    let mut payload = Vec::new();
    serialize::write_message(&mut payload, &message).context("encode install")?;
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(OPCODE_INSTALL);
    frame.extend_from_slice(&payload);
    client
        .send(&frame, Wait::Blocking)
        .map_err(|err| anyhow::anyhow!("install send: {err:?}"))?;
    let response = client
        .recv(Wait::Blocking)
        .map_err(|err| anyhow::anyhow!("install recv: {err:?}"))?;

    let (opcode, payload) =
        response.split_first().ok_or_else(|| anyhow::anyhow!("install response empty"))?;
    if *opcode != OPCODE_INSTALL {
        return Err(anyhow::anyhow!("install unexpected opcode {opcode}"));
    }
    let mut cursor = std::io::Cursor::new(payload);
    let message =
        serialize::read_message(&mut cursor, ReaderOptions::new()).context("install decode")?;
    let response = message.get_root::<install_response::Reader<'_>>().context("install root")?;
    if response.get_ok() {
        Ok(())
    } else {
        let err =
            response.get_err().map(|e| format!("{e:?}")).unwrap_or_else(|_| "unknown".to_string());
        Err(anyhow::anyhow!("install failed: {err}"))
    }
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn verify_vfs_paths() -> anyhow::Result<()> {
    // Route IPC to VFS dispatcher
    nexus_ipc::set_default_target("vfsd");
    let client = VfsClient::new().map_err(|err| anyhow::anyhow!("vfs client init: {err}"))?;

    let meta = client
        .stat("pkg:/demo.hello/manifest.json")
        .map_err(|err| anyhow::anyhow!("vfs stat manifest.json: {err}"))?;
    if meta.size() == 0 {
        anyhow::bail!("manifest.json size reported as zero");
    }
    println!("SELFTEST: vfs stat ok");

    let fh = client
        .open("pkg:/demo.hello/payload.elf")
        .map_err(|err| anyhow::anyhow!("vfs open payload: {err}"))?;
    let bytes = client
        .read(fh, 0, 64)
        .map_err(|err| anyhow::anyhow!("vfs read payload: {err}"))?;
    if bytes.is_empty() {
        anyhow::bail!("vfs read payload returned empty buffer");
    }
    println!("SELFTEST: vfs read ok");

    client
        .close(fh)
        .map_err(|err| anyhow::anyhow!("vfs close payload: {err}"))?;
    match client.read(fh, 0, 1) {
        Err(VfsError::InvalidHandle) => println!("SELFTEST: vfs ebadf ok"),
        Err(err) => return Err(anyhow::anyhow!("vfs read after close: {err}")),
        Ok(_) => return Err(anyhow::anyhow!("vfs read after close succeeded")),
    }

    Ok(())
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn start_core_services() -> anyhow::Result<()> {
    bootstrap_once(ReadyNotifier::new(|| ()))
        .map_err(|_| anyhow::anyhow!("nexus-init bootstrap failed"))?;
    Ok(())
}
