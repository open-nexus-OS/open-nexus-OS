# RFC-0004: Loader Safety & Shared-Page Guards

- Status: Phase 0 Complete (Security Floor; maintenance ongoing); Phase 1/2 Deferred
- Authors: Runtime Team
- Last Updated: 2025-12-19

## Status at a Glance

- **Phase 0 (Security Floor)**: Complete ✅
- **Phase 1 (Runtime Diagnostics)**: Complete ✅
- **Phase 2 (Deferred Enhancements)**: Not started (optional; deferred)

Definition:

- In this RFC, “Complete” refers to **Phase 0 Security Floor** being complete (loader/mapping
  invariants enforced + tests/proof). Phase 1/2 are explicitly **non-blocking** follow-ups.
- Phase 0 “complete” is not “never touch again”: if we discover a new loader/guard invariant that
  materially raises the security floor (and can be proven via tests/markers), we may land it and
  update this RFC while still keeping Phase 1/2 deferred.

## Phase 0 Security Floor – Checklist + Proof Matrix

This section is the “what must stay true” contract for RFC‑0004. Each item should have an explicit
proof signal (unit test, kernel selftest marker, or deterministic boot telemetry).

| Invariant | What it means | Proof |
| --- | --- | --- |
| Zeroed allocations | User-visible allocations (VMO backing, exec PT_LOAD backing, spawn stacks) do not leak stale bytes | `KSELFTEST: vmo zero ok` |
| User-pointer guard | Syscalls that copy from user memory reject pointers outside Sv39 user VA range (and null) deterministically | `KSELFTEST: userptr guard ok` |
| W^X enforced | No mapping may be writable and executable; RX segments are never writable | `KSELFTEST: w^x enforced` + exec mapper checks |
| Per-service immutable metadata | Service name and bootstrap metadata are mapped read-only per service; no shared mutable “scratch” pointers | `[INFO exec] EXEC-META: ...` telemetry + `BootstrapInfo` RO mapping |
| Guard pages remain unmapped | Stack boundary + bootstrap info guard page remain unmapped to catch OOB | `STACK-CHECK` telemetry + `exec_v2` guard assertion |

## Summary

The current bootstrap path now uses a kernel `exec` loader; `nexus-init` only
forwards packaged ELFs via the thin `init-lite` wrapper. Earlier userspace
mapping code shared a scratch page between kernel and init, leaking pointers
and causing traps. With mapping/guards moved into the kernel, this RFC scopes
the guard/provenance expectations that must stay enforced in the kernel loader
and documents the shutdown of the old userspace loader path.

This RFC defines a scoped plan for hardening the loader and its shared
artifacts so that each service owns immutable copies of the metadata it
relies on, and crash diagnostics can trust pointer provenance.

## Motivation

- **Pointer provenance**: Logging and diagnostic code must be able to
  reject rogue pointers deterministically. Copying metadata into an
  immutable VMO per service keeps provenance simple and auditable.
- **Shared-page lifetime**: The bootstrap buffer is reused across service
  loads. Without isolation, stale data survives between services and can
  surface as bogus pointers later.
- **Deterministic crash analysis**: When traps occur, we need to know if
  the fault is due to true memory bugs or stale bootstrap remnants.

## Goals

1. Ensure all service metadata (names, strings, log targets) resides in a
   dedicated, read-only mapping per service.
2. Clear or recycle the shared scratch page after each spawn so no stale
   pointers leak to the next service.
3. Extend the loader self-tests to verify mapping invariants and detect
   regressions early.
4. Provide optional trap-time diagnostics to highlight pointer violations
   that reach the kernel.

## Non-Goals

- Rewriting the `nexus-loader` crate or ELF parser beyond the minimum
  adjustments needed for metadata isolation.
- Changing the syscall ABI for logging or introducing asynchronous log
  routing (tracked in RFC-0003).
- Defining the kernel IPC and capability semantics for services (tracked in RFC-0005).
  This RFC focuses on loader/mapping/guard safety and provenance constraints that IPC must obey.

## Implementation Plan

### Completion snapshot (2025-12-19) — Phase 0 (Security Floor)

- Phase 0 complete. Kernel `exec_v2` owns ELF mapping, enforces W^X at the syscall boundary and during PT_LOAD mapping, and maintains guarded stacks. The user VMO arena is **zero-initialized by construction** (no stale bytes leak across allocations), and a kernel selftest asserts this property (`KSELFTEST: vmo zero ok`). Per-service metadata is provided via a read-only metadata page plus a read-only `BootstrapInfo` page. Best-effort guard-gap assertions ensure we do not accidentally map into existing page-aligned gaps between PT_LOAD segments, and the page above the `BootstrapInfo` page remains unmapped (guard).

### Phase 0 – Immediate Hardening (Current Quarter)

- Copy service name strings into a per-service read-only VMO before
  passing them to logging APIs, and drop all writable aliases once the
  metadata is published.
- Zero (or unmap) the bootstrap scratch page once a service has been
  spawned so no stale pointers persist.
- Map each ELF `PT_LOAD` segment with the exact protection bits it
  requires and introduce guard pages as standalone `PROT_NONE` mappings
  rather than inflating the data segment. Never request or grant `RW | X`
  mappings; enforce W^X in the kernel’s `as_map`/page-table layer so a
  rogue caller cannot obtain a writable alias to executable pages.
