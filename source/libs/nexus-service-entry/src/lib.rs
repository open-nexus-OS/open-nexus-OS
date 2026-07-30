// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

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
//! ADR: docs/adr/0017-service-architecture.md

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
            $crate::os::set_service_name(env!("CARGO_PKG_NAME"));
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

/// True when the build opted into verbose logging via `INIT_LITE_LOG_TOPICS=trace|verbose`.
/// Compile-time string; used to gate developer-only output that must stay off by default.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) fn verbose_build() -> bool {
    option_env!("INIT_LITE_LOG_TOPICS")
        .map(|spec| {
            spec.split(',').any(|token| {
                let token = token.trim();
                token.eq_ignore_ascii_case("trace") || token.eq_ignore_ascii_case("verbose")
            })
        })
        .unwrap_or(false)
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
/// Emits a single pre-allocator proof-of-life marker to the debug UART. These raw bytes have
/// no newline and would fuse with the next line, so they are gated to verbose builds only —
/// a normal boot stays clean; a debug build (`INIT_LITE_LOG_TOPICS=trace`) restores them.
pub unsafe fn write_boot_marker(byte: u8) {
    if verbose_build() {
        let _ = nexus_abi::debug_putc(byte);
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
/// OS-specific entry glue providing allocator, panic, and bootstrap helpers.
pub mod os {
    extern crate alloc;

    use core::alloc::{GlobalAlloc, Layout};
    use core::any::type_name;
    use core::panic::PanicInfo;
    use core::ptr;
    use core::slice;
    use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

    use nexus_abi::{debug_putc, exit};
    #[cfg(feature = "alloc-log")]
    use nexus_log;
    use nexus_sync::SpinLock;

    /// Result type expected from service OS entry functions.
    pub type ServiceResult<E> = core::result::Result<(), E>;

    // Bring-up sizing: bounded heaps (kernel early-linear-map budget). Most
    // services: 384KiB; proof clients 512/768KiB. windowd 2MiB (input bursts
    // + greeter bake overflowed 1MiB). Shell app-host history: 4MiB (profile
    // remounts page-faulted 2MiB), then 8MiB (theme = drop-first remount with
    // an open overlay panel faulted at exactly base+4MiB). 16MiB (app-host):
    // every structural DSL interaction re-emits scene + layout + texts onto
    // this never-freeing bump (~100-300KiB/click, settings-sized page) —
    // 8MiB froze after a few dozen clicks. Honest fix (emit-generation
    // arena) recorded in TASK-0311; the watermark markers below make the
    // remaining ceiling visible before it freezes.
    const HEAP_SIZE: usize = if cfg!(feature = "heap-16m") {
        16 * 1024 * 1024
    } else if cfg!(feature = "heap-8m") {
        8 * 1024 * 1024
    } else if cfg!(feature = "heap-4m") {
        4 * 1024 * 1024
    } else if cfg!(feature = "heap-2m") {
        2 * 1024 * 1024
    } else if cfg!(feature = "heap-1m") {
        1024 * 1024
    } else if cfg!(feature = "heap-768k") {
        768 * 1024
    } else if cfg!(feature = "heap-512k") {
        512 * 1024
    } else {
        384 * 1024
    };
    static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

    struct Bump {
        start: usize,
        end: usize,
        current: usize,
        /// Reported high-water marks (bitmask 50/75/90%): bump exhaustion
        /// was a silent freeze — one UART line per threshold warns first.
        watermarks: u8,
    }

    impl Bump {
        const fn empty() -> Self {
            Self { start: 0, end: 0, current: 0, watermarks: 0 }
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
            self.note_watermark();
            result
        }

        fn note_watermark(&mut self) {
            let total = self.end.saturating_sub(self.start);
            let used = self.current.saturating_sub(self.start);
            for (bit, pct) in [(1u8, 50usize), (2, 75), (4, 90)] {
                if total > 0 && self.watermarks & bit == 0 && used >= total / 100 * pct {
                    self.watermarks |= bit;
                    log_heap_watermark(pct, used, total);
                }
            }
        }
    }

    /// Crossed-watermark UART line (alloc-free: runs inside the allocator).
    fn log_heap_watermark(pct: usize, used: usize, total: usize) {
        debug_write_bytes(b"heap-watermark svc=");
        debug_write_str(service_name());
        for (label, v) in [(b" pct=" as &[u8], pct), (b" used=0x", used), (b" of=0x", total)] {
            debug_write_bytes(label);
            debug_write_hex(v);
        }
        debug_write_byte(b'\n');
    }

    struct LockedBump {
        inner: SpinLock<Bump>,
        ready: AtomicBool,
    }

    impl LockedBump {
        const fn new() -> Self {
            Self { inner: SpinLock::new(Bump::empty()), ready: AtomicBool::new(false) }
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
    /// Task #14 tripwire: the bump cursor is MONOTONE by construction. A
    /// cursor that moves backwards means the allocator state was overwritten
    /// (e.g. a stack cliff into .bss) — the root of overlapping allocations
    /// and "impossible" data corruption. Checked on every alloc; violation
    /// is reported once, loudly.
    static LAST_CURSOR: AtomicUsize = AtomicUsize::new(0);
    static CURSOR_REGRESSED: AtomicBool = AtomicBool::new(false);

    fn check_cursor_monotone(cur_after: usize) {
        let prev = LAST_CURSOR.swap(cur_after, Ordering::AcqRel);
        if cur_after < prev && !CURSOR_REGRESSED.swap(true, Ordering::AcqRel) {
            debug_write_bytes(b"!alloc-cursor-regressed svc=");
            debug_write_str(service_name());
            debug_write_bytes(b" prev=0x");
            debug_write_hex(prev);
            debug_write_bytes(b" now=0x");
            debug_write_hex(cur_after);
            debug_write_byte(b'\n');
        }
    }

    /// Heap introspection for proofs: (start, current, end).
    pub fn heap_cursor() -> (usize, usize, usize) {
        ALLOCATOR.ensure_init();
        let bump = ALLOCATOR.inner.lock();
        (bump.start, bump.current, bump.end)
    }
    static ALLOC_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
    static ALLOC_ZERO_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
    const ALLOC_LOG_LIMIT: usize = 128;
    const ALLOC_ZERO_LOG_LIMIT: usize = 64;
    static SERVICE_NAME_PTR: AtomicUsize = AtomicUsize::new(0);
    static SERVICE_NAME_LEN: AtomicUsize = AtomicUsize::new(0);

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
            check_cursor_monotone(cur_after);
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
            check_cursor_monotone(cur_after);
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
                // Task #14 root cause lived here: a debug shim moved ptr/size
                // into s5/s6 around this write WITHOUT declaring the clobber.
                // Depending on register allocation (i.e. on the binary
                // layout), it destroyed live caller values — the wild follow-up
                // stores reset the bump cursor and overlapped live
                // allocations ("impossible" empty/à-la-carte corruption in
                // f32/alloc-heavy code). Plain write_bytes is all this needs.
                ptr::write_bytes(ptr, 0, layout.size());
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
        debug_write_bytes(b"alloc_error svc=");
        debug_write_str(service_name());
        debug_write_bytes(b" size=0x");
        debug_write_hex(layout.size());
        debug_write_bytes(b" align=0x");
        debug_write_hex(layout.align());
        debug_write_byte(b'\n');
        exit(-1)
    }

    #[panic_handler]
    fn panic(info: &PanicInfo) -> ! {
        debug_write_bytes(b"panic svc=");
        debug_write_str(service_name());
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
        configure_verbosity_from_env();
        // Verdict folding: ask the kernel (single fw_cfg-derived source) whether this is an
        // interactive boot. If so, a service's routine markers fold into one `<service> N/N`
        // verdict; proof boots leave it off so `verify-uart` still sees every raw marker.
        nexus_abi::set_verdict_fold(nexus_abi::boot_should_fold_verdicts());
        // RFC-0068: arm EVERY service entering here so its scattered `debug_println` runtime traces
        // also fold — pre-`ready` markers tally into the `<service> N/N` verdict, post-`ready` traces
        // fold into recall-only detail (`NEXUS_LOG_EXPAND=<svc>`). Every service that reaches this
        // entry flushes a verdict, so no folded line is lost; failures and proof boots always print.
        nexus_abi::service_verdict_arm();
        // Configurable per-group expand: if this service is named in `NEXUS_LOG_EXPAND` (a comma list),
        // opt it out of folding so its full raw marker stream shows while every other group stays
        // compactly folded — `NEXUS_LOG_EXPAND=netstackd just start` to focus one service.
        if service_expand_requested() {
            nexus_abi::set_verdict_expand(true);
        }
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

    #[inline(always)]
    fn service_name() -> &'static str {
        let ptr = SERVICE_NAME_PTR.load(Ordering::Relaxed);
        let len = SERVICE_NAME_LEN.load(Ordering::Relaxed);
        if ptr == 0 || len == 0 {
            return "unknown";
        }
        let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
        unsafe { core::str::from_utf8_unchecked(bytes) }
    }

    /// True when this service is listed in the `NEXUS_LOG_EXPAND` build env (a comma-separated set of
    /// group names) — then it prints raw (un-folded) so its full detail shows while others stay
    /// folded. Compile-time today (`just start` rebuilds); a runtime fw_cfg source can replace this.
    fn service_expand_requested() -> bool {
        match option_env!("NEXUS_LOG_EXPAND") {
            Some(list) => {
                let name = service_name();
                list.split(',').any(|g| g.trim() == name)
            }
            None => false,
        }
    }

    #[inline(always)]
    /// Records the service name for allocator/panic diagnostics.
    pub fn set_service_name(name: &'static str) {
        SERVICE_NAME_PTR.store(name.as_ptr() as usize, Ordering::Relaxed);
        SERVICE_NAME_LEN.store(name.len(), Ordering::Relaxed);
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
        debug_write_bytes(b"service error svc=");
        debug_write_str(service_name());
        debug_write_bytes(b" type=");
        debug_write_str(type_name::<E>());
        debug_write_byte(b'\n');
    }

    fn log_alloc_init_debug(base: usize, end: usize) {
        if !probe_logs_enabled() {
            return;
        }
        debug_write_bytes(b"alloc-init svc=");
        debug_write_str(service_name());
        debug_write_bytes(b" base=0x");
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
        debug_write_bytes(b"alloc svc=");
        debug_write_str(service_name());
        debug_write_bytes(b" size=0x");
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
        debug_write_bytes(b"alloc-fail svc=");
        debug_write_str(service_name());
        debug_write_bytes(b" site=");
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
                line.text("alloc-fail svc=");
                line.text(service_name());
                line.text(" site=");
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
        debug_write_bytes(b"alloc-zero svc=");
        debug_write_str(service_name());
        debug_write_bytes(b" ");
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
                        spec.split(',').any(|token| token.trim().eq_ignore_ascii_case("probe"))
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

    /// One-time per-process verbosity setup from the build-time `INIT_LITE_LOG_TOPICS` knob.
    /// A `trace` or `verbose` token re-enables developer breadcrumbs (`debug_trace`) and lifts
    /// the `nexus_log` floor to Debug; without it the process stays quiet (Warn/Info). This is
    /// the "flags" path: detail is one rebuild-time token away, never deleted.
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    fn configure_verbosity_from_env() {
        if super::verbose_build() {
            nexus_abi::set_debug_trace(true);
            #[cfg(feature = "alloc-log")]
            nexus_log::set_max_level(nexus_log::Level::Debug);
        }
    }

    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    fn configure_verbosity_from_env() {}

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
