// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel main bring-up and idle loop
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU selftests/marker ladder (see scripts/qemu-test.sh)
//! PUBLIC API: kmain()
//! DEPENDS_ON: hal::VirtMachine, mm::AddressSpaceManager, sched::Scheduler, ipc::Router
//! INVARIANTS: Activate kernel AS before complex init; cooperative scheduling via SYSCALL_YIELD
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use alloc::vec::Vec;
use core::{fmt::Write as _, mem::MaybeUninit};

use crate::ipc;
use crate::ipc::header::MessageHeader;
use crate::{
    cap::{Capability, CapabilityKind, Rights},
    hal::virt::VirtMachine,
    hal::{IrqCtl, Tlb},
    mm::{AddressSpaceManager, AsHandle},
    sched::{EnqueueOutcome, QosClass, Scheduler},
    selftest,
    syscall::{api, SyscallTable},
    task::{Pid, TaskTable},
    types::CpuId,
};

// (no private selftest stack; kernel stack is provisioned by linker)

/// Aggregated kernel state initialised during boot.
struct KernelState {
    hal: VirtMachine,
    scheduler: Scheduler,
    tasks: TaskTable,
    ipc: ipc::Router,
    address_spaces: AddressSpaceManager,
    #[allow(dead_code)]
    kernel_as: AsHandle,
    #[allow(dead_code)]
    syscalls: SyscallTable,
    #[cfg(target_os = "none")]
    #[allow(dead_code)]
    hart_timers: crate::timer::HartTimers,
    #[cfg(target_os = "none")]
    #[allow(dead_code)]
    waitsets: crate::waitset::WaitsetTable,
    #[cfg(target_os = "none")]
    #[allow(dead_code)]
    fences: crate::fence::FenceTable,
}

static mut KERNEL_STATE: MaybeUninit<KernelState> = MaybeUninit::uninit();

#[allow(static_mut_refs)]
unsafe fn init_kernel_state() -> &'static mut KernelState {
    unsafe { KERNEL_STATE.write(KernelState::new()) }
}

