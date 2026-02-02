#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]

//! CONTEXT: virtioblkd (v0) — virtio-blk MMIO owner proof service (TASK-0010 consumer)
//! OWNERS: @runtime
//! STATUS: Experimental (proof-only)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Proven via QEMU markers (scripts/qemu-test.sh)

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
fn os_entry() -> core::result::Result<(), ()> {
    use nexus_abi::{debug_println, mmio_map, yield_};

    // Deterministic MMIO cap slot owned by init distribution.
    const MMIO_CAP_SLOT: u32 = 48;
    const MMIO_VA: usize = 0x2000_e000;
    // virtio-mmio IDs
    const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976; // "virt"
    const VIRTIO_DEVICE_ID_BLK: u32 = 2;

    let _ = debug_println("virtioblkd: ready");

    // Retry briefly in case init distributes after service starts.
    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(1_000_000_000);
    loop {
        match mmio_map(MMIO_CAP_SLOT, MMIO_VA, 0) {
            Ok(()) => break,
            Err(_) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    let _ = debug_println("virtioblkd: mmio map FAIL");
                    loop {
                        let _ = yield_();
                    }
                }
                let _ = yield_();
            }
        }
    }

    let magic = unsafe { core::ptr::read_volatile((MMIO_VA + 0x000) as *const u32) };
    let device_id = unsafe { core::ptr::read_volatile((MMIO_VA + 0x008) as *const u32) };
    if magic == VIRTIO_MMIO_MAGIC && device_id == VIRTIO_DEVICE_ID_BLK {
        let _ = debug_println("virtioblkd: mmio window mapped ok");
    } else {
        let _ = debug_println("virtioblkd: mmio window mapped FAIL");
    }

    loop {
        let _ = yield_();
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite")))]
fn main() {}
