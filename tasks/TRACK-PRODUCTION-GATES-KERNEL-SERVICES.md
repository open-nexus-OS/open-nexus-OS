---
title: TRACK Production Gates (kernel + core services): release closure plan for kiosk/IoT, consumer, and hardware beta
status: Draft
owner: @runtime @security @storage @ui
created: 2026-04-13
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Status board / group inventory: tasks/STATUS-BOARD.md
  - Keystone closure plan: tasks/TRACK-KEYSTONE-GATES.md
  - Zero-Copy App Platform: tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Networking drivers roadmap: tasks/TRACK-NETWORKING-DRIVERS.md
  - Creative apps gate consumer: tasks/TRACK-CREATIVE-APPS.md
---

## Purpose

This track turns the grouped `TASK-*` inventory into an explicit **release-gate contract** for the
kernel and core services.

`tasks/STATUS-BOARD.md` stays the dashboard. This track defines what each group must prove before we
call the system:

- **hardware bringup beta**,
- **secure kiosk / IoT releasable**,
- or **consumer-floor releasable**.

Tasks remain the execution truth. This track must not become hidden DoD.

## Gate tier taxonomy

- **`production-grade`**
  Release-critical. Security, recovery, boundedness, and proof quality must be strong enough that
  failure here blocks kiosk/IoT and consumer claims.
- **`production-floor`**
  Real, coherent, and test-backed. May still need optimization or breadth work, but must already be
  safe, deterministic, and honest enough for real product integration.
- **`beta-floor`**
  Real behavior with deterministic proofs and no fake success. Good enough for bringup/beta
  programs, not strong enough yet for consumer closeout.

## Release profiles

### Hardware bringup beta

Minimum meaning:

- kernel boots deterministically on QEMU `virt`,
- core device-class services can come up without authority drift,
- faults are diagnosable,
- and unfinished areas remain explicitly labeled as beta/stub/degraded.

### Secure kiosk / IoT release

Minimum meaning:

- policy, identity, persistence, update/recovery, and kernel authority boundaries are release-grade,
- networking/distributed surfaces are bounded and deny-by-default,
- no subsystem claims success without real proof.

### Consumer-floor release

Minimum meaning:

- kiosk/IoT floor is already satisfied,
- UI-critical kernel/service paths avoid avoidable copy storms and wakeup churn,
- present/input/session/storage/update flows are stable enough to support smooth daily use.

## Group gate matrix

| Group | Tier | Release target | Primary closure direction |
|------|------|----------------|---------------------------|
| Kernel Core & Runtime | `production-grade` | consumer + kiosk/IoT | kernel runtime closure, IPC/capability correctness, zero-copy and OOM/resource truth |
| Security, Policy & Identity | `production-grade` | consumer + kiosk/IoT | deny-by-default policy, identity custody, quotas/egress, audit + reject proofs |
| Storage, PackageFS & Content | `production-grade` | consumer + kiosk/IoT | durable `/state`, package/content integrity, quota/error semantics, bounded write-paths |
| Updates, Packaging & Recovery | `production-grade` | consumer + kiosk/IoT | canonical package/update contracts, rollback-safe recovery, signed/provisioned bringup |
| DSoftBus & Distributed | `production-floor` | consumer + distributed beta | preserve legacy closure and extend without regressing bounded auth/routing guarantees |
| Networking & Transport | `production-floor` | consumer + kiosk/IoT | netstack hardening, ingress bounds, real-connect closure, virtio-net service proofs |
| Observability, Crash, Perf & Diagnostics | `production-floor` | consumer + hardware beta | crash evidence, perf budgets, soak/flake discipline, offline diagnostics |
| Accounts, Ability & Sessions | `production-floor` | consumer | session and lifecycle authority, foreground/background policy, home isolation |
| Windowing, UI & Graphics | `production-floor` | consumer | present/input/perf floor, zero-copy surface discipline, first-frame to no-trail/no-mosaic |
| DevX, Config & Tooling | `production-floor` | internal + hardware beta | single CLI, config/schema discipline, deterministic harness hygiene |
| Bringup, Hardware & Drivers | `beta-floor` | hardware bringup beta | device-class services, RISC-V `virt` completion, DriverKit bringup proofs |
| Text, IME, I18N & Accessibility | `beta-floor` | consumer beta | deterministic text/IME/locale/a11y spine |
| Media & Creative | `beta-floor` | consumer beta | audiod/media-session floor, bounded decode/capture paths |
| Messaging, Search, Store & Sharing | `beta-floor` | consumer beta | real user-facing service surfaces without drift or hidden background work |
| DSL, App Platform & SDK | `beta-floor` | ecosystem beta | DSL/SDK contracts and app-platform proofs with no hidden kernel asks |

## New kernel production-grade gap tasks

These tasks are the explicit "not yet production-grade" gap closure items for the kernel/core spine:

- `TASK-0286` — real kernel memory accounting (`RSS` / pressure snapshots)
- `TASK-0287` — kernel memory-pressure enforcement + canonical OOM handoff
- `TASK-0288` — runtime/SMP/timer/IPI closure under deterministic stress
- `TASK-0289` — boot trust floor (verified boot anchors + anti-rollback + measured boot handoff)
- `TASK-0290` — kernel zero-copy closure (VMO seal rights + write-map denial + reuse truth)

