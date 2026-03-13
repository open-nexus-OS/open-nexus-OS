//! Nonce-correlated netstack RPC over deterministic reply slots.

use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::{KernelClient, Wait};
use super::validate::{extract_netstack_reply_nonce, response_matches};

#[inline]
pub(crate) fn next_nonce(n: &mut u64) -> u64 {
    let out = *n;
    *n = n.wrapping_add(1);
    out
}

pub(crate) fn rpc_nonce(
    pending: &mut ReplyBuffer<16, 512>,
    net: &KernelClient,
    req: &[u8],
    expect_rsp_op: u8,
    nonce: u64,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<[u8; 512], ()> {
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
    if net
        .send_with_cap_move_wait(req, reply_send_clone, Wait::NonBlocking)
        .is_err()
    {
        let _ = nexus_abi::cap_close(reply_send_clone);
        return Err(());
    }
    let _ = nexus_abi::cap_close(reply_send_clone);

    // If the reply already arrived out-of-order, return it from the pending buffer first.
    {
        let mut tmp = [0u8; 512];
        if let Some(n) = pending.take_into(nonce, &mut tmp) {
            if response_matches(&tmp[..n], expect_rsp_op, nonce) {
                return Ok(tmp);
            }
        }
    }

    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 512];
    for _ in 0..50_000 {
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if response_matches(&buf[..n], expect_rsp_op, nonce) {
                    return Ok(buf);
                }
                // Unmatched reply on shared inbox: buffer by nonce if it looks like a netstackd reply.
                if let Some(other) = extract_netstack_reply_nonce(&buf[..n]) {
                    let _ = pending.push(other, &buf[..n]);
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return Err(()),
        }
    }
    Err(())
}
