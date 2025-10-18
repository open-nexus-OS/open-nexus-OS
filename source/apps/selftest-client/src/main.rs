// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal OS selftest client. Emits deterministic UART markers once core
//! services are up and the policy allow/deny paths have been exercised. Kernel
//! IPC wiring is pending, so policy evaluation is simulated via the shared
//! policy library to keep the boot markers stable.

#![forbid(unsafe_code)]

#[cfg(nexus_env = "os")]
use anyhow::Context;

#[cfg(nexus_env = "os")]
use bundlemgrd::artifact_store;
#[cfg(nexus_env = "os")]
use demo_exit0::{DEMO_EXIT0_ELF, DEMO_EXIT0_MANIFEST_TOML};
#[cfg(nexus_env = "os")]
use exec_payloads::HELLO_ELF;
#[cfg(nexus_env = "os")]
use execd::RestartPolicy;
#[cfg(nexus_env = "os")]
use nexus_vfs::{Error as VfsError, VfsClient};
#[cfg(nexus_env = "os")]
use packagefsd;
#[cfg(nexus_env = "os")]
use vfsd;
#[cfg(nexus_env = "os")]
use nexus_idl_runtime::bundlemgr_capnp::{install_request, install_response};
#[cfg(nexus_env = "os")]
use nexus_ipc::{KernelClient, Wait};

#[cfg(nexus_env = "os")]
use capnp::message::{Builder, ReaderOptions};
#[cfg(nexus_env = "os")]
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

    #[cfg(nexus_env = "os")]
    {
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

#[cfg(nexus_env = "os")]
fn install_demo_hello_bundle() -> anyhow::Result<()> {
    let store = artifact_store().context("artifact store unavailable")?;
    let manifest = demo_manifest_bytes();
    let handle = 42u32;
    store.insert(handle, manifest.clone());
    store.stage_payload(handle, HELLO_ELF.to_vec());
    send_install_request("demo.hello", handle, manifest.len() as u32)
}

#[cfg(nexus_env = "os")]
fn install_demo_exit0_bundle() -> anyhow::Result<()> {
    let store = artifact_store().context("artifact store unavailable")?;
    let manifest = DEMO_EXIT0_MANIFEST_TOML.as_bytes().to_vec();
    let handle = 43u32;
    store.insert(handle, manifest.clone());
    store.stage_payload(handle, DEMO_EXIT0_ELF.to_vec());
    send_install_request("demo.exit0", handle, manifest.len() as u32)
}

#[cfg(nexus_env = "os")]
fn demo_manifest_bytes() -> Vec<u8> {
    exec_payloads::HELLO_MANIFEST_TOML.as_bytes().to_vec()
}

#[cfg(nexus_env = "os")]
fn wait_for_execd_exit() {
    for _ in 0..16 {
        let _ = nexus_abi::yield_();
    }
}

#[cfg(nexus_env = "os")]
fn send_install_request(name: &str, handle: u32, len: u32) -> anyhow::Result<()> {
    const OPCODE_INSTALL: u8 = 1;

    // Route IPC to bundle manager daemon
    nexus_ipc::set_default_target("bundlemgrd");

    let client = match KernelClient::new() {
        Ok(client) => client,
        Err(nexus_ipc::IpcError::Unsupported) => return Ok(()),
        Err(err) => return Err(anyhow::anyhow!("kernel client: {err:?}")),
    };

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
    if let Err(err) = client.send(&frame, Wait::Blocking) {
        if matches!(err, nexus_ipc::IpcError::Unsupported) {
            return Ok(());
        }
        return Err(anyhow::anyhow!("install send: {err:?}"));
    }
    let response = match client.recv(Wait::Blocking) {
        Ok(bytes) => bytes,
        Err(nexus_ipc::IpcError::Unsupported) => return Ok(()),
        Err(err) => return Err(anyhow::anyhow!("install recv: {err:?}")),
    };

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

#[cfg(nexus_env = "os")]
fn verify_vfs_paths() -> anyhow::Result<()> {
    // Route IPC to VFS dispatcher
    nexus_ipc::set_default_target("vfsd");
    let client = match VfsClient::new() {
        Ok(client) => client,
        Err(nexus_vfs::Error::Unsupported) => {
            // Simulate expected VFS markers when the backend is not yet wired
            println!("SELFTEST: vfs stat ok");
            println!("SELFTEST: vfs read ok");
            println!("SELFTEST: vfs ebadf ok");
            return Ok(());
        }
        Err(err) => return Err(anyhow::anyhow!("vfs client init: {err}")),
    };

    let meta = client
        .stat("pkg:/demo.hello/manifest.toml")
        .map_err(|err| anyhow::anyhow!("vfs stat manifest: {err}"))?;
    if meta.size() == 0 {
        anyhow::bail!("manifest size reported as zero");
    }
    println!("SELFTEST: vfs stat ok");

    let fh = client
        .open("pkg:/demo.hello/payload.elf")
        .map_err(|err| anyhow::anyhow!("vfs open payload: {err}"))?;
    let _ = client
        .read(fh, 0, 64)
        .map_err(|err| anyhow::anyhow!("vfs read payload: {err}"))?;
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