## Production-grade closure groups

### Gate A — Kernel Core & Runtime

Representative task spine:

- `TASK-0010`, `TASK-0012`, `TASK-0012B`, `TASK-0013`, `TASK-0042`
- `TASK-0054B`, `TASK-0054C`, `TASK-0054D`
- `TASK-0188`, `TASK-0228`, `TASK-0245`, `TASK-0247`
- `TASK-0267`, `TASK-0269`, `TASK-0277`
- `TASK-0281`, `TASK-0282`, `TASK-0283`
- `TASK-0286`, `TASK-0287`, `TASK-0288`, `TASK-0290`

Closure definition:

- SMP/runtime behavior is deterministic under QEMU `virt` with bounded retries and no inbox-drain
  folklore.
- IPC channel, reply, and capability-transfer behavior is proven under load and does not depend on
  fake-success markers.
- UI-critical hot paths have a real low-copy discipline:
  short control messages stay small, bulk payloads follow VMO/filebuffer rules, repeated-use paths
  prefer reuse over churn.
- Resource truth exists: spawn/resource failures, OOM pressure, and backpressure are surfaced with
  stable labels and bounded behavior.
- Kernel type/ownership hardening reduces whole classes of capability/per-CPU misuse before runtime.

### Gate B — Security, Policy & Identity

Representative task spine:

- `TASK-0008`, `TASK-0008B`, `TASK-0019`, `TASK-0028`, `TASK-0043`
- `TASK-0136`, `TASK-0160`, `TASK-0167`, `TASK-0168`
- `TASK-0189`

Closure definition:

- Sensitive operations are deny-by-default and routed through canonical policy authority.
- Identity custody is explicit; test/backfill keys remain visibly non-production.
- Reject paths are first-class: invalid syscall/profile/quota/egress cases produce deterministic
  denials and `test_reject_*` style coverage where relevant.
- Audit trails exist for security decisions without leaking secrets.

### Gate C — Storage, PackageFS & Content

Representative task spine:

- `TASK-0009`, `TASK-0025`, `TASK-0031`, `TASK-0032`, `TASK-0033`
- `TASK-0132`, `TASK-0133`, `TASK-0134`
- `TASK-0232`, `TASK-0233`, `TASK-0264`, `TASK-0265`, `TASK-0284`

Closure definition:

- `/state` is durable and reboot-stable.
- Error semantics and quota enforcement are deterministic and visible to callers.
- Package/content read and write paths stay bounded and integrity-aware.
- Zero-copy claims are honest: bulk transfer contracts exist where payload size justifies them, and
  fallbacks are measurable instead of hand-waved.

### Gate D — Updates, Packaging & Recovery

Representative task spine:

- `TASK-0007`, `TASK-0029`, `TASK-0034`, `TASK-0035`, `TASK-0036`, `TASK-0037`
- `TASK-0050`, `TASK-0051`
- `TASK-0129`, `TASK-0130`
- `TASK-0197`, `TASK-0198`, `TASK-0238`, `TASK-0239`
- `TASK-0260`, `TASK-0261`, `TASK-0289`

Closure definition:

- Canonical package/bundle format is the only referenced release contract.
- Update and rollback state machines are durable, auditable, and recoverable.
- Recovery tooling is signed/policy-gated where required and does not invent side channels around
  the normal authority model.
- Provisioning/reflash flows are deterministic enough to support bringup and field recovery.

## Production-floor closure groups

### Gate E — Windowing, UI & Graphics service floor

Representative task spine:

- `TASK-0054`..`TASK-0056C`
- `TASK-0169`, `TASK-0170`
- `TASK-0207`, `TASK-0208`
- `TASK-0250`, `TASK-0251`, `TASK-0253`

Closure definition:

- first-frame, present, and input paths work end-to-end with deterministic markers,
- surface ownership/reuse rules are clear enough to avoid avoidable copies and window-trail style
  artifacts caused by kernel/core-service churn,
- perf claims are backed by budgets or measured scenes, not narrative optimism.

### Gate F — DSoftBus & Distributed

Representative task spine:

- `TASK-0003`..`TASK-0005`
- `TASK-0015`..`TASK-0017`
- `TASK-0020`..`TASK-0024`
- `TASK-0030`, `TASK-0157`, `TASK-0158`
- `TASK-0195`, `TASK-0196`, `TASK-0211`, `TASK-0212`
- `TASK-0219`, `TASK-0220`

Closure definition:

