# RFC-0085: Kernel-owned VA allocation — the kernel picks addresses, userspace receives them

- Status: Draft
- Owners: @kernel-mm-team @ui
- Created: 2026-07-28
- Last Updated: 2026-07-28
- Links:
  - Tasks: `tasks/TASK-0310-kernel-owned-va-allocation.md` (execution + proof)
  - ADRs: `docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md`
  - Related: `docs/rfcs/RFC-0080-shared-atlas-ro-vmo.md` (VmoRo aliases),
    `tasks/TASK-0309-gpud-framebuffer-vmo-map-ordering-hazard.md` (the
    investigation that forced this)

## Status at a Glance

- **Phase 0 (idempotency regression + doc fix)**: ✅ (2026-07-28, ladder 9/9)
- **Phase 1 (va_space pure module + host proofs)**: ✅ (13 host tests)
- **Phase 2 (unmap_leaf + embedding + window refusal)**: ⬜
- **Phase 3 (syscalls 53/54/55 + ABI + gated mm markers)**: ⬜
- **Phase 4 (gpud migration)**: ⬜
- **Phase 5 (remaining processes)**: ⬜
- **Phase 6 (legacy deletion)**: ⬜

## Scope boundaries (anti-drift)

- **This RFC owns**: the managed VA window and its invariant; the `VaSpace`
  region-table policy; `vm_map`/`vm_unmap`/`mmio_map_auto` (syscalls
  53/54/55) semantics and error model; the superpage promotion rule; the
  `AbiError` widening (`OutOfMemory`/`NoSpace`/`NotFound`/`Busy`); the
  legacy-path window refusal; the migration end-state (which wrappers die).
- **This RFC does NOT own** (named v2 seams, deliberately not built):
  VMAR hierarchy / sub-regions / rights inheritance; ASLR (determinism-first
  repo — the window base is the future knob); per-ASID TLB scoping;
  generation-tagged `AsHandle`; region splitting on unmap; page-table-page
  reclamation on unmap; PIE/relocatable ELF.

### Relationship to tasks

`tasks/TASK-0310-*` carries stop conditions and proof commands per phase.

## Context

Userspace invents virtual addresses: gpud hand-carves ~450 MiB across two
never-freeing bump allocators; six components copy the same MMIO window
constant `0x2000_e000`; hidrawd keeps nine windows in fixed arrays;
virtio-blk's VAs are module-global (two devices in one process would
collide). There is **no unmap anywhere in the kernel** — the page-table
module header claims one; the code never had it. The 46.9 MiB framebuffer is
mapped by a 12,000-iteration per-page syscall loop, in which a
timing-dependent EINVAL lived that evaded three instrumented hunts and
disappeared (0/8 runs) the moment tracing shifted the timing — a Heisenbug
that evades hunting but cannot evade an architecture with no window for it.

The model is the one this repo already claims as its lineage (seL4/Fuchsia):
Fuchsia's VMAR and Mach's `VM_FLAGS_ANYWHERE` — the kernel chooses the
address, records the region, returns the VA.

## Goals

- One syscall maps a whole range; the kernel picks and returns the VA.
- A real `vm_unmap`; released VA is reused (first-fit).
- Per-address-space region accounting: overlaps refused BEFORE the page
  table, with names; diagnostics can name the occupant by kind.
- 2 MiB superpage promotion, invisible in the ABI.
- Magic VA constants deleted tree-wide.

## Non-Goals

The v2 seams listed under scope boundaries.

## Constraints / invariants (hard requirements)

- **Invariant**: the managed window `[0x5000_0000, 0x8000_0000)` contains
  only kernel-chosen, region-tracked mappings; regions are pairwise
  disjoint, va-sorted; fixed-VA maps into the window are refused (EPERM).
  This is what keeps the hole-finder sound without consulting the page table.
- **Determinism**: allocation is first-fit over a sorted table — a golden
  op-sequence host test pins exact addresses byte-for-byte. Failures leave
  allocator state bit-identical.
- **Bounded**: `MAX_REGIONS = 64` fixed capacity (2.5 KiB per AS, no heap
  churn), O(64) operations, at most one TLB shootdown per unmap. Overflow is
  asymmetric: a `vm_map` that cannot be RECORDED is REFUSED (ENOSPC, no fake
  success); a kernel-placed Fixed record that does not fit is COUNTED
  (`untracked`), never silently forgotten — refusing an exec over
  bookkeeping would invert priorities, and Fixed records never feed the
  hole-finder.
- **No fake success**: never map without a record, never record without a
  map; a mid-plan failure rolls back already-written leaves.