- Add loader unit tests that assert copied strings and guard pages live
  inside the immutable VMOs and that RX segments are never mapped writable.
- Document pointer provenance expectations for services that link against
  `nexus-service-entry`.
- Keep the enhanced debugging instrumentation (UART probes, guard counters)
  in place until the new StrRef/VMO path is finished, so we can detect regressions
  quickly during bring-up.
- Kernel `exec` loader stack policy (in effect): map stack downward from the
  fixed top, include a boundary page above the top-of-stack address, place SP at
  least one page below the mapped top, and keep a guard page above. Emit
  STACK-MAP / STACK-CHECK telemetry and add selftests that verify `top-1` is
  mapped and `top` faults to catch regressions early.

### Phase 1 – Runtime Diagnostics

- Complete. Kernel trap handling now emits a concise tag when a user page-fault hits a known loader
  guard page (e.g. stack guard or bootstrap-info guard), making guard violations immediately
  attributable during bring-up.
- Init surfaces failures with concise labels (see `ipc_error_label` / `abi_error_label` in `nexus-init`).

### Phase 2 – Deferred Enhancements

- Consider allocating metadata pages via the address-space manager instead
  of per-service VMOs, enabling tighter accounting.
- Explore making the scratch page per-task (swap the shared page for a
  ring buffer or per-service staging area).

## Current Status

- Phase 0: **Complete.** Guard logic in `nexus-log` already rejects
  pointers that escape the `.rodata`/`.data` fences. On the loader side we
  now:
  - Allocate per-service VMOs from a dedicated 16 MiB arena that is identity
    mapped behind the kernel stacks so user payloads can never alias kernel RAM.
  - Zero-initialize all arena allocations (VMOs, exec PT_LOAD backing, stacks) to
    prevent stale bytes and pointer remnants leaking across services/allocations.
  - Zero freshly allocated spawn stacks and identity-map the stack pool so the kernel can clear it
    deterministically (no stale user stack contents).
  - `exec_v2` syscall can copy the service name into a per-service, read-only metadata page mapped
    into the child address space (kernel emits `EXEC-META` telemetry during boot).
  - The kernel also maps a **read-only bootstrap info page** (struct `BootstrapInfo`) at a stable
    VA adjacent to the stack mapping. This page provides `meta_name_ptr/meta_name_len` so early
    services (and later IPC bootstrap) can find provenance-safe metadata without shared scratch
    pages.
  - Enforce W^X at the syscall boundary (`sys_as_map`) and in the page-table
    layer, so writable aliases to executable segments are rejected up front.
  - Wire `sys_vmo_write` with a real copy path that validates user pointers
    and writes directly into the arena backing each VMO.
  - Keep the old bootstrap guard page as a `NOLOAD` section, ensuring the new
    metadata copies never overlap it.
  - Guard pages / gaps:
    - The page above the `BootstrapInfo` page is kept unmapped (guard).
    - Page-aligned gaps between PT_LOAD segments remain unmapped; the kernel asserts it does not
      accidentally map into such gaps during `exec_v2` bring-up (best-effort; ELFs are not rejected
      if they have no gaps).
  As the loader moves into a kernel `exec` path, these invariants remain
  requirements for the kernel implementation.
- Phase 1: Not started (optional diagnostics).
- Phase 2: Not started.

### Hybrid Guard Instrumentation (2025-11-29)

While Phase 0 continues, we introduced a stop-gap “hybrid” layer to make pointer
faults deterministic. With the kernel `exec` path live, the userspace pieces have
been retired; the relevant bits remain in the kernel loader:

- Guard accounting (PT_LOAD + guard VMAs) now lives in the kernel `exec` mapper,
  which enforces non-overlap and W^X before committing page-table entries.
- The logger still bounds-checks every slice and reports the originating return
  address; userspace bounce buffers were removed alongside the old loader.
- Sink-side instrumentation continues to record the emitting log’s
  `[LEVEL target]` when a guard violation or oversized write is detected.

## Relationship to Other RFCs

- Depends on RFC-0003 for unified logging assumptions (string provenance,
  pointer guards) and now reuses its topic/guard infrastructure.
- Complements RFC-0002 by ensuring service bootstrapping remains robust
  when multiple processes are spawned in sequence and by enforcing W^X
  invariants at the kernel boundary.

## Testing

- Extend the shared loader tests (`nexus-init` os-lite backend, invoked via the
  init-lite wrapper) to assert that every service metadata pointer resides inside the per-service VMO (`cargo test -p nexus-init` on
  the host side).
- Kernel selftest: `KSELFTEST: vmo zero ok` asserts that `vmo_create` returns zeroed backing bytes (QEMU marker-driven).
- Kernel selftest: `KSELFTEST: userptr guard ok` asserts that syscalls which copy from user memory
  reject pointers outside the Sv39 user VA range deterministically (e.g. `SYSCALL_VMO_WRITE`).
- Integration test: boot `neuron-boot` with the service list enabled and
  assert that no `[USER-PF]` traces occur due to logging pointers.
- Manual trap-review: verify trap dumps now identify guard failures
  explicitly once Phase 1 is implemented.

## Open Questions

- Should the scratch page be unmapped between service loads to catch
  remaining references eagerly?
- Do we want to introduce a dedicated loader capability type for metadata
  VMOs, or is documenting the provenance sufficient for now?
- How should we expose guard failures to higher level tooling
  (e.g. integrate with the planned logging control plane in RFC-0003)?
