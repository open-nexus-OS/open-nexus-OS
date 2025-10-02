#![no_std]
#![no_main]

use core::panic::PanicInfo;

// QEMU RISC-V 'virt' UART0 (NS16550A)
const UART0: usize = 0x1000_0000;
const UART_THR: usize = 0x00; // transmit holding register
const UART_LSR: usize = 0x05; // line status register
const LSR_THRE: u8 = 1 << 5; // transmitter holding register empty

#[inline(always)]
fn mmio8(off: usize) -> *mut u8 {
    (UART0 + off) as *mut u8
}

fn uart_putc(b: u8) {
    unsafe {
        while core::ptr::read_volatile(mmio8(UART_LSR)) & LSR_THRE == 0 {}
        core::ptr::write_volatile(mmio8(UART_THR), b);
    }
}

fn uart_write(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' {
            uart_putc(b'\r');
        }
        uart_putc(b);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Disable S-mode interrupts and clear pending bits
    unsafe {
        core::arch::asm!(
            "csrw sie, zero",     // mask all S-mode interrupts
            "csrw sip, zero",     // clear any pending S-mode interrupts
            "li t0, 0x2",         // SSTATUS.SIE bit
            "csrc sstatus, t0",   // clear SIE
            out("t0") _,
            options(nostack)
        );
    }

    // Park all HARTs the same way (we don’t rely on a single boot hart yet)
    uart_write("=== NEURON (open-nexus-os) ===\n");
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    uart_write("panic\n");
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
