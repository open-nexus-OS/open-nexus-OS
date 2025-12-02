# RFC-0004: Loader Safety & Shared-Page Guards

- Status: In Progress
- Authors: Runtime Team
- Last Updated: 2025-11-29

## Summary

The current userspace bootstrap path shares a scratch page between the
kernel and the os-lite init loader (now implemented inside `nexus-init` and
invoked via the thin `init-lite` wrapper). Subsequent services inherit pointers into this
shared area (service names, logging targets), which violates the
assumption behind the unified logging facade (RFC-0003) that string data
passed to `LineBuilder` lives in stable, read-only memory. During the
latest debugging session this resulted in repeated traps when the logging
facade attempted to dereference a pointer inside the shared page.

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
- Implementing a generalized capability model for scratch pages—this RFC
  focuses on the init-loader path only.

## Implementation Plan

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

### Phase 1 – Runtime Diagnostics

- Add a lightweight kernel-side trap hook that emits which guard rejected
  a pointer (e.g. logging guard vs. loader provenance).
- Surface failures in `init-lite` with a concise error tag instead of
  cascading traps.

### Phase 2 – Deferred Enhancements

- Consider allocating metadata pages via the address-space manager instead
  of per-service VMOs, enabling tighter accounting.
- Explore making the scratch page per-task (swap the shared page for a
  ring buffer or per-service staging area).

## Current Status

- Phase 0: **In progress.** Guard logic in `nexus-log` already rejects
  pointers that escape the `.rodata`/`.data` fences. On the loader side we
  now:
  - Allocate per-service VMOs from a dedicated 16 MiB arena that is identity
    mapped behind the kernel stacks so user payloads can never alias kernel RAM.
  - Enforce W^X at the syscall boundary (`sys_as_map`) and in the page-table
    layer, so writable aliases to executable segments are rejected up front.
  - Wire `sys_vmo_write` with a real copy path that validates user pointers
    and writes directly into the arena backing each VMO.
  - Keep the old bootstrap guard page as a `NOLOAD` section, ensuring the new
    metadata copies never overlap it.
  Outstanding tasks are zeroing the bootstrap scratch page after each spawn,
  introducing explicit `PROT_NONE` guard gaps between writable segments, and
  teaching the loader self-tests to assert the new invariants.
- Phase 1: Not started.
- Phase 2: Not started.

### Hybrid Guard Instrumentation (2025-11-29)

While Phase 0 continues, we introduced a stop-gap “hybrid” layer to make pointer
faults deterministic:

- The `nexus-init` os-lite loader now tracks every `PT_LOAD` and guard VMA it maps via a
  `RangeTracker`. The tracker enforces non-overlap (segments vs. guards) before
  issuing `as_map` calls and logs a structured `guard-conflict` error if a
  caller ever attempts to reuse an address range. This mirrors seL4’s PMP-based
  guard accounting without requiring kernel changes.
- The loader’s per-service metadata strings are copied into small bounce buffers
  with canaries prior to logging. If a caller scribbles over the buffer, the
  canary trips immediately and the fault is attributed to the correct site.
- Together with the RFC‑0003 slice validation the hybrid layer ensures that a
  corrupt pointer cannot flow silently from the loader into the logging façade:
  the loader catches overlap/guard issues, while the logger bounds-checks every
  slice and reports the originating return address.
- Sink-side instrumentation now records the emitting log’s `[LEVEL target]` when
  a guard violation or oversized write is detected, giving clear blame data for
  any remaining corruption that bypasses loader checks.

These measures are documented here so we can treat them as part of RFC‑0004’s
Phase 0 completion criteria rather than temporary debugging hacks.

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
