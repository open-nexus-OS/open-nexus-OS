//! TASK-0023B P2-14: init-health helper for the updated submodule.
//!
//! Hosts `init_health_ok` -- bring-up health probe sent on the init control
//! channel (CTRL_SEND_SLOT=1 / CTRL_RECV_SLOT=2). Behavior is byte-for-byte
//! identical to the pre-split implementation.

use nexus_abi::{yield_, MsgHeader};

pub(crate) fn init_health_ok() -> core::result::Result<(), ()> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    static NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut req = [0u8; 8];
    req[..4].copy_from_slice(&[b'I', b'H', 1, 1]);
    req[4..8].copy_from_slice(&nonce.to_le_bytes());
    let hdr = MsgHeader::new(0, 0, 0, 0, req.len() as u32);

    // Use explicit time-bounded NONBLOCK loops (avoid flaky kernel deadline semantics).
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(30_000_000_000); // 30s (init may contend with stage work)
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(CTRL_SEND_SLOT, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(|_| ())?;
                    if now >= deadline {
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }

    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
    loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline {
                return Err(());
            }
        }
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n == 9 && buf[0] == b'I' && buf[1] == b'H' && buf[2] == 1 {
                    let got_nonce = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
                    if got_nonce != nonce {
                        continue;
                    }
                    if buf[3] == (1 | 0x80) && buf[4] == 0 {
                        return Ok(());
                    }
                    return Err(());
                }
                // Ignore unrelated control responses.
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        j = j.wrapping_add(1);
    }
}
