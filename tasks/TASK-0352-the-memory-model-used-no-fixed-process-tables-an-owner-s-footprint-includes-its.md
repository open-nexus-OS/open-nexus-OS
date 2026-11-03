---
schema: 1
id: TASK-0352
title: "The memory model used: no fixed process tables, an owner's footprint includes its surfaces, limits per app"
status: in_progress
status_note: "package 2 started 2026-11-03: P0 measure, kernel stories P5-P7 first, then the service tables P1-P3, app heap P8, board P4, address space P9"
owner: ["@kernel-team", "@runtime"]
created: 2026-11-03
updated: 2026-11-03
subsystem: kernel
components: [execd, abilitymgr, memd, gpud]
kind: service
novelty: variant
estimate: 5
started: 2026-11-03
active_days: 1
dates_source: recorded
variance_note: "estimate 5d, built in 1 active day: the kernel stories reused TASK-0347's bound rule (ENOMEM never a panic) and kheap::admits; the lanes (headless, many-apps) and one board cycle proved everything the same day; left open by rule 11 until the consumer TASK-0351/P4 starts (package 3)"
goal:
  metric: "apps alive at once; apps whose surfaces are charged to them"
  baseline: "32 (execd tracked children, abilitymgr instances, memd banded processes); 8 GPU charge clients, a ninth app's surfaces go uncharged (TASK-0348 'Not doing here')"
  target: "a 33rd app launches; every app's surfaces charged; an app over its limit refused by name while the others run"
  instrument: "execd/abilitymgr launch lines · memd footprint per app (Part::Surfaces)"
  result: "QEMU many-apps: 40 instances of one app launched and drawn (0 refusals, 0 init: FAIL mint-pair create); their surfaces charged to the owner (memd: gpu charge bytes=552960000 processes=40); the 65th instance refused by name in the host test; board: the kernel's five growth markers and the 32 MiB heap probe ok"
consumer: TASK-0351/P4
depends_on: [TASK-0348]
links:
  - label: "Scope from"
    target: "TASK-0348 'Not doing here' (the old number TASK-0349 there means this task)"
  - label: Contract
    target: "RFC-0100 (memory object model v2)"
phases:
  - id: P0
    story: "every fixed table of the memory model is listed with its size (kernel, runtime, services), and the 33rd app's refusal is logged"
    state: done
    gate: "QEMU: the refusal of app 33 logged by name; the table list with sizes in the ledger body"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "build/logs/many-apps--2026-11-03T15-20-04 (refusal of app 33 by name; the kernel's endpoint table 384 bites first, at app 29: init: FAIL mint-pair create)"
  - id: P1
    story: "execd, abilitymgr and memd grow with the apps"
    state: done
    gate: "QEMU: 40 app instances launched, 0 refusals"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "build/logs/many-apps--2026-11-03T17-01-37 (40 × abilitymgr: launch (app=settings, 127 presents, 0 launch refused, 0 band dropped)"
  - id: P2
    story: "every app's surfaces count in its footprint"
    state: done
    gate: "QEMU many-apps: memd: gpu charge (client=<app:settings sid> bytes>0 processes=40) — every instance of the app shares its kernel sid, so one owner row holds all forty; the band push names the owner (memd wire v2); host test_reject_an_owner_less_band_push_and_a_charge_names_its_processes"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "build/logs/many-apps--2026-11-03T17-01-37 (memd: gpu charge client=0x349d94f73886cb27 bytes=552960000 total=676515840 processes=40)"
  - id: P3
    story: "an app past its limit is refused by name and the others keep running"
    state: done
    gate: "host test_reject_a_launch_past_the_app_s_limit_by_name_while_others_launch (65th instance STATUS_FULL, another app launches) and gpud test_reject_an_owner_past_its_budget_while_another_owner_fits; QEMU many-apps: 40 instances of one app under the 64 limit and under the owner's surface budget, 0 refusals, 0 other apps affected. Not proven in QEMU: the refusal itself (no lane reaches 65 instances or 555 MB of surfaces on 1 GiB)"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "build/logs/many-apps--2026-11-03T17-01-37 (0 launch refused, 0 past its budget, 40 windows = 552960000 bytes of surfaces)"
  - id: P4
    story: "the same on the board"
    state: done
    gate: "board: KSELFTEST kheap grows / lazy sparse / tables grow / vm window ok, SELFTEST: heap 32m ok, every app's surfaces charged with processes=1, 0 refusals, 0 panics"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "build/logs/board--2026-11-03T17-28-06 (five KSELFTEST markers ok, heap 32m ok committed_pages=8208, 8 launches, memd: gpu charge processes=1 per app)"
  - id: P5
    story: "the kernel heap grows from the frame pool instead of a fixed 8 MiB"
    state: done
    gate: "KSELFTEST: kheap grows ok (from= to=); host test_reject_a_heap_growth_past_the_pool; 40 apps with 0 ENOMEM from the heap"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "build/logs/headless--2026-11-03T16-07-48 (KHEAP: grew arena=2; KSELFTEST: kheap grows ok from=8 MiB to=16 MiB)"
  - id: P6
    story: "a lazy object's page map is sparse: reserving costs nothing until a page is touched"
    state: done
    gate: "KSELFTEST: lazy sparse ok (reserved=1 GiB heap_bytes<=64 KiB); host: a 1 GiB object reserved costs <= 64 KiB, the dense word per page is gone"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "build/logs/headless--2026-11-03T16-07-48 (KSELFTEST: lazy sparse ok reserved=512 MiB heap_reserved=2048 heap_touched=6144; ambient sparse=1 refused=1)"
  - id: P7
    story: "a process's regions and capabilities, the object table and the endpoint table grow with their use, bounded by the pool, never by a constant"
    state: done
    gate: "KSELFTEST: tables grow ok (regions=65 caps=257 endpoints=385); host test_reject_* growth past the pool answers ENOMEM/ENOSPC, never a panic; QEMU many-apps: 40 apps with no init: FAIL mint-pair create"
    started: 2026-11-03
    done: 2026-11-03
    evidence: "build/logs/many-apps--2026-11-03T17-01-37 and build/logs/headless--2026-11-03T16-58-06 (KSELFTEST: tables grow ok regions=65 caps=257 cap_slots=512 endpoints=385; 40 apps, 0 init: FAIL mint-pair create)"
  - id: P8
    story: "an app's heap grows from a lazy object and its payload is sized by its bundle, not a 4 MiB .bss and a 512 KiB cap"
    state: done
    gate: "QEMU: SELFTEST: heap 32m ok (32 MiB allocated from a 64 MiB lazy .bss heap, committed_pages within a tenth of 8192); payload object = payload_vmo::VMO_LEN (16 MiB bound) with host test_reject_a_payload_past_the_bundle_bound; 0 heap-* features left in service manifests. Not proven: a bundle above 512 KiB launching — none exists (settings fills 93% of the old cap)"
    started: 2026-11-03
    done: 2026-11-03
    evidence: build/logs/headless--2026-11-03T16-58-06
  - id: P9
    story: "a process's address space is the whole user half, sparse: a reservation past 768 MiB maps"
    state: done
    gate: "KSELFTEST: vm window ok (a 2 GiB reservation found above the old 768 MiB window's end, a page past that end mapped and translated); host: va_space finds a hole above the old window"
    started: 2026-11-03
    done: 2026-11-03
    evidence: build/logs/headless--2026-11-03T16-58-06