- **Security floor**: caller's own AS only (no handle argument — no confused
  deputy, no new exposure of the untagged `AsHandle`). W^X enforced;
  EXECUTE via `vm_map` denied outright in v1 (no consumer). MMIO stays
  USER|RW, never EXEC. `VmoRo` maps force-read-only via `vmo_ro`.
- **Stubs policy**: none. Phases land whole or not at all.

## Proposed design

### Contract / interface (normative)

**Window** (in `mm/mod.rs`, beside the arena SSOT):
`USER_VM_WINDOW = [0x5000_0000, 0x8000_0000)` — 768 MiB. Above every
inventoried fixed use (gpud's carve-outs end at 0x4400_0000; a guard band
0x4400_0000–0x5000_0000 stays reserved), below `USER_VADDR_LIMIT`
0x8000_0000 so every returned VA is sign-positive and can never collide
with a negative errno in the one-usize return channel.

**Policy** (`src/va_space.rs`, pure, un-gated, host-proven — the
`image_allocs` family): `VaSpace { regions: [VaRegion; 64], len, untracked,
peak, window_base, window_len }`, `RegionKind { Vmo, Mmio, Fixed }`,
`find_hole(len, align, phase)` (lowest fit, phase-aware), `insert`,
`remove_exact` (whole-region only), `record_fixed` (coalescing),
`any_backed_by` (destroy guard), `admits_fixed` (window refusal),
`superpage_plan(va, pa, len)` (head/2M-interior/tail tiling; promotes only
when `va ≡ pa (mod 2 MiB)`).

**Syscalls**:

```
SYSCALL_VM_MAP        = 53   a0=vmo_slot a1=offset a2=len a3=flags → va
SYSCALL_VM_UNMAP      = 54   a0=va a1=len → 0
SYSCALL_MMIO_MAP_AUTO = 55   a0=cap_slot a1=offset a2=len → va
```

- `vm_map`: derive cap with `Rights::MAP` (`Vmo`/`VmoRo`); validate offset
  page-aligned, `offset+len ≤ cap.len`, flags ⊆ {R, RW}; `pa = base+offset`
  (VMOs are physically contiguous — load-bearing precondition);
  `find_hole(len, 2 MiB, pa % 2 MiB)` for ≥2 MiB else 4 KiB; execute the
  superpage plan (`map_2m` interior, `map` head/tail); rollback on mid-plan
  failure; record; return va. No shootdown on map (invalid→valid is never
  cached).
- `vm_unmap`: exact whole recorded `Vmo`/`Mmio` region; `Fixed` ⇒ EPERM.
  Clear leaves via new `PageTable::unmap_leaf` (4K + 2M,
  `MapError::NotMapped` for absent), local sfence + ONE
  `smp::tlb::shootdown_all()` per call, region removed only after the
  shootdown returns.
- `mmio_map_auto`: DeviceMmio cap, whole len in one call, same security
  floor as `sys_mmio_map`. Idempotency by deletion: a caller that never
  chooses an address cannot collide; deliberately NO dedupe (no hidden
  kernel state).
- Legacy `SYSCALL_MAP` (4) stays for the ELF loader (fixed VAs are a linker
  contract — chosen by the kernel's toolchain at link time; PIE is the
  future knob), becomes region-tracked (`record_fixed`) and REFUSES the
  managed window. `SYSCALL_MMIO_MAP` (27) is deleted in Phase 6; the number
  is retired, never reused (the 44-collision scar is the warning).
- exec records ELF segments, stack and meta/info pages as `Fixed` regions —
  the table describes the whole address space.

**Error model** (ADR-0054 continuation — no wildcard arms):

| Failure | errno | AbiError |
|---|---|---|
| window exhausted | ENOMEM 12 | `OutOfMemory` (new) |
| region table full | ENOSPC 28 | `NoSpace` (new) |
| unmap: nothing at va | ENOENT 2 | `NotFound` (new) |
| vmo_destroy with live regions | EBUSY 16 | `Busy` (new) |
| fixed map into window | EPERM 1 | `PermissionDenied` |
| bad flags/len/alignment | EINVAL 22 | `InvalidArgument` |
| overlap (fixed path only) | EEXIST 17 | `AlreadyExists` |

The four new `AbiError` variants dissolve the pre-existing 12/28 →
`SpawnFailed` decode collapse; spawn wrappers translate locally to keep
their API, and exhaustive matches surface every other consumer at compile
time. New failure paths log occupant-style
(`VM-MAP-FAIL reason=window-exhausted free_max=… want=…`), error path only.

