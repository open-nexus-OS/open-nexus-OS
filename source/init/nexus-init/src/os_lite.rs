#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use nexus_abi::yield_;

pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    pub fn new(func: F) -> Self {
        Self(func)
    }

    pub fn notify(self) {
        (self.0)();
    }
}

#[derive(Debug)]
pub enum InitError {}

pub fn touch_schemas() {}

pub fn service_main_loop<F>(notifier: ReadyNotifier<F>) -> Result<(), InitError>
where
    F: FnOnce() + Send,
{
    println("init: start\n");
    notifier.notify();
    println("init: ready\n");
    loop {
        let _ = yield_();
    }
}

fn println(s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        for b in s.as_bytes() {
            uart_write_byte(*b);
        }
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