impl KernelState {
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    fn new() -> Self {
        panic!("KernelState::new is only available on riscv64 none target");
    }

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn new() -> Self {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let cap_new: fn() -> crate::cap::CapTable = crate::cap::CapTable::new;
            let cap_wc: fn(usize) -> crate::cap::CapTable = crate::cap::CapTable::with_capacity;
            unsafe {
                let inst_new = core::ptr::read_volatile((cap_new as usize) as *const u32);
                let inst_wc = core::ptr::read_volatile((cap_wc as usize) as *const u32);
                if inst_new == 0 || inst_wc == 0 {
                    panic!("RX-CHK: zero instruction at beacon");
                }
            }
        }
        // Bring up address-space manager and activate kernel AS before any
        // complex code paths (like task/cap table initialisation) to ensure
        // the kernel text/data mappings are active in SATP.
        let mut address_spaces = AddressSpaceManager::new();
        let kernel_as = match address_spaces.create() {
            Ok(handle) => handle,
            Err(err) => {
                use core::fmt::Write as _;
                let mut w = crate::uart::raw_writer();
                let _ = write!(w, "KS-E: as_create {:?}\n", err);
                panic!("kernel address space create failed");
            }
        };
        if let Err(err) = address_spaces.attach(kernel_as, Pid::KERNEL) {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "KS-E: as_attach {:?}\n", err);
        }
        // Activate kernel address space immediately to ensure deterministic
        // RX mapping for subsequent code paths.
        #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "bringup_identity"))]
        if let Err(err) = address_spaces.activate_via_trampoline(kernel_as) {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "KS-E: as_activate tramp {:?}\n", err);
            panic!("kernel address space activate via trampoline failed");
        }
        #[cfg(not(all(
            target_arch = "riscv64",
            target_os = "none",
            feature = "bringup_identity"
        )))]
        if let Err(err) = address_spaces.activate(kernel_as) {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "KS-E: as_activate {:?}\n", err);
            panic!("kernel address space activate failed");
        }

        // The kernel AS is now active, so the identity-mapped fw_cfg window is reachable.
        // Probe the boot mode (proof vs interactive) once — this gates whether later kernel
        // boot markers fold into the verdict grid (interactive) or stay raw (proof). Safe to
        // call here; defaults to raw on any failure. Alloc-free (fixed buffers, MMIO reads).
        crate::boot_mode::detect();

        // Now proceed with task table and the rest of bring-up under the active SATP.
        let mut tasks = TaskTable::new();
        // If an early trap occurred, print it once to aid bring-up debugging.
        if let Some(tf) = crate::trap::last_trap() {
            let mut u = crate::uart::raw_writer();
            let _ = write!(
                u,
                "TRAP-EARLY: scause=0x{:x} sepc=0x{:x} stval=0x{:x}\n",
                tf.scause, tf.sepc, tf.stval
            );
        }
        // Slot 0: bootstrap endpoint loopback
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            let _ = caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV,
                },
            );
            // Slot 1: identity VMO for bootstrap mappings
            let _ = caps.set(
                1,
                Capability {
                    kind: CapabilityKind::Vmo { base: 0x8000_0000, len: 0x10_0000 },
                    rights: Rights::MAP,
                },
            );
            // Slot 2: endpoint-factory authority (init-lite receives a derived copy).
            let _ = caps.set(
                2,
                Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE },
            );
        }
        // Bind kernel AS handle to bootstrap task
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut scheduler = Scheduler::new();
        if matches!(scheduler.enqueue(Pid::KERNEL, QosClass::Normal), EnqueueOutcome::Rejected(_)) {
            panic!("scheduler bootstrap enqueue rejected");
        }

        let mut syscalls = SyscallTable::new();
        api::install_handlers(&mut syscalls);

        let router = ipc::Router::new(8);

        let hal = VirtMachine::new();
        #[cfg(feature = "debug_uart")]
        log_debug!(target: "kmain", "KS: after VirtMachine::new");

        Self {
            hal,
            scheduler,
            tasks,
            ipc: router,
            address_spaces,
            kernel_as,
            syscalls,
            hart_timers: crate::timer::HartTimers::new(),
            waitsets: crate::waitset::WaitsetTable::new(),
            fences: crate::fence::FenceTable::new(),
        }
    }

    #[allow(dead_code)]
    fn banner(&self) {
        // Decorative boot banner — emitted RAW (no `[LEVEL target]` prefix) so the logo reads
        // as art, not log lines. The single intentional exception to the `[ts] TAG content`
        // console grid. The version line keeps the literal `neuron vers.` (the harness's first
        // proof-of-life marker) so the marker ladder stays green.
        use crate::uart::write_line;
        write_line("");
        write_line(r"     _ __   ___ _   _ _ __ ___  _ __");
        write_line(r"    | '_ \ / _ \ | | | '__/ _ \| '_ \");
        write_line(r"    | | | |  __/ |_| | | | (_) | | | |");
        write_line(r"    |_| |_|\___|\__,_|_|  \___/|_| |_|");
        write_line("");
        write_line("    neuron vers. 0.1.0  ·  One OS. Many Devices.");
        write_line("");
        self.assert_memory_layout();
    }

    /// P0.1 layout audit (2026-07-07): the boot proceeds over several
    /// FIXED-ADDRESS windows (stack pool, kernel page pool, user VMO arena)
    /// whose neighbors move with the image. A silent overlap corrupts
    /// distant subsystems (the `.data`-cursor zero-guards in StackPool /
    /// alloc_init_page exist BECAUSE this happened before). Check the
    /// invariants ONCE at boot and report loudly — with values — instead of
    /// failing later as an anonymous StackExhausted/oom.
    fn assert_memory_layout(&self) {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            extern "C" {
                static __bss_end: u8;
            }
            // P0.1 perturbation-gate anchor: a compile-time-sized rodata pad
            // that is genuinely REFERENCED (volatile read below), so no
            // linker pass can ever collect it. Appending an unreferenced
            // `#[used] #[no_mangle]` static to a compiled file proved to be
            // a PLACEBO — gc-sections dropped it and the gate's landing
            // check (`pad did not land`) caught exactly that. Sized via
            // `NEURON_LAYOUT_PAD` (contract-image-layout.sh); 0 = zero cost.
            const LAYOUT_PAD_LEN: usize = {
                match option_env!("NEURON_LAYOUT_PAD") {
                    Some(s) => {
                        let b = s.as_bytes();
                        let mut v = 0usize;
                        let mut i = 0;
                        while i < b.len() {
                            if b[i] >= b'0' && b[i] <= b'9' {
                                v = v * 10 + (b[i] - b'0') as usize;
                            }
                            i += 1;
                        }
                        v
                    }
                    None => 0,
                }
            };
            static LAYOUT_PAD: [u8; LAYOUT_PAD_LEN] = [0xA5; LAYOUT_PAD_LEN];
            let pad_probe: usize = if LAYOUT_PAD_LEN > 0 {
                // Volatile read takes the array's ADDRESS — the pad must
                // exist in the image (no const-fold, no GC).
                unsafe { core::ptr::read_volatile(LAYOUT_PAD.as_ptr()) as usize }
            } else {
                0
            };
            let image_end = core::ptr::addr_of!(__bss_end) as usize;
            let pool_base = crate::mm::KERNEL_PAGE_POOL_BASE;
            let pool_end = pool_base + crate::mm::KERNEL_PAGE_POOL_LEN;
            let arena_base = crate::mm::USER_VMO_ARENA_BASE;
            let arena_end = arena_base + crate::mm::USER_VMO_ARENA_LEN;
            let mut ok = true;
            if image_end > pool_base {
                log_error!(
                    "LAYOUT: kernel image end 0x{:x} overlaps page pool 0x{:x} — image grew past the window",
                    image_end,
                    pool_base
                );
                ok = false;
            }
            if pool_end > arena_base {
                log_error!(
                    "LAYOUT: page pool end 0x{:x} overlaps VMO arena 0x{:x}",
                    pool_end,
                    arena_base
                );
                ok = false;
            }
            // Headroom report (values, not vibes): how far the image may
            // still grow before it hits the pool window.
            let headroom = pool_base.saturating_sub(image_end);
            // Armed pad must really be in memory with its fill byte — the
            // volatile read above took its address, this checks its content.
            if LAYOUT_PAD_LEN > 0 && pad_probe != 0xA5 {
                log_error!("LAYOUT: pad probe mismatch (read 0x{:x}, want 0xa5)", pad_probe);
                ok = false;
            }
            if ok {
                log_info!(
                    target: "kmain",
                    "KERNEL: layout ok (image_end=0x{:x} pool=0x{:x} headroom={}K arena_end=0x{:x} pad={})",
                    image_end,
                    pool_base,
                    headroom / 1024,
                    arena_end,
                    LAYOUT_PAD_LEN
                );
            }
            // Under 64K of slack is a red flag long before it is a failure.
            if ok && headroom < 64 * 1024 {
                log_error!(
                    "LAYOUT: only {} bytes headroom between kernel image and page pool",
                    headroom
                );
            }
        }
    }

    #[allow(dead_code)]
    fn exercise_ipc(&mut self) {
        // Send a bootstrap message to prove IPC wiring works before tasks run.
        let header = MessageHeader::new(0, 0, 0x100, 0, 0);
        if self.ipc.send(0, ipc::Message::new(header, Vec::new(), None)).is_ok() {
            let _ = self.ipc.recv(0);
        }
    }
}