**`vmo_destroy`**: existing sole-owner gate + `any_backed_by` over the
caller's regions ⇒ EBUSY while a `vm_map` of the range is live. Legacy
fixed-VA mappings remain "caller's contract" until migration completes.

**Process exit**: nothing new — `VaSpace` is embedded in `AddressSpace` and
dies with it; ASID quarantine already covers TLB staleness.

**nexus-abi**: one wrapper per syscall (`vm_map`, `vm_unmap`,
`mmio_map_auto`), `AbiError`-faithful. End-state: `vmo_map_page` (lossy) and
`vmo_map` deleted; `vmo_map_page_sys` and `mmio_map` deleted after their
last callers migrate.

### Phases / milestones

See the task ledger. Order: idempotency regression (done) → pure module
(done) → kernel plumbing → syscalls + ABI + first GATED mm markers → gpud →
hidrawd/atlas/the-six/virtio-blk → legacy deletion. Superpage promotion may
slide from Phase 3 to 4 without ABI change: the phase-aware hole-finder
ships first, so VAs do not move when promotion lands.

## Security considerations

- **Threat model**: a service mapping over another's memory (impossible —
  own-AS only), confused deputy via AS handles (no handle argument), W^X
  violations (three existing layers + EXECUTE denied), stale mappings onto
  recycled arena pages after `vmo_destroy` (closed by the EBUSY guard),
  hidden kernel state (mmio_map_auto deliberately does not dedupe).
- **Mitigations**: the window invariant with its negative tests; bounded
  table with refuse-don't-trust re-validation in `insert`.
- **Open risks**: shootdown frequency under the BKL at gpud churn (tripwire:
  `KSELFTEST: bkl budget ok`; fallback: phase vm_unmap like VMO_CREATE).

## Failure model (normative)

The errno table above; every refusal named, logged with numbers on the
error path, and state-preserving. No silent fallback anywhere: a mapping
the kernel cannot account for is never created.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cargo test -p neuron va_space   # 13 proofs incl. golden sequence, reuse,
                                # exhaustive superpage phases, bounded fuzz
```

### Proof (OS/QEMU)

```bash
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=240s just test-os
```

### Deterministic markers

The first GATED mm markers in the repo (mm was QEMU-exercised, never
QEMU-proven): `KSELFTEST: vm map ok` (va in window, translate agrees, L1
leaf verified when 2M-eligible), `KSELFTEST: vm unmap ok` (translate None,
remap returns the SAME va), `KSELFTEST: vm map reject ok` (fixed-into-window
EPERM, unknown-unmap ENOENT) — plus promotion of the existing ungated
`as map ok` / `vmo zero ok` / `w^x enforced`. One commit per the
marker-mirror rule (script + manifest must agree).

## Alternatives considered

- **Per-process userspace VA allocator** — rejected: the kernel cannot trust
  or verify it, and the region table is needed for unmap anyway.
- **Full VMAR hierarchy now** — rejected: no consumer, capacity cost; seams
  named.
- **Handle-parameterized target AS** — rejected: confused deputy + stale
  untagged-handle exposure.
- **Deduping idempotent mmio_map_auto** — rejected: hidden state; the
  probe-scan callers are correct with two aliases.
- **Raising gpud's stride again** (the 16→32 MiB precedent) — rejected:
  re-arms the same trap on the next growth; TASK-0309's stopgap already
  proved the class needs an allocator, not a bigger constant.

## Open questions

- execd `std_server`'s second stack convention (0x4000_0000) vs exec.rs's
  0x2000_0000 — reconcile or record as Fixed (Phase 2 decision).
- Whether `SYSCALL_MAP` retires post-migration (Phase 6 audit).

---

## Implementation Checklist

- [x] **Phase 0**: seven `AlreadyExists` idempotency fixes + syscall-list
      doc — proof: ladder 9/9 (2026-07-28)
- [x] **Phase 1**: `va_space.rs` + 13 host proofs — proof:
      `cargo test -p neuron va_space`
- [ ] **Phase 2**: `unmap_leaf`, embedding, window refusal, exec Fixed
      records
- [ ] **Phase 3**: syscalls 53/54/55, ABI widening, first gated mm markers,
      `bkl budget ok` stays green
- [ ] **Phase 4**: gpud migration; `VaWindow` and both slot arenas deleted;
      FB map time collapses
- [ ] **Phase 5**: hidrawd, atlas, the 0x2000_e000 six, virtio-blk
- [ ] **Phase 6**: legacy wrappers + `sys_mmio_map` deleted; grep proves
      zero legacy callers
- [ ] Security-relevant negative tests exist (`test_reject_*`: fixed-into-
      window, table-full, partial-unmap, EXEC flags)
