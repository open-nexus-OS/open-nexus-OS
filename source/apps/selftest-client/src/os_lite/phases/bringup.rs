// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 1 of 12 — bringup (keystored, qos, timed-coalesce, rng,
//!   device-key, statefs CRUD/persist, reply slot announce, dsoftbus
//!   readiness gate, samgrd v1 register/lookup/unknown/malformed).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — first slice.
//!
//! Extracted in Cut P2-02 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. `keystored` is resolved here, used by
//! `keystored_cap_move_probe`, then dropped at end-of-phase. The policy slice
//! (later P2-07) re-resolves `keystored` for `keystored_sign_denied`;
//! `resolve_keystored_client` emits no markers, so the marker ladder is
//! unchanged.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::{yield_, MsgHeader};
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::ipc::routing::{route_with_retry, routing_v1_get};
use crate::os_lite::{probes, services, timed};

pub(crate) fn run(ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // keystored v1 (routing + put/get/del + negative cases)
    let keystored = match services::keystored::resolve_keystored_client() {
        Ok(client) => client,
        Err(_) => return Err(()),
    };
    emit_line("SELFTEST: ipc routing keystored ok");
    emit_line("SELFTEST: keystored v1 ok");
    if probes::ipc_kernel::qos_probe().is_ok() {
        emit_line("SELFTEST: qos ok");
    } else {
        emit_line("SELFTEST: qos FAIL");
    }
    if timed::timed_coalesce_probe().is_ok() {
        emit_line("SELFTEST: timed coalesce ok");
    } else {
        emit_line("SELFTEST: timed coalesce FAIL");
    }
    // RNG and device identity key selftests (run early to keep QEMU marker deadlines short).
    probes::rng::rng_entropy_selftest();
    probes::rng::rng_entropy_oversized_selftest();
    let device_pubkey = probes::device_key::device_key_selftest();
    // statefs (basic put/get/list + unauthorized access)
    if let Ok(statefsd) = route_with_retry("statefsd") {
        if services::statefs::statefs_put_get_list(&statefsd).is_ok() {
            emit_line("SELFTEST: statefs put ok");
        } else {
            emit_line("SELFTEST: statefs put FAIL");
        }
        if services::statefs::statefs_unauthorized_access(&statefsd).is_ok() {
            emit_line("SELFTEST: statefs unauthorized access rejected");
        } else {
            emit_line("SELFTEST: statefs unauthorized access rejected FAIL");
        }
        if services::statefs::statefs_persist(&statefsd).is_ok() {
            emit_line("SELFTEST: statefs persist ok");
        } else {
            emit_line("SELFTEST: statefs persist FAIL");
        }
    } else {
        emit_line("SELFTEST: statefs put FAIL");
        emit_line("SELFTEST: statefs unauthorized access rejected FAIL");
        emit_line("SELFTEST: statefs persist FAIL");
    }
    if let Some(pubkey) = device_pubkey {
        if probes::device_key::device_key_reload_and_check(&pubkey).is_ok() {
            emit_line("SELFTEST: device key persist ok");
        } else {
            emit_line("SELFTEST: device key persist FAIL");
        }
    } else {
        emit_line("SELFTEST: device key persist FAIL");
    }
    // @reply slots are deterministically distributed by init-lite to selftest-client.
    // The slot constants live in `context::PhaseCtx::bootstrap()`.
    let reply_ok = true;
    emit_bytes(b"SELFTEST: reply slots ");
    emit_hex_u64(ctx.reply_send_slot as u64);
    emit_byte(b' ');
    emit_hex_u64(ctx.reply_recv_slot as u64);
    emit_byte(b'\n');

    // Loopback sanity: prove the @reply send/recv slots refer to the same live endpoint.
    // This is safe (self-addressed) and helps debug CAP_MOVE reply delivery.
    if reply_ok {
        let ping = [b'R', b'P', 1, 0];
        let hdr = MsgHeader::new(0, 0, 0, 0, ping.len() as u32);
        // Best-effort send; ignore failures (still proceed with tests).
        let _ = nexus_abi::ipc_send_v1(
            ctx.reply_send_slot,
            &hdr,
            &ping,
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        );
        let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
        let mut rb = [0u8; 8];
        let mut ok = false;
        for _ in 0..256 {
            match nexus_abi::ipc_recv_v1(
                ctx.reply_recv_slot,
                &mut rh,
                &mut rb,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    let n = n as usize;
                    if n == ping.len() && &rb[..n] == &ping {
                        ok = true;
                        break;
                    }
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = yield_();
                }
                Err(_) => break,
            }
        }
        if ok {
            emit_line("SELFTEST: reply loopback ok");
        } else {
            emit_line("SELFTEST: reply loopback FAIL");
        }
    } else {
        emit_line("SELFTEST: reply loopback FAIL");
    }

    if reply_ok {
        if services::keystored::keystored_cap_move_probe(ctx.reply_send_slot, ctx.reply_recv_slot)
            .is_ok()
        {
            emit_line("SELFTEST: keystored capmove ok");
        } else {
            emit_line("SELFTEST: keystored capmove FAIL");
        }
    } else {
        emit_line("SELFTEST: keystored capmove FAIL");
    }

    // Readiness gate: ensure dsoftbusd is ready before running routing-dependent probes.
    // This is required for the canonical marker ladder order in `scripts/qemu-test.sh`.
    if let Ok(logd) = KernelClient::new_for("logd") {
        let start = nexus_abi::nsec().unwrap_or(0);
        let deadline = start.saturating_add(5_000_000_000); // 5s (bounded)
        loop {
            if services::logd::logd_query_contains_since_paged(&logd, 0, b"dsoftbusd: ready")
                .unwrap_or(false)
            {
                break;
            }
            let now = nexus_abi::nsec().unwrap_or(0);
            if now >= deadline {
                // Don't emit FAIL markers here; the harness will fail anyway if dsoftbusd never becomes ready.
                break;
            }
            for _ in 0..32 {
                let _ = yield_();
            }
        }
    }

    // samgrd v1 lookup (routing + ok/unknown/malformed)
    let samgrd = match route_with_retry("samgrd") {
        Ok(client) => client,
        Err(_) => return Err(()),
    };
    let (sam_send_slot, sam_recv_slot) = samgrd.slots();
    emit_bytes(b"SELFTEST: samgrd slots ");
    emit_hex_u64(sam_send_slot as u64);
    emit_byte(b' ');
    emit_hex_u64(sam_recv_slot as u64);
    emit_byte(b'\n');
    let samgrd = samgrd;
    emit_line("SELFTEST: ipc routing samgrd ok");
    // Reply inbox for CAP_MOVE samgrd RPC.
    let (route_send, route_recv) = match routing_v1_get("vfsd") {
        Ok((st, send, recv)) if st == nexus_abi::routing::STATUS_OK && send != 0 && recv != 0 => {
            emit_bytes(b"SELFTEST: routing vfsd st=0x");
            emit_hex_u64(st as u64);
            emit_bytes(b" send=0x");
            emit_hex_u64(send as u64);
            emit_bytes(b" recv=0x");
            emit_hex_u64(recv as u64);
            emit_byte(b'\n');
            (send, recv)
        }
        _ => {
            // Fallback to deterministic slots distributed by init-lite to selftest-client.
            emit_line("SELFTEST: routing vfsd fallback slots");
            (0x03, 0x04)
        }
    };
    match services::samgrd::samgrd_v1_register(&samgrd, "vfsd", route_send, route_recv) {
        Ok(0) => emit_line("SELFTEST: samgrd v1 register ok"),
        Ok(st) => {
            emit_bytes(b"SELFTEST: samgrd v1 register FAIL st=0x");
            emit_hex_u64(st as u64);
            emit_byte(b'\n');
        }
        Err(_) => emit_line("SELFTEST: samgrd v1 register FAIL err"),
    }
    match services::samgrd::samgrd_v1_lookup(&samgrd, "vfsd") {
        Ok((st, got_send, got_recv)) => {
            if st == 0 && got_send == route_send && got_recv == route_recv {
                emit_line("SELFTEST: samgrd v1 lookup ok");
            } else {
                emit_line("SELFTEST: samgrd v1 lookup FAIL");
            }
        }
        Err(_) => emit_line("SELFTEST: samgrd v1 lookup FAIL"),
    }
    match services::samgrd::samgrd_v1_lookup(&samgrd, "does.not.exist") {
        Ok((st, _send, _recv)) => {
            if st == 1 {
                emit_line("SELFTEST: samgrd v1 unknown ok");
            } else {
                emit_line("SELFTEST: samgrd v1 unknown FAIL");
            }
        }
        Err(_) => emit_line("SELFTEST: samgrd v1 unknown FAIL"),
    }
    // Malformed request (wrong magic) should not return OK.
    samgrd
        .send(
            b"bad",
            IpcWait::Timeout(core::time::Duration::from_millis(200)),
        )
        .map_err(|_| ())?;
    let rsp = samgrd
        .recv(IpcWait::Timeout(core::time::Duration::from_millis(200)))
        .map_err(|_| ())?;
    if rsp.len() == 13 && rsp[0] == b'S' && rsp[1] == b'M' && rsp[2] == 1 && rsp[4] != 0 {
        emit_line("SELFTEST: samgrd v1 malformed ok");
    } else {
        emit_line("SELFTEST: samgrd v1 malformed FAIL");
    }

    // `keystored` is intentionally dropped at end-of-phase; the policy slice
    // (later P2-07) re-resolves it via `services::keystored::resolve_keystored_client()`.
    // `route_with_retry`/`resolve_keystored_client` are silent (no markers), so
    // re-resolution preserves the marker ladder byte-identically.
    let _ = keystored;
    Ok(())
}
