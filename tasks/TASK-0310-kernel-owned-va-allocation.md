---
title: TASK-0310 Kernel-owned VA allocation — vm_map/vm_unmap/mmio_map_auto end to end
status: In progress (2026-07-28) — Phases 0–2 done
owner: @kernel-mm-team @ui
created: 2026-07-28
links:
  - RFC: docs/rfcs/RFC-0085-kernel-owned-va-allocation.md
  - ADR: docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md
  - Forced by: tasks/TASK-0309-gpud-framebuffer-vmo-map-ordering-hazard.md
  - Pauses: tasks/TASK-0308-dsl-slots-and-window-scaffold.md (Phasen 7/8)
---

## Context

TASK-0309 exposed the class: userspace invents VAs (gpud ~450 MiB across two
never-freeing bump arenas; six copies of `0x2000_e000`; hidrawd's nine fixed
windows; virtio-blk module-global), the kernel has NO unmap and NO region
record, and the framebuffer maps via a 12 000-syscall loop in which a
timing-dependent EINVAL lived that instrumentation made vanish (0/8 after,
~50 % before — a Heisenbug). RFC-0085 is the architecture that removes the
habitat: the kernel picks, records and returns addresses.

## Phases (stop conditions + proof)

### Phase 0 — idempotency regression + doc fix ✅ (2026-07-28)

The seven `mmio_map` callers that treated `InvalidArgument` as already-mapped
now match `AlreadyExists` (ADR-0054 moved the meaning): goldfish-rtc,
virtio-rng ×2, virtio-input, virtio-blk, nexus-net-os, smoltcp_probe,
nexus-init. `InvalidArgument` no longer doubles as a success — it was masking
genuinely bad arguments.

`docs/architecture/01-neuron-kernel.md`'s syscall list was stale at 28; now
complete through 52 with the ADR-0054 errno notes.

Ratchet fallout, four honest splits: `syscall/api/vm_map.rs` (map surface out
of vmo.rs, 705→524 — Phase 3 lands its handlers exactly there),
`core/trap/errno.rs` (the errno tables out of handler.rs),
`mm/page_table_tests.rs` (with an HONESTY note: parent `mod mm` is
target-gated, so these are documentation-grade — the live host oracle is
`va_space`), `bootstrap/labels.rs` (init's error-name tables).

**Proof: ladder 9/9, `SELFTEST: Completed`** (build/logs/headless--2026-07-28T10-37-57).

### Phase 1 — pure module + host proofs ✅ (2026-07-28)

`source/kernel/neuron/src/va_space.rs` (376 LOC, un-gated, `pub` until the
in-kernel consumers land) + `va_space_tests.rs` (13 proofs):

golden sequence pins exact addresses · first-fit reuse (the anti-virgl-arena
test) · phase-aligned holes · superpage plan tiles exactly for EVERY page
phase of a 2 MiB frame (512 cases) + refuses incongruent ranges · exhaustion
leaves state bit-identical · 65th region refused without phantom · 2000-step
bounded LCG fuzz with the invariant walked after every op · unmap
discrimination (NotFound/LenMismatch/double) · fixed-into-window refusal ·
fixed coalescing + counted overflow · destroy guard · edge inputs.

**Proof: `cargo test -p neuron va_space` → 13/13.**

### Phase 2 — kernel plumbing ✅ (2026-07-28)

`PageTable::unmap_leaf` (4K leaf + 2M leaf, mid-superpage → `Unaligned`,
absent → new `MapError::NotMapped` → ENOENT); `map_2m` already
`pub(crate)` — reachable for Phase 3's superpage path. `VaSpace` embedded
in `AddressSpace` (constructed with `USER_VM_WINDOW_BASE/LEN` from
`mm/mod.rs`) behind `AddressSpaceManager::map_page_tracked`: refuses
fixed-VA maps into the window (`VA-FIXED-REFUSED` log + Overlap→EEXIST),
then maps, then `record_fixed` (coalescing). All 8 fixed-VA call sites
switched (exec.rs ×5, vmo.rs `sys_as_map`, vm_map.rs ×2).

**Stack conventions decided**: kernel exec's 0x2000_0000 and execd's child
convention 0x4000_0000 both stay — both lie below the window base
0x5000_0000 and are recorded as `Fixed` at map time; the window invariant
is untouched.

Ratchet fallout, two honest splits: `mm/kernel_layout.rs` (boot-time
kernel-segment + identity mapping out of address_space.rs, 968→674) and
`mm/page_table_verify.rs` (the `debug_assertions` invariant walker,
page_table.rs 689→643).

**Found & fixed en route — torn UART markers**: the ladder went red with
`rngd: ready` deterministically losing its tail mid-line (`rngd[INFO as]
AS: trampoline enter…` — 4 bytes out, 8 bytes GONE). Root cause is the
documented byte-level corruption of the lock-free `sys_debug_putc` path;
13 services carried the same copy-pasted per-byte `emit_line` fallback.
All 13 now delegate to `nexus_abi::debug_println` (one atomic
`debug_write`, folding moves inside so markers are not double-tallied).
P2 only shifted boot timing onto the landmine; any change could re-arm it.

**Proof: `cargo test -p neuron` 42/42 · `just check` green · ladder 9/9
`[PASS] chain-marker contract` (headless--2026-07-28T12-58-29).** One
intermediate run failed on the known pre-existing `ime v2 candidates`
flake (unrelated, tracked separately).

### Phase 3 — syscalls + ABI + first gated mm markers ⬜

53/54/55 in `api/vm_map.rs`; nexus-abi wrappers; `AbiError` +
`OutOfMemory/NoSpace/NotFound/Busy`; `vmo_destroy` EBUSY guard;
`KSELFTEST: vm map ok / vm unmap ok / vm map reject ok` + promotion of
`as map ok`/`vmo zero ok`/`w^x enforced` — markers_generated + both
`expected_sequence` blocks + proof manifest in ONE commit; selftest-client
probe. Gate: new markers green AND `bkl budget ok` stays green.

### Phase 4 — gpud ⬜

FB loop → one `vm_map`; delete `VaWindow` + 0x2040_0000 slots; virgl arena →
vm_map/vm_unmap (exhaustion cliff gone); `release_resource` really unmaps;
queues/MMIO. Gate: gpud phases green, FB map time collapses, budget green.

### Phase 5 — the rest ⬜

hidrawd (9 windows), app-host atlas, the 0x2000_e000 six, virtio-blk
per-device. Gate: headless + dhcp profiles.

### Phase 6 — deletion ⬜

`vmo_map_page`/`vmo_map_page_sys`/`mmio_map` wrappers + `sys_mmio_map`
handler deleted (27 retired, never reused); docs/CHANGELOG; grep proves zero
legacy callers. `SYSCALL_MAP` retirement audit.

## Honest notes

- The TASK-0309 intermittent is NOT proven dead by this work — its habitat
  (the per-page loop) is removed, and the MAP-FAIL stage trace stays armed
  until Phase 6 deletes the legacy path.
- `mm`-gated kernel tests still do not compile on host (Context::new arity
  drift) — out of scope here; the live host oracle is `va_space`.