---

## The ask

Package 2 of the operator's three (2026-10-30): one memory model, used. No table that bounds the
number of apps or the size of a process by a constant; an owner's footprint includes its surfaces;
a limit per app, refused by name; and the kernel stories behind them — a kernel heap that grows,
sparse lazy objects, per-process region and capability tables that grow, an app heap that grows
and a payload sized by its bundle, an address space that is the whole user half.

## Measured before the cut (2026-11-03, the twelve-window lane and the code)

| Table / size | Where | Today | Refusal today |
|---|---|---|---|
| kernel heap | `neuron/src/lib.rs` `HEAP_SIZE` | 8 MiB `.bss.heap`, fixed | `ENOMEM` above `kheap::RESERVE` (ADR-0091); no telemetry line |
| lazy object page map | `mm/lazy_pages.rs` `words: Vec<u32>` | one u32 per reserved page, dense (1 GiB = 1 MiB of heap) | `EINVAL` past the pool's frames |
| regions per address space | `va_space.rs` `MAX_REGIONS` | 64 embedded (2.5 KiB per space) | the 65th region is counted, never mapped |
| capabilities per task | `cap/mod.rs` `DEFAULT_CAP_SLOTS` | 256 (4 KiB per task), `Vec` never grown | `NoSpace` on `cap_clone` |
| user VM window | `mm/mod.rs` `USER_VM_WINDOW_LEN` | 768 MiB of a 2 GiB user half | `ENOMEM` from `vm_map` |
| app heap | `nexus-service-entry` `HEAP` | static `.bss`, 384 KiB … 16 MiB by `heap-*` feature | the allocator fails, the app dies |
| app payload | app-host `PAYLOAD_MAX_LEN`, execd `PAYLOAD_VMO_BYTES` | 512 KiB fixed | a bigger bundle is a malformed header |
| app instances | abilitymgr `MAX_INSTANCES` | 32 | `STATUS_FULL` (`TooManyInstances`), unlogged |
| app processes banded | memd `bands::MAX_PROCESSES` | 32 `[Option<Entry>; 32]` | `Applied::Full`: the 33rd is never a victim |
| GPU charge clients | memd `gpu::MAX_CLIENTS` | 8 | a ninth client's surfaces go uncharged |
| execd tracked children | `children::MAX_TRACKED` | 32 exited rows (live rows never counted out) | — (already grows) |

Lane `many-windows--2026-11-03T13-04-23`: QEMU 1005 MiB (257425 frames), 221319 free at the
selftest with 12 windows; each app-host instance `rss=22441984` (22.4 MB, shared pages
included); `spaces=42`. The kernel heap's use is unmeasured — P0 adds its line.

## Measured after the cut (2026-11-03, lanes `headless--16-58-06` and `many-apps--17-01-37`)

