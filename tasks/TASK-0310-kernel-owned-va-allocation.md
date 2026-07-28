---
title: TASK-0310 Kernel-owned VA allocation — vm_map/vm_unmap/mmio_map_auto end to end
status: In progress (2026-07-28) — Phases 0–5 done
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

### Phase 3 — syscalls + ABI + first gated mm markers ✅ (2026-07-28)

Syscalls 53/54/55 live: handlers in `api/vm_map.rs`, executor in NEW
`mm/vm_ops.rs` (RECORD-THEN-MAP with mid-plan rollback — never a
half-mapped zombie; CLEAR-SHOOTDOWN-FORGET — one `shootdown_all` per
unmap, region forgotten only after; `superpage_plan` drives `map_2m` for
interiors). `Error::Va(VaError)` + `Error::ResourceBusy` with an
exhaustive `va_errno` table (ADR-0054). `vmo_destroy` refuses EBUSY while
ANY address space maps the range (`vm_ops::any_space_maps`); legacy Fixed
records keep the old caller contract. `va_space` gained `peek_exact`
(remove_exact now peeks first).

nexus-abi: `vm_map`/`vm_unmap`/`mmio_map_auto` wrappers; `AbiError` +
`OutOfMemory(12)/NoSpace(28)/NotFound(2)/Busy(16)` — the historic
12/28→`SpawnFailed` collapse is gone; the `spawn` wrapper translates
locally so init's `spawn_last_error` diagnosis keeps firing. All three
exhaustive label tables extended (gpud diag, selftest-client, init).

**First gated mm markers ever**: `KSELFTEST: vm map ok` (va in window,
translate matches, interior 2 MiB leaf PROVEN via `leaf_span_at`),
`vm unmap ok` (translate gone + same-va re-map = first-fit reuse),
`vm map reject ok` (fixed-into-window EEXIST + unknown-unmap ENOENT +
destroy-while-mapped EBUSY) — emitted by `selftest/vm_alloc.rs` (split;
also deduped the two verbatim W^X blocks, selftest/mod.rs 2487→2422).
Promoted `vmo zero ok`/`as map ok`/`w^x enforced` into the gates.
Userspace e2e: `SELFTEST: vm map roundtrip ok` (map→read seed→write→
vmo_read verify→EBUSY→unmap→SAME-va re-map→destroy) in the mmio phase.
Manifest + both `expected_sequence` blocks + probe in this one package.

**Proof: ladder green with the new gates on the FIRST run
(headless--2026-07-28T13-39-02), `bkl budget ok (max_wait=0us)` — the
per-unmap shootdown is invisible at selftest load. `cargo test -p neuron`
42/42 · `just check` green.**

### Phase 4 — gpud ✅ (2026-07-28)

Every VA in gpud is kernel-chosen now. Framebuffer attach: the ~12 000-call
per-page loop is ONE `vm_map` (TASK-0309's Heisenbug habitat deleted).
DELETED: `VaWindow` + `alloc_resource_va_index` + `MAX_RESOURCE_VA_SLOTS` +
the whole fixed constant map (`GPU_QUEUE/CMD/RESP/CURSOR_*` 0x2030-region,
`GPU_RESOURCE_BASE_VA/STRIDE` 0x2040-region, `GPU_VIRGL_BACKING_*`
0x3800-region — the 12-slot arena whose exhaustion was a silent GL→2D
fallback), plus `next_resource_va_index`/`virgl_backing_count`.
`CtrlQueue::new` maps its three pools whole-range; `mmio_map_auto` replaces
the fixed 0x2020_0000 MMIO window; `create_resource_os` and both virgl
backing/scratch paths are one-call maps. `release_resource` REALLY unmaps
(`ResourceRecord.backing_map_len` carries the exact region; unmap ordered
before `vmo_destroy` — the new EBUSY guard enforces it).

**Found & fixed en route — the `ime v2 candidates` "flake" was a real
last-writer-loses bug**: imed's OSK path switches the engine and persists
`input.keymap`; the settings spine (settingsd → inputd → imed main endpoint)
relays asynchronously, and a STALE in-flight relay of the previous tag could
clobber a fresh switch mid-typing (uart: `inputd: keymap set zh` landing
AFTER the FAIL). Kernel-timing shifts turned it from rare into 3-of-4 red.
Fix in the host-testable core: `note_persisted`/`relay_layout` — stale
relays are ignored until the own write's echo returns, with a bounded
ignore budget (4) so an external settings change can never be starved.
2 new host proofs (imed 18/18).

**Proof: ladder exit 0 TWICE in a row (headless--2026-07-28T15-11-35,
…T15-13-13) incl. `ime v2 candidates ok` and all RFC-0085 gates ·
`gpud: timing handoff_to_ready_ms=0` vs 63–69 ms before P4 — the FB map
cost measurably collapsed · `bkl budget ok (max_wait=0us)` · `just check`
green.**

### Phase 5 — the rest ✅ (2026-07-28)

Userspace no longer invents ANY virtual address:

- **hidrawd / virtio-input**: `MappedVirtioInputDevice::open(cap, slot)` —
  the nine fixed windows (3× MMIO/queue/buffer arrays) are gone; a failed
  probe UNMAPS before returning so the service-loop retry cannot leak a
  region per round.
- **virtio-rng / rngd**: whole-window `mmio_map_auto` + queue/buffer
  `vm_map`, once-latched (runs per entropy request).
- **goldfish-rtc / timed**: kernel-chosen window, cached across the
  Implausible-retry loop.
- **virtio-blk / statefsd + virtioblkd**: per-device kernel-chosen vas —
  the module-global consts (and their cross-driver "avoid 0x2000_e000"
  comment) deleted; the retry loop maps once.
- **nexus-net-os / netstackd**: mmio + queue + buffer one-call maps.
  FOUND EN ROUTE: `poll_inner_once` hardcoded `mmio_va: 0x2000_e000` in its
  `SmolDevice` — the first boot page-faulted at 0x2000_e060 (USER-PF, the
  guard working as designed: a stale fixed VA is now a loud fault, not a
  silent alias). `Inner` carries the mapped va.
- **app-host atlas**: the ~1100-call per-page RO map at fixed 0x3000_0000
  is one `vm_map` on the `VmoRo` alias (kernel force-maps RO).
- **nexus-init probe**: ONE whole-window map of all 8 virtio-mmio slots —
  the per-slot maps at the shared 0x2000_e000 va and the AlreadyExists
  dance are gone.
- **selftest-client**: fw_cfg window kernel-chosen (`AtomicUsize` base);
  smoltcp opt-in probe migrated; the RETIRED `mmio_map_probe` deleted.
- **userspace/memory**: dead `map_ro_pages` (caller-chosen va, zero
  callers) deleted.

The `0x2000_e000` literal survives nowhere. Every remaining
`vmo_map_page*`/`mmio_map` reference in the tree is nexus-abi itself or a
marker/comment — Phase 6 deletes the wrappers.

**Proof: headless ladder exit 0 (…T15-35-26, incl. `ime v2 candidates ok`,
`APPHOST: atlas mapped`, rngd/rtc/blk/net proofs) · dhcp profile exit 0
(dhcp bound + dns ok) · `just check` green · test-host 615/615. hidrawd's
new open path is device-exercised only in the interactive lane (headless
has no virtio-input) — `just start` sanity remains on the user.**

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
