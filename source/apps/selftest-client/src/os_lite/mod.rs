extern crate alloc;

use crate::markers::emit_line;
use nexus_abi::yield_;

mod context;
mod dsoftbus;
mod ipc;
mod mmio;
mod net;
mod phases;
mod probes;
mod services;
mod timed;
mod updated;
mod vfs;

pub fn run() -> core::result::Result<(), ()> {
    let mut ctx = context::PhaseCtx::bootstrap()?;
    phases::bringup::run(&mut ctx)?;
    phases::routing::run(&mut ctx)?;
    phases::ota::run(&mut ctx)?;
    phases::policy::run(&mut ctx)?;

    phases::exec::run(&mut ctx)?;
    phases::logd::run(&mut ctx)?;
    phases::ipc_kernel::run(&mut ctx)?;
    phases::mmio::run(&mut ctx)?;
    phases::vfs::run(&mut ctx)?;
    phases::net::run(&mut ctx)?;

    // TASK-0005: Cross-VM remote proxy proof (opt-in 2-VM harness).
    // Only Node A emits the markers; single-VM smoke must not block on remote RPC waits.
    if ctx.os2vm && ctx.local_ip.is_some() {
        // Retry with a wall-clock bound to keep tests deterministic and fast.
        // dsoftbusd must establish the session first.
        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let deadline_ms = start_ms.saturating_add(4_000);
        let mut ok = false;
        loop {
            if dsoftbus::remote::resolve::dsoftbusd_remote_resolve("bundlemgrd").is_ok() {
                ok = true;
                break;
            }
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if now_ms >= deadline_ms {
                break;
            }
            let _ = yield_();
        }
        if ok {
            emit_line("SELFTEST: remote resolve ok");
        } else {
            emit_line("SELFTEST: remote resolve FAIL");
        }

        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let deadline_ms = start_ms.saturating_add(4_000);
        let mut got: Option<u16> = None;
        loop {
            if let Ok(count) = dsoftbus::remote::resolve::dsoftbusd_remote_bundle_list() {
                got = Some(count);
                break;
            }
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if now_ms >= deadline_ms {
                break;
            }
            let _ = yield_();
        }
        if let Some(_count) = got {
            emit_line("SELFTEST: remote query ok");
        } else {
            emit_line("SELFTEST: remote query FAIL");
        }

        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let deadline_ms = start_ms.saturating_add(4_000);
        let mut statefs_ok = false;
        loop {
            if dsoftbus::remote::statefs::dsoftbusd_remote_statefs_rw_roundtrip().is_ok() {
                statefs_ok = true;
                break;
            }
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if now_ms >= deadline_ms {
                break;
            }
            let _ = yield_();
        }
        if statefs_ok {
            emit_line("SELFTEST: remote statefs rw ok");
        } else {
            emit_line("SELFTEST: remote statefs rw FAIL");
        }

        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let deadline_ms = start_ms.saturating_add(4_000);
        let mut pkg_ok = false;
        loop {
            if let Ok(bytes) = dsoftbus::remote::pkgfs::dsoftbusd_remote_pkgfs_read_once(
                "pkg:/system/build.prop",
                64,
            ) {
                if !bytes.is_empty() {
                    pkg_ok = true;
                    break;
                }
            }
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if now_ms >= deadline_ms {
                break;
            }
            let _ = yield_();
        }
        if pkg_ok {
            emit_line("SELFTEST: remote pkgfs read ok");
        } else {
            emit_line("SELFTEST: remote pkgfs read FAIL");
        }
    }

    emit_line("SELFTEST: end");

    // Stay alive (cooperative).
    loop {
        let _ = yield_();
    }
}

// NOTE: Keep this file's marker surface centralized in `crate::markers`.
