extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::{yield_, MsgHeader};
use statefs::protocol as statefs_proto;

use super::super::ipc::routing::route_with_retry;
use crate::markers::emit_line;

pub(crate) fn bootctl_persist_check() -> core::result::Result<(), ()> {
    const BOOTCTL_KEY: &str = "/state/boot/bootctl.v1";
    const BOOTCTL_VERSION: u8 = 1;
    emit_line("SELFTEST: bootctl persist begin");
    let client = route_with_retry("statefsd")?;
    let (send_slot, recv_slot) = client.slots();
    // Deterministic: use SF v2 (nonce) and only accept the matching reply.
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let get_v1 = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, BOOTCTL_KEY)
        .map_err(|_| ())?;
    // Upgrade v1 request frame to v2 by inserting nonce after the 4-byte header.
    let mut get = Vec::with_capacity(get_v1.len().saturating_add(8));
    get.extend_from_slice(&get_v1[..4]);
    get[2] = statefs_proto::VERSION_V2;
    get.extend_from_slice(&nonce.to_le_bytes());
    get.extend_from_slice(&get_v1[4..]);
    // NOTE: Avoid `KernelClient::send/recv` timeout semantics here (kernel deadlines can be flaky
    // under QEMU when queues are full). Use explicit nsec-bounded NONBLOCK loops instead.
    // Send.
    let hdr = MsgHeader::new(0, 0, 0, 0, get.len() as u32);
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(2_000_000_000);
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &get, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(|_| ())?;
                    if now >= deadline {
                        emit_line("SELFTEST: bootctl persist send timeout");
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }
    // Recv.
    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 512];
    let mut j: usize = 0;
    let n = loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline {
                emit_line("SELFTEST: bootctl persist recv timeout");
                return Err(());
            }
        }
        match nexus_abi::ipc_recv_v1(
            recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => break n as usize,
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        j = j.wrapping_add(1);
    };
    let n = core::cmp::min(n, buf.len());
    if n < 13 || buf[0] != statefs_proto::MAGIC0 || buf[1] != statefs_proto::MAGIC1 {
        return Err(());
    }
    if buf[2] != statefs_proto::VERSION_V2 {
        return Err(());
    }
    let got_nonce =
        u64::from_le_bytes([buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12]]);
    if got_nonce != nonce {
        return Err(());
    }
    let bytes = statefs_proto::decode_get_response(&buf[..n]).map_err(|_| ())?;
    if bytes.len() != 6 || bytes[0] != BOOTCTL_VERSION {
        return Err(());
    }
    Ok(())
}
