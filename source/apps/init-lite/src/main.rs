#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
use core::panic::PanicInfo;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
use nexus_abi as abi;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[no_mangle]
#[link_section = ".text._start"]
pub extern "C" fn _start() -> ! {
    let _ = abi::debug_putc(b'!');
    let _ = abi::debug_println("init: start");
    let _ = abi::yield_();
    let _ = abi::debug_println("init: ready");
    loop {
        for _ in 0..1024 { core::hint::spin_loop(); }
        let _ = abi::yield_();
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {}

// DEPRECATED: This app is superseded by `source/init/nexus-init`.
// Prefer using the init library with std_server/os_lite backends.
// See ADR: docs/adr/0001-runtime-roles-and-boundaries.md
