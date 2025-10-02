#![no_std]
#![no_main]
#![forbid(clippy::unwrap_used)]

mod arch;
pub mod cap;
pub mod ipc;
pub mod sched;
pub mod trap;
pub mod vm;

use core::fmt::{self, Write};
use core::panic::PanicInfo;

struct Uart;

impl Uart {
    fn write_byte(byte: u8) {
        let base = 0x1000_0000 as *mut u8;
        unsafe {
            core::ptr::write_volatile(base, byte);
        }
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            Self::write_byte(byte);
        }
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut uart = Uart;
    let _ = writeln!(uart, "=== NEURON (open-nexus-os) ===");
    arch::riscv::init();
    boot_loop();
}

fn boot_loop() -> ! {
    loop {
        arch::riscv::wait_for_interrupt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut uart = Uart;
    let _ = writeln!(uart, "panic: {}", info);
    loop {
        arch::riscv::wait_for_interrupt();
    }
}
