#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use nexus_abi::yield_;

/// Callback invoked when the cooperative bootstrap has reached a stable state.
pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    /// Create a new notifier from the supplied closure.
    pub fn new(func: F) -> Self {
        Self(func)
    }

    /// Execute the wrapped callback.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Placeholder error type used by the os-lite backend.
#[derive(Debug)]
pub enum InitError {}

/// No-op for parity with the std backend which warms schema caches.
pub fn touch_schemas() {}

/// Sequential bootstrap loop that emits stage0-compatible UART markers and
/// cooperatively yields control back to the scheduler.
pub fn service_main_loop<F>(notifier: ReadyNotifier<F>) -> Result<(), InitError>
where
    F: FnOnce() + Send,
{
    emit_line("init: start");
    notifier.notify();
    emit_line("init: ready");
    loop {
        let _ = yield_();
    }
}

fn emit_line(message: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        for b in message.as_bytes() {
            uart_write_byte(*b);
        }
        uart_write_byte(b'\n');
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
