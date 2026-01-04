#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod os {
    use core::convert::Infallible;

    use nexus_abi::{debug_putc, yield_};
    use nexus_service_entry::{declare_entry, os::ServiceResult};

    declare_entry!(debug_main);

    #[allow(unreachable_code)]
    fn debug_main() -> ServiceResult<Infallible> {
        write_line("debugsvc: start");
        loop {
            write_line("debugsvc: alive");
            let _ = yield_();
        }
        #[allow(unreachable_code)]
        Ok(())
    }

    fn write_line(s: &str) {
        write_bytes(s.as_bytes());
        let _ = debug_putc(b'\n');
    }

    fn write_bytes(bytes: &[u8]) {
        for &b in bytes {
            let _ = debug_putc(b);
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() {
    println!("debugsvc host stub");
}