- legacy `TASK-0001`..`TASK-0020` closure remains green,
- `TASK-0021` is now closed as done in `RFC-0035` (host-first QUIC scaffold with fail-closed strict mode),
- `TASK-0021` closure preserves OS QUIC disabled-by-default with deterministic fallback markers and bounded selection/perf proofs, including host runtime selection wiring (`DSOFTBUS_TRANSPORT=tcp|quic|auto`), real host QUIC transport assertions (`quic_host_transport_contract` with explicit QUIC+mux smoke payload), selection/perf suites (`quic_selection_contract`), and `REQUIRE_DSOFTBUS=1` single-VM marker gate,
- dependency harmonization after `TASK-0021` keeps strict deny policy (`multiple-versions = "deny"`): `thiserror`/`snow` convergence is complete, `rand_core` split is removed, and only compatibility-constrained `getrandom`/`windows-sys` lines are narrowly skipped in `config/deny.toml`,
- auth/discovery/session/routing follow-ons preserve bounded identity enforcement,
- new media/busdir/QUIC work does not regress the earlier deterministic host/OS/2-VM story.

### Gate G — Networking & Transport

Representative task spine:

- `TASK-0016B`, `TASK-0052`, `TASK-0177`
- `TASK-0193`, `TASK-0194`
- `TASK-0248`, `TASK-0249`

Closure definition:

- local networking and ingress policy behave as one coherent stack,
- real-connect and virtio-net proofs exist without ambient trust shortcuts,
- failure modes are diagnosable offline.

### Gate H — Observability, Crash, Perf & Diagnostics

Representative task spine:

- `TASK-0006`, `TASK-0014`, `TASK-0018`
- `TASK-0143`, `TASK-0144`, `TASK-0145`
- `TASK-0172`, `TASK-0173`
- `TASK-0227`, `TASK-0242`, `TASK-0243`

Closure definition:

- crash and bugreport flows produce real evidence,
- perf gates are deterministic and tied to scenes or bounded workloads,
- soak/flake handling detects drift instead of normalizing it.

### Gate I — Accounts, Ability & Sessions

Representative task spine:

- `TASK-0109`, `TASK-0110`
- `TASK-0126B`
- `TASK-0223`, `TASK-0224`
- `TASK-0235`

Closure definition:

- session ownership is explicit,
- greeter/home isolation is real,
- foreground/background and kill semantics do not fork into parallel policy systems.

### Gate J — DevX, Config & Tooling

Representative task spine:

- `TASK-0045`, `TASK-0046`
- `TASK-0262`, `TASK-0266`, `TASK-0268`, `TASK-0273`, `TASK-0285`

Closure definition:

- one authoritative CLI path exists for diagnostics,
- config/schema surfaces do not drift per subsystem,
- naming/harness/tooling reinforce the same proof model used by runtime work.

## Beta-floor closure groups

These groups must still be real and bounded, but they do not block the first kiosk/IoT or consumer
core-service floor by themselves:

- **Bringup, Hardware & Drivers**
  `TASK-0244`..`TASK-0259`, `TASK-0271`, `TASK-0272`, `TASK-0280`.
  Goal: hardware bringup beta with explicit device/service contracts.
- **Text, IME, I18N & Accessibility**
  `TASK-0077`, `TASK-0146`..`TASK-0149`, `TASK-0175`, `TASK-0240`, `TASK-0241`.
  Goal: deterministic text and accessibility spine without hidden platform forks.
- **Media & Creative**
  `TASK-0155`, `TASK-0184`, `TASK-0185`, `TASK-0218`, `TASK-0254`.
  Goal: bounded media control and playback/capture floor.
- **Messaging, Search, Store & Sharing**
  `TASK-0122C`, `TASK-0123`, `TASK-0126C`, `TASK-0126D`, `TASK-0153`, `TASK-0154`, `TASK-0213`,
  `TASK-0214`, `TASK-0226`.
  Goal: first real user-facing service surfaces with no authority drift.
- **DSL, App Platform & SDK**
  `TASK-0077B`, `TASK-0077C`, `TASK-0078`, `TASK-0078B`, `TASK-0079`, `TASK-0122B`, `TASK-0163`..`TASK-0166`,
  `TASK-0169B`, `TASK-0274`, `TASK-0280B`, `TASK-0284B`.
  Goal: ecosystem/builder contract closure without sneaking kernel work into app tracks.

## Phase order

### Phase 1 — Release spine

Close first:

- Kernel Core & Runtime
- Security, Policy & Identity
- Storage, PackageFS & Content
- Updates, Packaging & Recovery
- Bringup, Hardware & Drivers

This phase determines whether we can honestly claim secure kiosk/IoT direction and hardware beta.

### Phase 2 — Consumer interaction floor

Close next:

- Windowing, UI & Graphics
- Networking & Transport
- Observability, Crash, Perf & Diagnostics
- Accounts, Ability & Sessions

This phase determines whether the system can feel stable and smooth enough for real daily use.

### Phase 3 — Distributed and ecosystem expansion

Close after the core is honest:

- DSoftBus & Distributed
- Text, IME, I18N & Accessibility
- Media & Creative
- Messaging, Search, Store & Sharing
- DSL, App Platform & SDK
- DevX, Config & Tooling

## Anti-drift rules for gate extraction

- If a group gate adds a prerequisite, update the relevant task `depends-on`, proof section, and
  gate notes in the task itself.
- `production-grade` groups must have explicit negative-path proofs where security or authority is
  involved.
- `production-floor` groups must not claim perf closure without a measurable budget, scene, or
  bounded workload.
- `beta-floor` groups must never emit success markers for placeholder behavior.