/// Kernel main invoked after boot assembly completed.
/// CRITICAL: Activate kernel address space before complex init; idle loop uses SYSCALL_YIELD.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn kmain() -> ! {
    #[cfg(feature = "boot_timing")]
    let t0 = crate::arch::riscv::read_time();
    let kernel = unsafe { init_kernel_state() };
    crate::smp::init_boot_hart_state();
    // Provide the syscall/trap runtime with a stable timer reference. We store it as a raw pointer
    // to avoid borrowing `kernel.hal` for `'static` (we keep mutating `kernel` afterwards).
    let timer: &'static dyn crate::hal::Timer = unsafe {
        let t: &dyn crate::hal::Timer = kernel.hal.timer();
        &*(t as *const dyn crate::hal::Timer)
    };
    let _default_trap_domain = crate::trap::install_runtime(
        &mut kernel.scheduler,
        &mut kernel.tasks,
        &mut kernel.ipc,
        &mut kernel.address_spaces,
        timer,
        &mut kernel.hart_timers,
        &mut kernel.waitsets,
        &mut kernel.fences,
        &kernel.syscalls,
    );
    kernel.tasks.bootstrap_mut().set_trap_domain(_default_trap_domain);

    let expected_online_mask = crate::smp::start_secondary_harts();
    // Bounded HSM bring-up with retry: a start request issued in quick
    // succession can get lost (hart never reaches the kernel entry despite a
    // success return); retry up to 3 times before declaring the timeout.
    let mut online_ok = crate::smp::wait_for_online_mask(expected_online_mask, 500_000_000);
    for _ in 0..3 {
        if online_ok {
            break;
        }
        crate::smp::retry_missing_harts(expected_online_mask);
        online_ok = crate::smp::wait_for_online_mask(expected_online_mask, 500_000_000);
    }
    // Boot gate: per-hart evidence + verdict. A lost hart is loud but NOT
    // fatal — the system boots degraded with the online set (the SMP proof
    // gate still requires its markers, so CI catches it honestly).
    crate::smp::emit_bringup_gate(expected_online_mask);
    let _ = online_ok;
    // Touch HAL traits to satisfy imports
    let uart_dev = kernel.hal.uart();
    let _: &dyn crate::hal::Uart = uart_dev;
    kernel.hal.tlb().flush_all();
    kernel.hal.irq().disable(0);
    kernel.hal.irq().enable(0);
    #[cfg(feature = "boot_timing")]
    {
        let t1 = crate::arch::riscv::read_time();
        let delta = (t1 - t0) as u64;
        use core::fmt::Write as _;
        let mut u = crate::uart::KernelUart::lock();
        let _ = write!(u, "T:init={}\n", delta);
    }
    // Minimal IO only
    #[cfg(feature = "boot_banner")]
    kernel.banner();
    // Keep timer interrupts disabled during selftest to avoid preemption/trap stack usage
    // Keep UART usage minimal before selftests
    // kernel.exercise_ipc();
    // (no pointer debug formatting in OS stage policy)
    // Quick sanity for OpenSBI environment
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        #[cfg(feature = "debug_uart")]
        log_info!(target: "env", "ENV: sbi present");
    }
    #[cfg(feature = "boot_timing")]
    let t2 = crate::arch::riscv::read_time();
    {
        let mut ctx = selftest::Context {
            hal: &kernel.hal,
            router: &mut kernel.ipc,
            address_spaces: &mut kernel.address_spaces,
            tasks: &mut kernel.tasks,
            scheduler: &mut kernel.scheduler,
            hart_timers: &mut kernel.hart_timers,
            waitsets: &mut kernel.waitsets,
            fences: &mut kernel.fences,
        };
        // Gate: validate target PC and dump RA/SP before entering selftest
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            extern "C" {
                static __text_start: u8;
                static __text_end: u8;
            }
            let target_pc = selftest::entry as usize;
            let start = unsafe { &__text_start as *const u8 as usize };
            let end = unsafe { &__text_end as *const u8 as usize };
            if !(target_pc >= start && target_pc < end && (target_pc & 0x3) == 0) {
                panic!("GATE: invalid selftest entry pc=0x{:x}", target_pc);
            }
            let _ra: usize;
            let _sp: usize;
            unsafe {
                core::arch::asm!("mv {o}, ra", o = out(reg) _ra, options(nostack, preserves_flags));
            }
            unsafe {
                core::arch::asm!("mv {o}, sp", o = out(reg) _sp, options(nostack, preserves_flags));
            }
            #[cfg(feature = "debug_uart")]
            {
                let mut u = crate::uart::raw_writer();
                let _ = write!(
                    u,
                    "GATE: before selftest ra=0x{:x} sp=0x{:x} pc=0x{:x}\n",
                    _ra, _sp, target_pc
                );
            }
        }
        // Prefer private selftest stack on OS if enabled; applies to full suite and spawn-only
        #[cfg(all(feature = "selftest_priv_stack", target_arch = "riscv64", target_os = "none"))]
        selftest::entry_on_private_stack(&mut ctx);
        #[cfg(not(all(
            feature = "selftest_priv_stack",
            target_arch = "riscv64",
            target_os = "none"
        )))]
        selftest::entry(&mut ctx);
        // Userspace acceptance markers are emitted by daemons and selftest-client.
        // RFC-0068: fold the bounded kernel-init phases (kself, syscalls, sched, boot, smp) into one
        // `<subject> N/N OK <ms>` grid verdict each (interactive boots only; proof boots emitted
        // them raw for verify-uart). By this point — after the selftest, before the idle loop hands
        // off to userspace — each of these phases has emitted all its markers, so the flush pairs
        // with the suppression in diag::log so no folded marker is ever dropped without a verdict.
        // (`as` flushes separately, on its rate-limiter, since it is fed by userspace switches.)
        crate::log::kflush_kernel_phase();
    }
    #[cfg(feature = "boot_timing")]
    {
        let t3 = crate::arch::riscv::read_time();
        let delta = (t3 - t2) as u64;
        use core::fmt::Write as _;
        let mut u = crate::uart::KernelUart::lock();
        let _ = write!(u, "T:selftest={}\n", delta);
    }
    // End of kernel bring-up; user-mode services are responsible for
    // emitting their own readiness markers.

    // Arm the reactive, preemptive scheduler tick. Selftests have completed and the
    // trap runtime is installed, so from here a supervisor timer IRQ (1) delivers
    // fired timer caps reactively and (2) preempts long-running user tasks, so no
    // single service can monopolise the cooperative scheduler. Done last to keep all
    // earlier bring-up/selftest sequencing non-preemptive.
    #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
    unsafe {
        crate::trap::timer_arm(crate::trap::DEFAULT_TICK_CYCLES);
        crate::trap::enable_timer_interrupts();
    }

    // Reactive device input: initialise the PLIC and unmask supervisor external
    // interrupts. No source is enabled until a driver binds its IRQ (irq_bind), so
    // nothing fires here; binding routes the device IRQ to the driver's endpoint.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        crate::hal::plic::plic_init();
        riscv::register::sie::set_sext();
    }

    // A6 structural proofs (SMP>=2 only, right after plic_init): every online
    // hart's S-context is addressable + initialised (threshold readback), and
    // per-context enable bitmaps are isolated (a probe source enabled for the
    // boot context must not leak into cpu1's).
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    if crate::smp::cpu_online_mask().count_ones() > 1 {
        let mut ctx_ok = true;
        for idx in 0..crate::smp::MAX_CPUS {
            let cpu = CpuId::from_raw(idx as u16);
            if !crate::smp::cpu_is_online(cpu) {
                continue;
            }
            if crate::hal::plic::selftest_ctx_threshold(cpu) == 0 {
                log_info!(target: "smp", "KSELFTEST: plic ctx cpu{} ok", idx);
            } else {
                ctx_ok = false;
                log_error!(target: "smp", "KSELFTEST: plic ctx cpu{} FAIL", idx);
            }
        }
        // Probe source 90 is unwired on QEMU virt (virtio 1..8, uart 10,
        // rtc 11, pci 32..35) — enabling it cannot fire anything.
        if let Some(probe) = crate::hal::plic::IrqId::new(90) {
            let cpu1 = CpuId::from_raw(1);
            crate::hal::plic::enable_source(probe, CpuId::BOOT);
            let boot_sees = crate::hal::plic::selftest_source_enabled(probe, CpuId::BOOT);
            let cpu1_sees = crate::hal::plic::selftest_source_enabled(probe, cpu1);
            crate::hal::plic::disable_source(probe, CpuId::BOOT);
            if ctx_ok && boot_sees && !cpu1_sees {
                log_info!(target: "smp", "KSELFTEST: plic isolation ok");
            } else {
                log_error!(
                    target: "smp",
                    "KSELFTEST: plic isolation FAIL boot={} cpu1={}",
                    boot_sees,
                    cpu1_sees
                );
            }
        }
    }

    // A7: announce the active per-hart timer mechanism. SBI set_timer is the
    // v1 path; SSTC (`stimecmp`, saves the SBI trap per tick) is a documented
    // follow-up pending a fault-safe detection probe.
    log_info!(target: "smp", "KINIT: timer sbi");

    // A3: selftests + init spawn are done — release the secondaries into
    // their scheduler loops (IPI punches them out of their park-WFI), then
    // become CPU 0's scheduler.
    crate::smp::mark_runtime_ready();
    for idx in 1..crate::smp::MAX_CPUS {
        let target = CpuId::from_raw(idx as u16);
        if crate::smp::cpu_is_online(target) {
            let _ = crate::smp::request_resched(target);
        }
    }
    crate::cpu_main::cpu_main(CpuId::BOOT)
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn kmain() -> ! {
    panic!("kmain is only available on riscv64 none target");
}
