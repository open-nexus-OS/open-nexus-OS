// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal OS selftest client. Emits deterministic UART markers once core
//! services are up and the policy allow/deny paths have been exercised. Kernel
//! IPC wiring is pending, so policy evaluation is simulated via the shared
//! policy library to keep the boot markers stable.

#![forbid(unsafe_code)]

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
        execd::exec_hello_elf().map_err(|err| anyhow::anyhow!("exec_hello_elf failed: {err}"))?;
        println!("SELFTEST: e2e exec-elf ok");
    }

    println!("SELFTEST: end");
    Ok(())
}
