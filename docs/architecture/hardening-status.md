# Kernel Hardening Objectives Status

This checklist captures the state of the hardening objectives from the October 15 brief. It is meant to provide a quick answer to whether the current tree satisfies every requested item.

| Objective | Scope | Status | Notes |
| --- | --- | --- | --- |
| 1. Kernel mapping | Final-image linker symbols, GLOBAL flags, RX guard | ✅ Completed | `map_kernel_segments` uses `__text_start/__text_end` (RX) and `__bss_end` plus stack symbols for RW mappings and emits the required marker. The RX guard reads bytes before any SATP switch. |
| 2. SATP switch island | Dedicated same-page trampoline and post-switch marker | ✅ Completed | All SATP activations route through the switch island, which performs the RX sanity probe, swaps stacks within the identity page, fences, and emits `AS: post-satp OK`. |
| 3. Syscall discipline | Typed decoders, canonical VAs, W^X denial | ✅ Completed | `types.rs` defines `VirtAddr`, `PageLen`, `AsHandle`, and `SlotIndex`; syscall decoders enforce alignment, canonicality, and W⊕X. |
| 4. Spawn in child AS | Fresh Sv39 AS with guarded stack or caller-provided AS | ✅ Completed | `TaskTable::spawn` allocates a guarded stack for new address spaces, validates entry PCs, and respects custom handles. |
| 5. Self-tests & markers | Ordered acceptance markers | ✅ Completed | Selftests emit the requested `KSELFTEST` markers and rely on feature-gated verbosity. |
| 6. Debug/dev hardening | Guard pages, lockdep-light, heap redzones, PT verifier, trap ring buffer | ✅ Completed | Kernel and selftest stacks gain unmapped guards, the SATP island reuses the guarded kernel stack, debug builds expose a trap ring buffer (`trap_ring`) and optional `trap_symbols`, and CI triage aborts on PANIC/EXC/ILLEGAL/RX markers. |
| 7. Verifiable-style refactors | Newtypes, pure helpers, structured logging | ✅ Completed | Typed IDs/lengths gate syscall inputs, helper routines stay pure, and the kernel now routes output through leveled log macros so debug chatter is suppressed in release builds. |

**Bottom line:** The hardening objectives are satisfied for mapping/guards/W^X/typed decoding.

**Current state note (2025-12-18):** syscall handlers return expected errors as `-errno` in `a0`.
The kernel may still terminate tasks in true “no forward progress” situations (e.g. repeated
ECALL storms), but ordinary syscall errors are returned to userspace.

## Related canonical references

- Kernel overview: `docs/architecture/01-neuron-kernel.md`
- Kernel + layering quick reference: `docs/ARCHITECTURE.md`
- Testing methodology and CI marker discipline: `docs/testing/index.md` and `scripts/qemu-test.sh`
