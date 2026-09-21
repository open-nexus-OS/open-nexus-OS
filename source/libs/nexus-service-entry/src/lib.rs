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

// The arena RULE, kept free of the allocator so it can be proven on the host
// (ADR-0065). Under `nexus_env="os"` the `GlobalAlloc` glue below consumes it;
// on the host only the unit tests do — hence the cfg-conditional allow, in the
// shape this tree already uses for host-tested SSOT logic
// (`windowd/src/interaction.rs`). Never a blanket allow.
#[cfg_attr(not(any(test, nexus_env = "os")), allow(dead_code))]
mod generation;
// The durable-state half of ADR-0065 exists only where a service opted in;
// a service without the feature never compiles it (cfg-gate, never allow).
#[cfg(any(test, feature = "small-object-free-list"))]
#[cfg_attr(not(any(test, nexus_env = "os")), allow(dead_code))]
mod freelist;

mod cursor_tripwire;
mod debug_write;

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
pub use os::{ready, stage};

/// Host builds: no control channel exists, so the readiness announce is a no-op and the
/// marker is not printed (host services log through their own std paths). The error type is
/// a host-only placeholder so call sites keep one `?`-able shape on both targets.
#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostReadyError;

/// Host counterpart of [`os::ready`]: nothing to announce, always `Ok(())`.
#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
pub fn ready(_marker: &str) -> core::result::Result<(), HostReadyError> {
    Ok(())
}

/// Host counterpart of [`os::stage`]: there is no control channel to report a boot stage on,
/// so the report is dropped. Stages exist only in a real boot (ADR-0062).
#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
pub fn stage(_stage: nexus_service_topology::Stage) {}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
/// OS-specific entry glue providing allocator, panic, and bootstrap helpers.
pub mod os {
    extern crate alloc;

    use core::alloc::Layout;
    use core::any::type_name;
    use core::panic::PanicInfo;
    use core::slice;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use crate::debug_write::{
        debug_write_byte, debug_write_bytes, debug_write_dec, debug_write_hex, debug_write_str,
    };
    pub use crate::generation::Region;
    use nexus_abi::exit;
    #[cfg(feature = "alloc-log")]
    use nexus_log;
    use nexus_sync::SpinLock;

    /// Result type expected from service OS entry functions.
    pub type ServiceResult<E> = core::result::Result<(), E>;

    // Bring-up sizing: bounded heaps (kernel early-linear-map budget). Most
    // services: 384KiB; proof clients 512/768KiB. windowd 2MiB (input bursts
    // + greeter bake overflowed 1MiB). Shell app-host history: 4MiB (profile
    // remounts page-faulted 2MiB), then 8MiB, then 16MiB — each raise bought
    // a few dozen more clicks, because every structural interaction rebuilt a
    // frame onto a bump that never frees. That story ended with ADR-0065
    // (TASK-0077C): frame output lives in the generation arena below and
    // durable state frees by size class, so app-host is back on the 4MiB
    // floor and its heap is flat over a session — proven by the boot marker
    // `apphost: heap steady`. The 50/75/90% watermark lines that warned of
    // the old ceiling are gone with it.
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

    /// The frame arena (ADR-0065), OPT-IN per service: a service that does not
    /// enable `frame-arena` does not get the array, so its image and its .bss
    /// are unchanged to the byte. Only a service that rebuilds and discards a
    /// frame on every interaction needs it, and today that is the app-host.
    ///
    /// Sized from the measurement in TASK-0077C — one layout call allocates
    /// 226 560 B against 119 824 B retained — with room for the text runs
    /// beside it. Two halves, so one generation is 512 KiB. The budget probe
    /// prints the real peak; this is a ceiling to tighten against, not a guess
    /// to live with.
    #[cfg(feature = "frame-arena")]
    const ARENA_SIZE: usize = 1024 * 1024;
    #[cfg(not(feature = "frame-arena"))]
    const ARENA_SIZE: usize = 0;
    static mut ARENA: [u8; ARENA_SIZE] = [0; ARENA_SIZE];

    struct Bump {
        start: usize,
        end: usize,
        current: usize,
        /// The frame arena, ONE PAIR PER REGION (TASK-0077C P2b): emit and
        /// layout reset on their own schedules. Empty and inert unless a
        /// service opts in AND opens a scope; `place()` then answers
        /// `BaseHeap` and nothing changes.
        gens: [crate::generation::Generations; crate::generation::Region::COUNT],
        /// Size-class free lists for DURABLE state (ADR-0065, TASK-0077C P3).
        /// Opt-in per service; a service without the feature has no field.
        #[cfg(feature = "small-object-free-list")]
        lists: crate::freelist::FreeLists,
    }

