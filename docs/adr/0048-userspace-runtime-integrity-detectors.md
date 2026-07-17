# ADR-0048: standing userspace runtime-integrity detectors

## Status

Accepted (2026-07-17). Implemented and boot-proven.

## Context

During the compute-broker work we hit "impossible" symptoms: the same pure
function over the same input produced different results per call instance in
the selftest process — empty rasters, boot-varying pixel counts — while
another process computed the host-exact answer. The failure was
address/layout-sensitive and survived every plausible-suspect review
(registers, soft-float, libm, dec2flt, memset/memcpy, allocator zeroing).

Root cause: a leftover debug shim in `nexus-service-entry::alloc_zeroed`
moved values into `s5`/`s6` around `write_bytes` WITHOUT declaring the
clobbers to the compiler. Depending on register allocation — i.e. on the
binary layout of that build — it destroyed live caller registers; the wild
follow-up stores reset the bump allocator's cursor, so later allocations
overlapped live ones. Every downstream symptom (empty plans, varying
digests, likely earlier worker deaths) was this one undeclared-clobber UB.

The lesson is structural: silent corruption classes (undeclared asm
clobbers, stack cliffs into .bss, allocator-state damage, broken builtins)
must be caught by STANDING, self-localizing detectors — not re-diagnosed
from scratch each time (the boot-gate doctrine).

## Decision

1. **Inline asm rule** (binding): every inline-asm block declares ALL
   registers it writes (`out`/`lateout`/explicit clobbers). Debug shims that
   stage values in callee-saved registers are forbidden — that is what
   `SELFTEST: regsoak` exists for instead.
2. **Allocator tripwire**: the service bump allocator checks cursor
   MONOTONICITY on every allocation. A cursor that moves backwards means its
   state was overwritten; it reports `!alloc-cursor-regressed svc=… prev=…
   now=…` once, loudly. `heap_cursor()` exposes (start, current, end) for
   probes.
3. **Standing selftest detectors** (bringup phase, markers declared in the
   proof manifest):
   - `SELFTEST: regsoak ok` — seeds s2–s11/t0–t6 with patterns, spins across
     several timer preemptions, verifies (kernel save/restore + asm-clobber
     detector; reports a per-register bitmask on failure);
   - `SELFTEST: f32 soak ok` — soft-float + libm sweeps against HOST-PINNED
     checksums (shared SSOT in `pinched::broker`, host tests regenerate);
   - `SELFTEST: alloc soak ok` / `SELFTEST: memset soak ok` — patterned and
     zeroed allocations, memset/memcpy across pointer-alignment × length
     combinations on stack and heap;
   - `SELFTEST: svg local determinism ok` — the original symptom as an
     end-to-end detector: repeated in-process plan+raster must be stable AND
     equal the host golden digest (with dec2flt and plan-digest stage checks
     that fire crumbs only on divergence).

## Consequences

- This class of bug now self-localizes: the failing detector names the
  broken layer (registers vs float vs allocator vs builtins vs pipeline
  stage) in the same boot log, instead of surfacing as downstream weirdness.
- Host-pinned constants tie every detector to the host build of the same
  code; legitimate changes fail the host test FIRST, with an update hint.
- Cost: a few ms of bringup time per boot — accepted.

## How to extend

When a new "impossible" corruption class appears, add its minimal standing
detector here (bringup phase, host-pinned expectation, loud one-line
diagnosis) as part of the fix — the detector IS the regression test.

## References

- ADR-0045/0046/0047 (the work that surfaced it)
- `source/libs/nexus-service-entry/src/lib.rs` (tripwire + the removed shim,
  documented at the `write_bytes` site)
- `source/apps/selftest-client/src/os_lite/phases/exec.rs` (detectors)
