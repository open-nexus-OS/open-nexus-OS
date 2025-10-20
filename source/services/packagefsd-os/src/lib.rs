#![no_std]

use os_mailbox_lite::mailbox;

pub struct ReadyNotifier<F: FnOnce()>(F);

impl<F: FnOnce()> ReadyNotifier<F> {
    pub fn new(func: F) -> Self { Self(func) }
    pub fn notify(self) { (self.0)() }
}

// Opcodes
const OPC_RESOLVE: u16 = 1;

pub fn service_main_loop<F: FnOnce()>(notifier: ReadyNotifier<F>) -> Result<(), ()> {
    println("packagefsd: ready\n");
    notifier.notify();
    let mut req = [0u8; 512];
    loop {
        let n = mailbox::server_poll(&mut req);
        if n == 0 { let _ = nexus_abi::yield_(); continue; }
        if n >= 8 {
            let opcode = u16::from_le_bytes([req[0], req[1]]);
            let len = u32::from_le_bytes([req[4], req[5], req[6], req[7]]) as usize;
            let payload = &req[8..8 + len.min(n.saturating_sub(8))];
            match opcode {
                OPC_RESOLVE => {
                    // payload: rel path bytes (no NUL)
                    let (ok, size, kind, bytes) = resolve(payload);
                    let mut resp = [0u8; 512];
                    resp[0..1].copy_from_slice(&[if ok {1} else {0}]);
                    resp[1..9].copy_from_slice(&size.to_le_bytes());
                    resp[9..11].copy_from_slice(&kind.to_le_bytes());
                    let copy_len = core::cmp::min(bytes.len(), resp.len().saturating_sub(11));
                    resp[11..11+copy_len].copy_from_slice(&bytes[..copy_len]);
                    mailbox::server_reply(&resp[..11+copy_len]);
                }
                _ => mailbox::server_reply(&[0]),
            }
        }
    }
}

fn resolve(rel: &[u8]) -> (bool, u64, u16, &'static [u8]) {
    // Minimal demo namespace: provide manifest.json and payload.elf content
    if rel == b"demo.hello/manifest.json" || rel == b"/demo.hello/manifest.json" {
        let bytes = b"{\"name\":\"demo.hello\"}";
        return (true, bytes.len() as u64, 0, bytes);
    }
    if rel == b"demo.hello/payload.elf" || rel == b"/demo.hello/payload.elf" {
        let bytes = b"HELLO_PAYLOAD_BYTES................................";
        return (true, bytes.len() as u64, 0, bytes);
    }
    (false, 0, 0, &[])
}

fn println(s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        for b in s.as_bytes() { uart_write_byte(*b); }
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn uart_write_byte(byte: u8) {
    const UART0_BASE: usize = 0x1000_0000;
    const UART_TX: usize = 0x0;
    const UART_LSR: usize = 0x5;
    const LSR_TX_IDLE: u8 = 1 << 5;
    unsafe {
        while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
        if byte == b'\n' {
            core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, b'\r');
            while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
        }
        core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, byte);
    }
}
