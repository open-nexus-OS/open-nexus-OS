#![no_std]

/// Minimal mailbox protocol: [u16 opcode][u16 reserved][u32 len][bytes...]
pub mod wire {
    #[repr(C, packed)]
    #[derive(Clone, Copy, Debug)]
    pub struct Header {
        pub opcode: u16,
        pub reserved: u16,
        pub len: u32,
    }

    impl Header {
        pub fn to_bytes(self) -> [u8; 8] {
            let mut buf = [0u8; 8];
            buf[0..2].copy_from_slice(&self.opcode.to_le_bytes());
            buf[2..4].copy_from_slice(&self.reserved.to_le_bytes());
            buf[4..8].copy_from_slice(&self.len.to_le_bytes());
            buf
        }
    }
}

/// Single-process cooperative mailbox built atop yield(); no real queues yet.
pub mod mailbox {
    use super::wire::Header;

    pub const MAX_FRAME: usize = 512;

    static mut REQ: [u8; MAX_FRAME] = [0; MAX_FRAME];
    static mut RSP: [u8; MAX_FRAME] = [0; MAX_FRAME];
    static mut REQ_LEN: usize = 0;
    static mut RSP_LEN: usize = 0;

    pub fn client_send(opcode: u16, payload: &[u8]) {
        let hdr = Header { opcode, reserved: 0, len: payload.len() as u32 }.to_bytes();
        unsafe {
            REQ[..8].copy_from_slice(&hdr);
            REQ[8..8 + payload.len()].copy_from_slice(payload);
            REQ_LEN = 8 + payload.len();
        }
    }

    pub fn client_recv(buf: &mut [u8]) -> usize {
        // Cooperative wait
        loop {
            unsafe {
                if RSP_LEN != 0 {
                    let n = core::cmp::min(buf.len(), RSP_LEN);
                    buf[..n].copy_from_slice(&RSP[..n]);
                    RSP_LEN = 0;
                    return n;
                }
            }
            let _ = nexus_abi::yield_();
        }
    }

    pub fn server_poll(req: &mut [u8]) -> usize {
        unsafe {
            let n = REQ_LEN;
            if n != 0 {
                let m = core::cmp::min(req.len(), n);
                req[..m].copy_from_slice(&REQ[..m]);
                REQ_LEN = 0;
                return m;
            }
        }
        0
    }

    pub fn server_reply(bytes: &[u8]) {
        unsafe {
            let n = core::cmp::min(bytes.len(), MAX_FRAME);
            RSP[..n].copy_from_slice(&bytes[..n]);
            RSP_LEN = n;
        }
    }
}