    impl Bump {
        const fn empty() -> Self {
            Self {
                start: 0,
                end: 0,
                current: 0,
                gens: [crate::generation::Generations::empty(); crate::generation::Region::COUNT],
                #[cfg(feature = "small-object-free-list")]
                lists: crate::freelist::FreeLists::empty(),
            }
        }

        fn init(&mut self, base: usize, size: usize) {
            if self.start == 0 && self.end == 0 {
                self.start = base;
                self.end = base + size;
                self.current = base;
                // Split the region evenly between the phases.
                #[allow(static_mut_refs)]
                unsafe {
                    let per = ARENA_SIZE / crate::generation::Region::COUNT;
                    let base = ARENA.as_mut_ptr() as usize;
                    for (i, pair) in self.gens.iter_mut().enumerate() {
                        pair.init(base + i * per, per);
                    }
                }
                log_alloc_init(base, self.end);
            }
        }

        /// Whether `addr` lies in the frame arena — such a block is reclaimed by
        /// its generation's reset and must never be pushed onto a free list.
        fn in_arena(addr: usize) -> bool {
            let base = core::ptr::addr_of!(ARENA) as usize;
            ARENA_SIZE != 0 && addr >= base && addr < base + ARENA_SIZE
        }

        /// The class-rounded layout a small request is carved with, so the
        /// block can serve ANY later request of its class (`crate::freelist`).
        #[cfg(feature = "small-object-free-list")]
        fn class_layout(layout: Layout) -> (Option<usize>, Layout) {
            match crate::freelist::class_for(layout.size(), layout.align()) {
                Some(class) => (
                    Some(class),
                    // SAFETY: every class size is a non-zero multiple of the
                    // (power-of-two) block alignment.
                    unsafe {
                        Layout::from_size_align_unchecked(
                            crate::freelist::CLASS_SIZES[class],
                            crate::freelist::BLOCK_ALIGN,
                        )
                    },
                ),
                None => (None, layout),
            }
        }

        #[cfg(not(feature = "small-object-free-list"))]
        fn class_layout(layout: Layout) -> (Option<usize>, Layout) {
            (None, layout)
        }

        /// A block a free list can hand back for this layout, if one is parked.
        #[cfg(feature = "small-object-free-list")]
        fn reuse(&mut self, class: Option<usize>) -> Option<*mut u8> {
            // SAFETY: only `dealloc` pushes, only with blocks this allocator
            // carved for that class and that nothing references any more.
            class.and_then(|c| unsafe { self.lists.pop(c) }).map(|addr| addr as *mut u8)
        }

        #[cfg(not(feature = "small-object-free-list"))]
        fn reuse(&mut self, _class: Option<usize>) -> Option<*mut u8> {
            None
        }

        /// The arena's answer for this layout, or `None` when the base heap
        /// below should serve it (no scope open, no arena, or a spill).
        fn arena_alloc(&mut self, layout: Layout) -> Option<*mut u8> {
            // Whichever region has a scope open answers; at most one does, and
            // outside every scope this is two predictable branches.
            for pair in &mut self.gens {
                if let crate::generation::Place::Arena(addr) =
                    pair.place(layout.size(), layout.align())
                {
                    return Some(addr as *mut u8);
                }
            }
            None
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
    mod frame_arena;
    pub use frame_arena::{
        arena_stats, close_frame_generation, frame_generation, free_list_stats, generation_count,
        FrameGeneration,
    };
    pub use global_alloc::carve_stats;

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

    mod global_alloc;

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

    mod ready;
    pub use ready::{ready, stage, wait_for_stage};

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
        // ADR-0062: the boot stage this service belongs to must have opened before it runs.
        // This is the ONLY synchronization a service does at startup — no resume order, no
        // yields, no time caps decide when it starts.
        wait_for_stage();
        match entry() {
            Ok(()) => exit(0),
            Err(err) => {
                log_service_error::<E>(&err);
                exit(-1)
            }
        }
    }

    #[inline(always)]
    pub(crate) fn service_name() -> &'static str {
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

    fn log_service_error<E>(_err: &E) {
        debug_write_bytes(b"service error svc=");
        debug_write_str(service_name());
        debug_write_bytes(b" type=");
        debug_write_str(type_name::<E>());
        debug_write_byte(b'\n');
    }

    mod alloc_log;
    use alloc_log::*;
}