| Table | Now | The lane's line |
|---|---|---|
| kernel heap | arenas from the pool | `KSELFTEST: kheap grows ok (from=8388608 to=16777216 arenas=2)`; `KHEAP: grew (arena=2)` on every lane |
| lazy object page map | sparse chunks | `KSELFTEST: lazy sparse ok (reserved=536870912 heap_reserved=2048 heap_touched=6144 heap_after=2048)` |
| regions / caps / endpoints | `Vec`, grown under `kheap::admits` | `KSELFTEST: tables grow ok (regions=65 caps=257 cap_slots=512 endpoints=385)`; 40 apps, 0 `init: FAIL mint-pair create` (the 384-endpoint table bit at app 29 before) |
| user VM window | the Sv39 user half | `KSELFTEST: vm window ok (len=0x3fb0000000 reserved=0x80000000 at=0x50000000 mapped=0x80001000)` |
| service heap | one 64 MiB lazy `.bss` | `SELFTEST: heap 32m ok (bytes=33554432 committed_pages=8208)` — 8192 pages touched, 16 of allocator words |
| app payload | 16 MiB lazy object | host `test_reject_a_payload_past_the_bundle_bound`; no bundle above 512 KiB exists to launch (settings fills 93%) |
| app instances / memd rows | `Vec`/`BTreeMap`, 64 per app | 40 × `abilitymgr: launch (app=settings`, 127 presents, 0 `launch refused`, 0 `band dropped` |
| surfaces in the footprint | owner on the band push (wire v2) | `memd: gpu charge (client=0x349d94f73886cb27 bytes=552960000 total=676515840 processes=40)` |
| surfaces per owner | ceiling less the compositor's eighth | 552 MB held of a 555 MB budget on 1 GiB, 0 `past its budget`; the refusal is host-tested only |

The lanes' red lines (`apphost: ux launch FAIL` × 41 — the 300 ms launch budget under TCG with
forty windows, `windowd: ux input FAIL` × 2, `init: FAIL declared slot route send slot=0x8`,
the two dsoftbus lines) are identical in count to the last green runs before this package.
The ambient-authority selftest's sparse budget scales with the pool now (`sparse_budget`): the
fixed 4 KiB per object was too tight at 1 GiB (`heap_delta=280832`).

Decisions while building: every instance of an app shares one kernel sid (`app:<bundle id>`, execd's
name), so the footprint is per app, not per window — `bands::app_sid` mirrors it in abilitymgr and
the push carries it; memd verifies the pusher is abilitymgr by the sender id, never the payload.
The per-app limit is a default (64) in `abilitymgr::lifecycle::instances_per_app`; a manifest
`max_instances` claim is a follow-up needing an RFC seed. The service heap needs no second
mechanism: `.bss` is already a lazy object (ADR-0077), so one 64 MiB heap costs the pages touched.

## Three lenses (architecture-review, 2026-11-03)

- **Scope** — `neuron/src/{lib.rs, mm/kheap.rs, mm/lazy_pages.rs, va_space.rs, cap/mod.rs, mm/mod.rs}`,
  `execd/children.rs`, `abilitymgr/lifecycle.rs`, `memd/{bands,gpu}.rs`, `nexus-service-entry`'s heap,
  app-host's payload bound. Out: swap and compressor policy, SMP, the GPU paths (package 3), the web
  engine, `nexus-loader`, pid reuse and capability-in-flight refcounts (TASK-0347 "not doing"), a
  policyd-granted large-memory manifest claim beyond the per-band default.
- **Invariant** — every growth is fallible and bounded by the frame pool or a per-process ceiling,
  answered `ENOMEM`/`ENOSPC`/`STATUS_FULL`, never a panic; reserving costs nothing, touching costs
  pages; identity stays the kernel sender id. Negative tests: `test_reject_a_heap_growth_past_the_pool`,
  `test_reject_a_cap_table_past_its_ceiling`, `test_reject_a_region_table_past_its_ceiling`,
  `test_reject_a_launch_past_the_app_s_limit_by_name_while_others_launch`,
  `test_reject_an_owner_past_its_budget_while_another_owner_fits`,
  `test_reject_an_owner_less_band_push_and_a_charge_names_its_processes`,
  `test_reject_a_payload_past_the_bundle_bound`.
- **Contract** — extends RFC-0100 (done) and ADR-0091's bound rule; no syscall change. One wire
  change: memd's band push carries the owner (memd wire `VERSION` 1 → 2, a service-internal protocol
  between abilitymgr and memd, recorded in ADR-0093's Built table, no RFC seed); ADR-0093 "tables
  grow, bounded by the pool, never by a constant" (seeded 2026-11-03); P9 amends RFC-0085's window.

## Not doing here

See the pragmatist's line above. The memd pressure order (purgeable → compress → swap → kill) is
RFC-0100's, built in layer 6; this task adds the per-app limit to it, not a new order.

## Definition of Done

Every phase done or dropped; `goal.result` with the lane's counts (apps alive, surfaces charged,
the refused app by name); the board cycle of P4; ADR-0093 Accepted; CHANGELOG and the memory docs
swept (`nx ledger stale` empty).
