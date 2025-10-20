#![no_std]

pub struct ReadyNotifier<F: FnOnce()>(F);

impl<F: FnOnce()> ReadyNotifier<F> {
    pub fn new(func: F) -> Self { Self(func) }
    pub fn notify(self) { (self.0)() }
}

pub fn service_main_loop<F: FnOnce()>(notifier: ReadyNotifier<F>) -> Result<(), ()> {
    println("vfsd: ready\n");
    notifier.notify();
    loop {
        let _ = nexus_abi::yield_();
    }
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
