// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host — the DSL app runtime process (TASK-0080D). R1 is the
//! ADR-0042 transport PROBE: spawned by execd (not a boot service), it
//! creates its own surface VMO, fills it with a solid color, and presents
//! through windowd's client-surface wire (`SURFACE_CREATE` with the VMO
//! capability moved, then a strictly-sequenced `SURFACE_PRESENT`). Proves
//! spawn + per-app VMO + cross-process present before any DSL involvement.
//! R2 mounts a real `.nxir` payload behind the same surface.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: wire codecs host-tested in nexus-display-proto; the probe
//! itself is proven via QEMU markers (`APPHOST: …`).
//! ADR: docs/adr/0042-cross-process-surface-transport.md

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), &'static str> {
    probe::run()
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("app-host: host mode - the probe runs on the OS (QEMU markers)");
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod probe {
    use nexus_abi::{cap_clone, debug_println, vmo_create, vmo_write, yield_};
    use nexus_display_proto::client_surface as wire;
    use nexus_ipc::{Client as _, KernelClient, Wait};

    /// Fixed child capability slots — execd transfers these AFTER spawn
    /// (`cap_transfer_to_slot`): SEND on windowd's server endpoint into 5,
    /// RECV on windowd's shared response endpoint into 6 (the inputd slot
    /// convention). The child may run before the transfer lands, so every
    /// first use retries bounded (the #123 empty-slot lesson).
    const WINDOWD_SEND_SLOT: u32 = 5;
    const WINDOWD_RECV_SLOT: u32 = 6;

    /// Probe surface: well under the transport bounds.
    const SURFACE_W: u16 = 320;
    const SURFACE_H: u16 = 240;

    /// Solid probe color (BGRA): a saturated teal nothing else in the shell
    /// paints — unmistakable in a screenshot.
    const FILL_BGRA: [u8; 4] = [0x98, 0xA1, 0x2A, 0xFF];

    /// Bounded retry budget for the cap-transfer race + windowd bring-up.
    const SEND_RETRIES: usize = 4000;

    pub(super) fn run() -> Result<(), &'static str> {
        let _ = debug_println("apphost: start");

        // 1. The app's own surface VMO (per-app isolation, ADR-0037).
        let bytes = SURFACE_W as usize * SURFACE_H as usize * 4;
        let vmo = vmo_create(bytes).map_err(|_| "apphost: vmo create failed")?;

        // 2. Fill with the probe color, row by row (no heap).
        let mut row = [0u8; SURFACE_W as usize * 4];
        for px in row.chunks_exact_mut(4) {
            px.copy_from_slice(&FILL_BGRA);
        }
        let row_bytes = SURFACE_W as usize * 4;
        for y in 0..SURFACE_H as usize {
            vmo_write(vmo, y * row_bytes, &row).map_err(|_| "apphost: vmo fill failed")?;
        }
        let _ = debug_println("apphost: vmo filled");

        let client = KernelClient::new_with_slots(WINDOWD_SEND_SLOT, WINDOWD_RECV_SLOT)
            .map_err(|_| "apphost: client slots")?;

        // 3. SURFACE_CREATE — a CLONE of the VMO cap moves with the message
        //    (the gpud-attach pattern); the original stays ours for redraws.
        let clone = cap_clone(vmo).map_err(|_| "apphost: cap clone failed")?;
        let create = wire::encode_surface_create(SURFACE_W, SURFACE_H, wire::FORMAT_BGRA8888);
        send_retry_cap(&client, &create, clone)?;
        let surface_id = recv_ack(&client, wire::OP_SURFACE_CREATE)?;
        let _ = debug_println("APPHOST: surface created");

        // 4. SURFACE_PRESENT seq=1, full damage — strictly one in flight.
        let damage = [wire::DamageRect { x: 0, y: 0, width: SURFACE_W, height: SURFACE_H }];
        let mut buf = [0u8; wire::SURFACE_PRESENT_MAX_LEN];
        let len = wire::encode_surface_present(surface_id, 1, &damage, &mut buf);
        send_retry(&client, &buf[..len])?;
        let _ = recv_ack(&client, wire::OP_SURFACE_PRESENT)?;
        let _ = debug_println("APPHOST: probe surface presented");

        // 5. Stay alive (R3 adds the input loop); fully yielded, zero polling
        //    work — the process exists so the window keeps its owner.
        loop {
            let _ = yield_();
        }
    }

    /// Sends with bounded retries: the fixed slots may not be populated yet
    /// (execd transfers after spawn) and windowd may still be booting.
    fn send_retry(client: &KernelClient, frame: &[u8]) -> Result<(), &'static str> {
        for _ in 0..SEND_RETRIES {
            match client.send(frame, Wait::NonBlocking) {
                Ok(()) => return Ok(()),
                Err(_) => {
                    let _ = yield_();
                }
            }
        }
        let _ = debug_println("apphost: FAIL send retries exhausted");
        Err("apphost: send failed")
    }

    fn send_retry_cap(
        client: &KernelClient,
        frame: &[u8],
        cap: u32,
    ) -> Result<(), &'static str> {
        for _ in 0..SEND_RETRIES {
            match client.send_with_cap_move_wait(frame, cap, Wait::NonBlocking) {
                Ok(()) => return Ok(()),
                Err(_) => {
                    let _ = yield_();
                }
            }
        }
        let _ = debug_println("apphost: FAIL create send retries exhausted");
        Err("apphost: create send failed")
    }

    /// Receives the matching ack (skips unrelated frames on the shared
    /// response channel, bounded). Returns the ack value on OK status.
    fn recv_ack(client: &KernelClient, op: u8) -> Result<u32, &'static str> {
        let mut frame = [0u8; 64];
        for _ in 0..SEND_RETRIES {
            match client.recv_into(Wait::NonBlocking, &mut frame) {
                Ok(len) => {
                    if let Some((status, value)) =
                        wire::decode_surface_ack(&frame[..len], op)
                    {
                        if status == wire::SURFACE_STATUS_OK {
                            return Ok(value);
                        }
                        let _ = debug_println("apphost: FAIL surface ack status");
                        return Err("apphost: ack status");
                    }
                    // Unrelated frame on the shared channel — keep waiting.
                }
                Err(_) => {
                    let _ = yield_();
                }
            }
        }
        let _ = debug_println("apphost: FAIL ack timeout");
        Err("apphost: ack timeout")
    }
}
