#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod os_entry {
    use nexus_abi::{debug_putc, ipc_recv_v1, ipc_send_v1, MsgHeader};
    use nexus_service_entry::declare_entry;

    extern crate alloc;
    use alloc::vec::Vec;

    declare_entry!(pong_main);

    fn pong_main() -> Result<(), ()> {
        fn print(s: &str) {
            for b in s.bytes() {
                let _ = debug_putc(b);
            }
        }

        print("PONG: starting\n");

        // Receive endpoint is in slot 3 (set by init-lite)
        let ep_recv = 3;
        let ep_send = 4;

        let mut recv_buf = Vec::with_capacity(8192);
        recv_buf.resize(8192, 0);

        print("PONG: ready\n");

        loop {
            let mut rhdr = MsgHeader::new(0, 0, 0, 0, 0);
            if ipc_recv_v1(ep_recv, &mut rhdr, &mut recv_buf, 0, 0).is_ok() {
                let payload_len = rhdr.len as usize;
                let _ = ipc_send_v1(ep_send, &rhdr, &recv_buf[..payload_len], 0, 0);
            }
        }
    }
}
