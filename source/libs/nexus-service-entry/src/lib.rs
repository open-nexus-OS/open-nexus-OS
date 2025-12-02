#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    feature(alloc_error_handler)
)]
#![deny(clippy::all, missing_docs)]

//! CONTEXT: Shared userspace entry glue for no_std services
//! OWNERS: @runtime
//! PUBLIC API: `declare_entry!` macro for OS builds
//! DEPENDS_ON: nexus-abi, nexus-sync
//! INVARIANTS: Provides deterministic panic/log handling and a tiny bump allocator
//! ADR: docs/rfcs/RFC-0002-process-per-service-architecture.md

/// Declares the `_start` entry point for OS builds, delegating to `bootstrap`.
///
/// Services should expose an `fn os_entry() -> Result<(), E>` and invoke this macro:
///
/// ```ignore
/// #![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]
/// nexus_service_entry::declare_entry!(crate::os_entry);
/// ```
///
/// For non-OS builds the macro expands to nothing.
#[macro_export]
macro_rules! declare_entry {
    ($path:path) => {
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        #[no_mangle]
        #[link_section = ".text._start"]
        pub extern "C" fn _start() -> ! {
            unsafe { $crate::init_global_pointer() };
            unsafe { $crate::write_boot_marker(b'U') };
            $crate::os::bootstrap(|| $path())
        }
    };
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
/// Initializes the RISC-V `gp` register so small-data accesses work before Rust runs.
pub unsafe fn init_global_pointer() {
    core::arch::asm!(
        ".option push",
        ".option norelax",
        "lla gp, __global_pointer$",
        ".option pop",
        options(nomem, nostack)
    );
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
/// Emits a single diagnostic marker directly to the debug UART.
pub unsafe fn write_boot_marker(byte: u8) {
    let _ = byte;
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
/// OS-specific entry glue providing allocator, panic, and bootstrap helpers.
pub mod os {
    extern crate alloc;

    use core::alloc::{GlobalAlloc, Layout};
    use core::any::type_name;
    use core::panic::PanicInfo;
    use core::ptr;
    use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

    use nexus_abi::{debug_putc, exit};
    #[cfg(feature = "alloc-log")]
    use nexus_log;
    use nexus_sync::SpinLock;

    /// Result type expected from service OS entry functions.
    pub type ServiceResult<E> = core::result::Result<(), E>;

    const HEAP_SIZE: usize = 192 * 1024;
    static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

    struct Bump {
        start: usize,
        end: usize,
        current: usize,
    }

    impl Bump {
        const fn empty() -> Self {
            Self {
                start: 0,
                end: 0,
                current: 0,
            }
        }

        fn init(&mut self, base: usize, size: usize) {
            if self.start == 0 && self.end == 0 {
                self.start = base;
                self.end = base + size;
                self.current = base;
                log_alloc_init(base, self.end);
            }
        }

        fn alloc(&mut self, layout: Layout) -> *mut u8 {
            let align_mask = layout.align().saturating_sub(1);
            let cur_before = self.current;
            let aligned = (cur_before + align_mask) & !align_mask;
            let size = layout.size();
            let result = match aligned.checked_add(size) {
                Some(next) if next <= self.end => {
                    self.current = next;
                    aligned as *mut u8
                }
                _ => core::ptr::null_mut(),
            };
            log_alloc_event(
                size,
                layout.align(),
                cur_before,
                aligned,
                self.current,
                self.end,
                result as usize,
            );
            result
        }
    }

    struct LockedBump {
        inner: SpinLock<Bump>,
        ready: AtomicBool,
    }

    impl LockedBump {
        const fn new() -> Self {
            Self {
                inner: SpinLock::new(Bump::empty()),
                ready: AtomicBool::new(false),
            }
        }

        fn ensure_init(&self) {
            if !self.ready.load(Ordering::Acquire) {
                let mut bump = self.inner.lock();
                if !self.ready.load(Ordering::Relaxed) {
                    #[allow(static_mut_refs)]
                    unsafe {
                        let base = HEAP.as_mut_ptr() as usize;
                        bump.init(base, HEAP_SIZE);
                    }
                    self.ready.store(true, Ordering::Release);
                }
                drop(bump);
            }
        }
    }

    static ALLOCATOR: LockedBump = LockedBump::new();
    static ALLOC_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
    static ALLOC_ZERO_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
    const ALLOC_LOG_LIMIT: usize = 128;
    const ALLOC_ZERO_LOG_LIMIT: usize = 64;

    struct GlobalAllocator;

    unsafe impl GlobalAlloc for GlobalAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCATOR.ensure_init();
            let mut bump = ALLOCATOR.inner.lock();
            let heap_start = bump.start;
            let heap_end = bump.end;
            let cur_before = bump.current;
            let mut ptr = bump.alloc(layout);
            let cur_after = bump.current;
            if ptr.is_null() && layout.size() == 0 {
                ptr = bump.current as *mut u8;
            }
            let exhausted = ptr.is_null() && layout.size() != 0;
            drop(bump);
            if exhausted {
                log_alloc_failure(
                    "alloc",
                    layout.size(),
                    layout.align(),
                    heap_start,
                    heap_end,
                    cur_before,
                    cur_after,
                );
                return ptr;
            }
            ptr
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            ALLOCATOR.ensure_init();
            let mut bump = ALLOCATOR.inner.lock();
            let heap_start = bump.start;
            let heap_end = bump.end;
            let cur_before = bump.current;
            let mut ptr = bump.alloc(layout);
            let cur_after = bump.current;
            if ptr.is_null() && layout.size() == 0 {
                ptr = bump.current as *mut u8;
            }
            let exhausted = ptr.is_null() && layout.size() != 0;
            drop(bump);
            if exhausted {
                log_alloc_failure(
                    "alloc_zeroed",
                    layout.size(),
                    layout.align(),
                    heap_start,
                    heap_end,
                    cur_before,
                    cur_after,
                );
                log_alloc_zeroed(layout.size(), layout.align(), ptr as usize);
                return ptr;
            }

            let mut saved_s5 = 0usize;
            let mut saved_s6 = 0usize;
            if !ptr.is_null() && layout.size() != 0 {
                let addr = ptr as usize;
                let end = addr.checked_add(layout.size()).unwrap_or(usize::MAX);
                if addr < heap_start || end > heap_end {
                    log_alloc_corruption(
                        addr,
                        layout.size(),
                        heap_start,
                        heap_end,
                        cur_before,
                        cur_after,
                    );
                    panic!("alloc_zeroed returned pointer outside heap range");
                }
                unsafe {
                    core::arch::asm!(
                        "mv {saved_s5}, s5",
                        "mv {saved_s6}, s6",
                        "mv s5, {ptr}",
                        "mv s6, {size}",
                        saved_s5 = out(reg) saved_s5,
                        saved_s6 = out(reg) saved_s6,
                        ptr = in(reg) ptr as usize,
                        size = in(reg) layout.size(),
                        options(nostack, preserves_flags)
                    );
                }
                ptr::write_bytes(ptr, 0, layout.size());
                unsafe {
                    core::arch::asm!(
                        "mv s5, {saved_s5}",
                        "mv s6, {saved_s6}",
                        saved_s5 = in(reg) saved_s5,
                        saved_s6 = in(reg) saved_s6,
                        options(nostack, preserves_flags)
                    );
                }
            }
            log_alloc_zeroed(layout.size(), layout.align(), ptr as usize);
            ptr
        }

        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
            // bump allocator does not support deallocation; memory is leaked intentionally
        }
    }

    #[global_allocator]
    static GLOBAL: GlobalAllocator = GlobalAllocator;

    #[alloc_error_handler]
    fn alloc_error(layout: Layout) -> ! {
        debug_write_bytes(b"alloc_error size=0x");
        debug_write_hex(layout.size());
        debug_write_bytes(b" align=0x");
        debug_write_hex(layout.align());
        debug_write_byte(b'\n');
        exit(-1)
    }

    #[panic_handler]
    fn panic(info: &PanicInfo) -> ! {
        debug_write_bytes(b"panic");
        if let Some(location) = info.location() {
            debug_write_bytes(b" file=");
            debug_write_str(location.file());
            debug_write_bytes(b" line=");
            debug_write_dec(location.line() as u64);
            debug_write_bytes(b" col=");
            debug_write_dec(location.column() as u64);
        }
        debug_write_byte(b'\n');
        exit(-1)
    }

    /// Bootstraps the service entrypoint and terminates the task upon completion.
    pub fn bootstrap<E, F>(entry: F) -> !
    where
        F: FnOnce() -> ServiceResult<E>,
    {
        unsafe { super::write_boot_marker(b'B') };
        ALLOCATOR.ensure_init();
        match entry() {
            Ok(()) => exit(0),
            Err(err) => {
                log_service_error::<E>(&err);
                exit(-1)
            }
        }
    }

    fn debug_write_bytes(bytes: &[u8]) {
        for &byte in bytes {
            let _ = debug_putc(byte);
        }
    }

    fn debug_write_byte(byte: u8) {
        let _ = debug_putc(byte);
    }

    fn debug_write_str(s: &str) {
        debug_write_bytes(s.as_bytes());
    }

    fn debug_write_hex(mut value: usize) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut buf = [0u8; core::mem::size_of::<usize>() * 2];
        for idx in (0..buf.len()).rev() {
            buf[idx] = HEX[(value & 0xF) as usize];
            value >>= 4;
        }
        for byte in &buf {
            let _ = debug_putc(*byte);
        }
    }

    fn debug_write_dec(mut value: u64) {
        let mut buf = [0u8; 20];
        let mut idx = buf.len();
        if value == 0 {
            idx -= 1;
            buf[idx] = b'0';
        } else {
            while value != 0 {
                idx -= 1;
                buf[idx] = b'0' + (value % 10) as u8;
                value /= 10;
            }
        }
        for byte in &buf[idx..] {
            let _ = debug_putc(*byte);
        }
    }

    fn log_service_error<E>(_err: &E) {
        debug_write_bytes(b"service error type=");
        debug_write_str(type_name::<E>());
        debug_write_byte(b'\n');
    }

    fn log_alloc_init_debug(base: usize, end: usize) {
        if !probe_logs_enabled() {
            return;
        }
        debug_write_bytes(b"alloc-init base=0x");
        debug_write_hex(base);
        debug_write_bytes(b" end=0x");
        debug_write_hex(end);
        debug_write_byte(b'\n');
    }

    fn log_alloc_event_debug(
        size: usize,
        align: usize,
        cur_before: usize,
        aligned: usize,
        cur_after: usize,
        end: usize,
        result: usize,
    ) {
        if !probe_logs_enabled() {
            return;
        }
        debug_write_bytes(b"alloc size=0x");
        debug_write_hex(size);
        debug_write_bytes(b" align=0x");
        debug_write_hex(align);
        debug_write_bytes(b" cur_before=0x");
        debug_write_hex(cur_before);
        debug_write_bytes(b" aligned=0x");
        debug_write_hex(aligned);
        debug_write_bytes(b" cur_after=0x");
        debug_write_hex(cur_after);
        debug_write_bytes(b" end=0x");
        debug_write_hex(end);
        debug_write_bytes(b" result=0x");
        debug_write_hex(result);
        debug_write_byte(b'\n');
    }

    fn log_alloc_failure(
        site: &str,
        size: usize,
        align: usize,
        heap_start: usize,
        heap_end: usize,
        cur_before: usize,
        cur_after: usize,
    ) {
        debug_write_bytes(b"alloc-fail site=");
        debug_write_str(site);
        debug_write_bytes(b" size=0x");
        debug_write_hex(size);
        debug_write_bytes(b" align=0x");
        debug_write_hex(align);
        debug_write_bytes(b" heap_start=0x");
        debug_write_hex(heap_start);
        debug_write_bytes(b" heap_end=0x");
        debug_write_hex(heap_end);
        debug_write_bytes(b" cur_before=0x");
        debug_write_hex(cur_before);
        debug_write_bytes(b" cur_after=0x");
        debug_write_hex(cur_after);
        debug_write_byte(b'\n');
        #[cfg(feature = "alloc-log")]
        {
            nexus_log::error("alloc", |line| {
                line.text("alloc-fail site=");
                line.text(site);
                line.text(" size=");
                line.hex(size as u64);
                line.text(" align=");
                line.hex(align as u64);
                line.text(" heap_start=");
                line.hex(heap_start as u64);
                line.text(" heap_end=");
                line.hex(heap_end as u64);
                line.text(" cur_before=");
                line.hex(cur_before as u64);
                line.text(" cur_after=");
                line.hex(cur_after as u64);
            });
        }
    }

    fn log_alloc_zero_debug(size: usize, align: usize, result: usize, phase: &str) {
        if !probe_logs_enabled() {
            return;
        }
        debug_write_bytes(b"alloc-zero ");
        debug_write_str(phase);
        debug_write_bytes(b" size=0x");
        debug_write_hex(size);
        debug_write_bytes(b" align=0x");
        debug_write_hex(align);
        debug_write_bytes(b" result=0x");
        debug_write_hex(result);
        debug_write_byte(b'\n');
    }

    fn log_alloc_corruption(
        addr: usize,
        size: usize,
        heap_start: usize,
        heap_end: usize,
        cur_before: usize,
        cur_after: usize,
    ) {
        debug_write_bytes(b"alloc-corrupt addr=0x");
        debug_write_hex(addr);
        debug_write_bytes(b" size=0x");
        debug_write_hex(size);
        debug_write_bytes(b" heap_start=0x");
        debug_write_hex(heap_start);
        debug_write_bytes(b" heap_end=0x");
        debug_write_hex(heap_end);
        debug_write_bytes(b" cur_before=0x");
        debug_write_hex(cur_before);
        debug_write_bytes(b" cur_after=0x");
        debug_write_hex(cur_after);
        debug_write_byte(b'\n');
        #[cfg(feature = "alloc-log")]
        {
            nexus_log::error("alloc", |line| {
                line.text("alloc-corrupt addr=");
                line.hex(addr as u64);
                line.text(" size=");
                line.hex(size as u64);
                line.text(" heap_start=");
                line.hex(heap_start as u64);
                line.text(" heap_end=");
                line.hex(heap_end as u64);
            });
        }
    }

    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    fn probe_logs_enabled() -> bool {
        static STATE: AtomicU8 = AtomicU8::new(2);
        match STATE.load(Ordering::Relaxed) {
            0 => false,
            1 => true,
            _ => {
                let enabled = option_env!("INIT_LITE_LOG_TOPICS")
                    .map(|spec| {
                        spec.split(',')
                            .any(|token| token.trim().eq_ignore_ascii_case("probe"))
                    })
                    .unwrap_or(false);
                STATE.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
                enabled
            }
        }
    }

    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    fn probe_logs_enabled() -> bool {
        false
    }

    #[cfg(feature = "alloc-log")]
    fn log_alloc_init(base: usize, end: usize) {
        if ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_LOG_LIMIT {
            return;
        }
        log_alloc_init_debug(base, end);
        nexus_log::info("alloc", |line| {
            line.text("alloc-init base=");
            line.hex(base as u64);
            line.text(" end=");
            line.hex(end as u64);
        });
    }

    #[cfg(not(feature = "alloc-log"))]
    fn log_alloc_init(base: usize, end: usize) {
        if ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_LOG_LIMIT {
            return;
        }
        log_alloc_init_debug(base, end);
    }

    #[cfg(feature = "alloc-log")]
    fn log_alloc_event(
        size: usize,
        align: usize,
        cur_before: usize,
        aligned: usize,
        cur_after: usize,
        end: usize,
        result: usize,
    ) {
        if ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_LOG_LIMIT {
            return;
        }
        log_alloc_event_debug(size, align, cur_before, aligned, cur_after, end, result);
        nexus_log::info("alloc", |line| {
            line.text("alloc size=");
            line.hex(size as u64);
            line.text(" align=");
            line.hex(align as u64);
            line.text(" cur_before=");
            line.hex(cur_before as u64);
            line.text(" aligned=");
            line.hex(aligned as u64);
            line.text(" cur_after=");
            line.hex(cur_after as u64);
            line.text(" end=");
            line.hex(end as u64);
            line.text(" result=");
            line.hex(result as u64);
        });
    }

    #[cfg(not(feature = "alloc-log"))]
    fn log_alloc_event(
        size: usize,
        align: usize,
        cur_before: usize,
        aligned: usize,
        cur_after: usize,
        end: usize,
        result: usize,
    ) {
        if ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_LOG_LIMIT {
            return;
        }
        log_alloc_event_debug(size, align, cur_before, aligned, cur_after, end, result);
    }

    #[cfg(feature = "alloc-log")]
    fn log_alloc_zeroed(size: usize, align: usize, result: usize) {
        if ALLOC_ZERO_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_ZERO_LOG_LIMIT {
            return;
        }
        log_alloc_zero_debug(size, align, result, "pre");
        nexus_log::info("alloc", |line| {
            line.text("alloc-zero size=");
            line.hex(size as u64);
            line.text(" align=");
            line.hex(align as u64);
            line.text(" result=");
            line.hex(result as u64);
        });
    }

    #[cfg(not(feature = "alloc-log"))]
    fn log_alloc_zeroed(size: usize, align: usize, result: usize) {
        if ALLOC_ZERO_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_ZERO_LOG_LIMIT {
            return;
        }
        log_alloc_zero_debug(size, align, result, "pre");
    }
}
