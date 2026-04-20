# Status Board: Task and Track Overview

This board stays intentionally simple:

- one **ongoing Done list** (not capped to a fixed range),
- one **complete TRACK index**.

Source of truth for task status remains each `tasks/TASK-*.md` header.
Sequential execution order remains `tasks/IMPLEMENTATION-ORDER.md`.

---

## Task Groups

This section adds a navigation layer over the full `TASK-*` set. Task files remain the execution truth; the groups below are for drift-free review, gate planning, and fast kernel/service scanning.

| Group | Done / Total | Progress | Kernel-touch tasks | Notes |
|------|---------------|----------|--------------------|-------|
| Kernel Core & Runtime | 6 / 30 | 20% | `TASK-0001`, `TASK-0010`..`TASK-0011`, `TASK-0011B`, `TASK-0012`, `TASK-0012B`, `TASK-0013`, `TASK-0013B`, `TASK-0042`, `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0188`, `TASK-0237`, `TASK-0245`, `TASK-0247`, `TASK-0269`, `TASK-0281`..`TASK-0283`, `TASK-0286`..`TASK-0288`, `TASK-0290` | Kernel scheduling, IPC, MM, QoS, OOM, and hardening authority. |
| DSoftBus & Distributed | 7 / 27 | 26% | — | Distributed session, transport, mux, and remote-service stack. |
| Networking & Transport | 1 / 8 | 12% | — | Netstack, dev networking, ingress, and OS transport services. |
| Observability, Crash, Perf & Diagnostics | 3 / 33 | 9% | — | Logs, traces, crash evidence, perf gates, soak, and diagnostics. |
| Accounts, Ability & Sessions | 0 / 8 | 0% | — | Accounts, ability lifecycle, sessions, greeter, and delegation surfaces. |
| Security, Policy & Identity | 3 / 36 | 8% | `TASK-0008`, `TASK-0019`, `TASK-0028`, `TASK-0043` | Policy authority, identity, sandboxing, ABI guardrails, and security surfaces. |
| Storage, PackageFS & Content | 2 / 25 | 8% | `TASK-0031` | Persistent state, VFS/content contracts, packagefs, quotas, and zero-copy content paths. |
| Updates, Packaging & Recovery | 1 / 21 | 5% | `TASK-0289` | Updates, packages, provisioning, installer, rollback, and recovery tooling. |
| Bringup, Hardware & Drivers | 0 / 12 | 0% | `TASK-0244`, `TASK-0251` | RISC-V bringup, device-class services, input/display/audio, and driver-facing tracks. |
| Windowing, UI & Graphics | 0 / 73 | 0% | — | Early renderer, windowing, compositor, and UI performance floor tasks. |
| Text, IME, I18N & Accessibility | 0 / 9 | 0% | — | Text stack, input methods, locale, and accessibility foundations. |
| Media & Creative | 0 / 5 | 0% | — | Media sessions, audio/video/camera, and creative/media UX slices. |
| Messaging, Search, Store & Sharing | 0 / 9 | 0% | — | Search, sharing, notifications, store, and user-facing data exchange. |
| DSL, App Platform & SDK | 0 / 14 | 0% | — | DSL, app platform, scene/runtime scaffolding, and SDK layers. |
| DevX, Config & Tooling | 1 / 10 | 10% | — | CLI/dev tooling, config/schema plumbing, and repo hygiene. |

---

## Group Gate Targets

Detailed closure contract: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

### Gate Tier Meanings

- `production-grade`: release-blocking. The area must have real enforcement, negative/reject-path proofs where relevant, bounded failure behavior, and enough recovery/security closure that kiosk/IoT and consumer claims would be dishonest without it.
- `production-floor`: real, coherent, and test-backed. The area can still grow in breadth or optimization, but it must already behave predictably under normal and bounded-stress conditions and must not rely on fake-success markers.
- `beta-floor`: real behavior with deterministic proofs, explicit limitations, and no hidden placeholder semantics. Good enough for hardware bringup or subsystem beta work, not good enough yet for release claims.

| Group | Gate tier | Release target | Current closure focus |
|------|-----------|----------------|-----------------------|
| Kernel Core & Runtime | `production-grade` | consumer + kiosk/IoT | `TASK-0286` memory accounting truth, `TASK-0287` memory pressure + OOM enforcement, `TASK-0288` runtime latency/stress closure, `TASK-0290` zero-copy closure |
| DSoftBus & Distributed | `production-floor` | consumer + distributed beta | preserve legacy `TASK-0001`..`TASK-0020` closure while finishing QUIC/authz/busdir/media-remote follow-ons |
| Networking & Transport | `production-floor` | consumer + kiosk/IoT | netstack hardening, ingress bounds, real-connect proofs, virtio-net service closure |
| Observability, Crash, Perf & Diagnostics | `production-floor` | consumer + hardware beta | deterministic crash retention, perf budgets, soak/flake gates, offline diagnostics |
| Accounts, Ability & Sessions | `production-floor` | consumer | session lifecycle, greeter/home isolation, foreground/background enforcement, ability kill semantics |
| Security, Policy & Identity | `production-grade` | consumer + kiosk/IoT | `TASK-0289` boot trust floor, plus `TASK-0286`/`TASK-0287` wherever quota/resource claims depend on kernel truth |
| Storage, PackageFS & Content | `production-grade` | consumer + kiosk/IoT | `TASK-0290` zero-copy closure, plus `TASK-0286`/`TASK-0287` for honest quota/resource enforcement |
| Updates, Packaging & Recovery | `production-grade` | consumer + kiosk/IoT | `TASK-0289` boot trust floor for verified boot, anti-rollback, measured boot, and trusted recovery/update closure |
| Bringup, Hardware & Drivers | `beta-floor` | hardware bringup beta | RISC-V `virt` closure, display/input/audio/battery/thermal/sensor bringup, driver contract proofs |
| Windowing, UI & Graphics | `production-floor` | consumer | first-frame/present/input smoothness, surface reuse, no-trail/no-mosaic floor, frame-budget evidence |
| Text, IME, I18N & Accessibility | `beta-floor` | consumer beta | deterministic text shaping, IME routing, locale switch, accessibility service spine |
| Media & Creative | `beta-floor` | consumer beta | audiod/media-session baseline, bounded decode/capture paths, deterministic UX proofs |
| Messaging, Search, Store & Sharing | `beta-floor` | consumer beta | search/share/notification/store baseline without authority drift or unbounded background work |
| DSL, App Platform & SDK | `beta-floor` | ecosystem beta | DSL/runtime contracts, app-platform proofs, SDK/codegen closure without hidden kernel asks |
| DevX, Config & Tooling | `production-floor` | internal + hardware beta | single CLI convergence, schema/config discipline, deterministic harness and repo hygiene |

### Current Production-Grade Closure Blockers

The tasks below are the current explicit closure set for the remaining kernel/core-service
`production-grade` gaps. If these are still open, release-grade claims for the affected groups stay
incomplete even if broad bring-up or beta-floor functionality exists.

- `TASK-0286` — kernel memory accounting truth: trusted RSS, mapped-bytes, fault/reclaim counters, and pressure snapshots for policy/diagnostic consumers.
- `TASK-0287` — kernel memory pressure + hard-limit enforcement: canonical OOM handoff, pressure watermarks, and real resource-boundary closure.
- `TASK-0288` — kernel runtime closure: latency-budget and stress proofs for SMP/timer/IPI/wakeup behavior under bounded load.
- `TASK-0289` — boot trust floor: verified boot anchors, rollback indices, and measured-boot handoff for updates, recovery, trust store, and device identity claims.
- `TASK-0290` — kernel zero-copy closure: VMO sealing rights, write-map denial, and truthful reuse/copy-fallback evidence for storage/UI hot paths.

Primary group impact:

- `Kernel Core & Runtime`: blocked primarily by `TASK-0286`, `TASK-0287`, `TASK-0288`, and `TASK-0290`.
- `Security, Policy & Identity`: blocked primarily by `TASK-0289`, with `TASK-0286`/`TASK-0287` still required wherever quota/resource claims depend on kernel truth.
- `Storage, PackageFS & Content`: blocked primarily by `TASK-0290`, with `TASK-0286`/`TASK-0287` still needed for honest quota/resource enforcement.
- `Updates, Packaging & Recovery`: blocked primarily by `TASK-0289`.

---

## Task Group Details

Use these groups to review a domain without opening every task file. `Kernel-touch` is derived from task touched paths that mention kernel- or ABI-level code.

### Kernel Core & Runtime

- Progress: `6 / 30` done (`20%`)
- Kernel-touch tasks: `TASK-0001`, `TASK-0010`..`TASK-0011`, `TASK-0011B`, `TASK-0012`, `TASK-0012B`, `TASK-0013`, `TASK-0013B`, `TASK-0042`, `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0188`, `TASK-0237`, `TASK-0245`, `TASK-0247`, `TASK-0269`, `TASK-0281`..`TASK-0283`, `TASK-0286`..`TASK-0288`, `TASK-0290`
- Tasks: `TASK-0001`, `TASK-0010`..`TASK-0011`, `TASK-0011B`, `TASK-0012`, `TASK-0012B`, `TASK-0013`, `TASK-0013B`, `TASK-0042`, `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0188`, `TASK-0228`..`TASK-0230`, `TASK-0237`, `TASK-0245`, `TASK-0247`, `TASK-0267`, `TASK-0269`, `TASK-0276`..`TASK-0277`, `TASK-0281`..`TASK-0290`

### DSoftBus & Distributed

- Progress: `7 / 27` done (`26%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0003`, `TASK-0003B`, `TASK-0003C`, `TASK-0004`..`TASK-0005`, `TASK-0015`..`TASK-0017`, `TASK-0020`..`TASK-0024`, `TASK-0023B`, `TASK-0030`, `TASK-0038`, `TASK-0040`, `TASK-0044`, `TASK-0157`..`TASK-0158`, `TASK-0195`..`TASK-0196`, `TASK-0211`..`TASK-0212`, `TASK-0219`..`TASK-0220`, `TASK-0231`

### Networking & Transport

- Progress: `1 / 8` done (`12%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0016B`, `TASK-0052`, `TASK-0177`, `TASK-0193`..`TASK-0194`, `TASK-0206`, `TASK-0248`..`TASK-0249`

### Observability, Crash, Perf & Diagnostics

- Progress: `3 / 33` done (`9%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0006`, `TASK-0014`, `TASK-0018`, `TASK-0026`, `TASK-0041`, `TASK-0048`..`TASK-0049`, `TASK-0056C`, `TASK-0060`, `TASK-0062B`, `TASK-0080`, `TASK-0141`..`TASK-0145`, `TASK-0152`, `TASK-0170`, `TASK-0172`..`TASK-0173`, `TASK-0183`, `TASK-0190`, `TASK-0201`..`TASK-0202`, `TASK-0205`, `TASK-0216`..`TASK-0217`, `TASK-0227`, `TASK-0234`, `TASK-0236`, `TASK-0242`..`TASK-0243`, `TASK-0264`

### Accounts, Ability & Sessions

- Progress: `0 / 8` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0065`, `TASK-0109`..`TASK-0110`, `TASK-0126B`, `TASK-0159`, `TASK-0223`..`TASK-0224`, `TASK-0235`

### Security, Policy & Identity

- Progress: `3 / 36` done (`8%`)
- Kernel-touch tasks: `TASK-0008`, `TASK-0019`, `TASK-0028`, `TASK-0043`
- Tasks: `TASK-0008`, `TASK-0008B`, `TASK-0019`, `TASK-0027`..`TASK-0029`, `TASK-0039`, `TASK-0043`, `TASK-0047`, `TASK-0053`, `TASK-0066`..`TASK-0068`, `TASK-0103`, `TASK-0107`..`TASK-0108`, `TASK-0111`, `TASK-0124`, `TASK-0126`, `TASK-0130`, `TASK-0136`..`TASK-0137`, `TASK-0139`, `TASK-0160`, `TASK-0162`, `TASK-0167`..`TASK-0168`, `TASK-0181`..`TASK-0182`, `TASK-0189`, `TASK-0191`..`TASK-0192`, `TASK-0221`, `TASK-0238`, `TASK-0259`, `TASK-0263`

### Storage, PackageFS & Content

- Progress: `2 / 25` done (`8%`)
- Kernel-touch tasks: `TASK-0031`
- Tasks: `TASK-0002`, `TASK-0009`, `TASK-0025`, `TASK-0031`..`TASK-0033`, `TASK-0051`, `TASK-0081`, `TASK-0084`, `TASK-0112`, `TASK-0132`..`TASK-0135`, `TASK-0161`, `TASK-0186`..`TASK-0187`, `TASK-0203`..`TASK-0204`, `TASK-0225`, `TASK-0232`..`TASK-0233`, `TASK-0246`, `TASK-0265`, `TASK-0284`

### Updates, Packaging & Recovery

- Progress: `1 / 21` done (`5%`)
- Kernel-touch tasks: `TASK-0289`
- Tasks: `TASK-0007`, `TASK-0034`..`TASK-0037`, `TASK-0050`, `TASK-0089`..`TASK-0090`, `TASK-0129`, `TASK-0131`, `TASK-0140`, `TASK-0174`, `TASK-0178`..`TASK-0180`, `TASK-0197`..`TASK-0198`, `TASK-0239`, `TASK-0260`..`TASK-0261`, `TASK-0289`

### Bringup, Hardware & Drivers

- Progress: `0 / 12` done (`0%`)
- Kernel-touch tasks: `TASK-0244`, `TASK-0251`
- Tasks: `TASK-0055D`, `TASK-0244`, `TASK-0250`..`TASK-0251`, `TASK-0253`, `TASK-0255`..`TASK-0258`, `TASK-0271`..`TASK-0272`, `TASK-0280`

### Windowing, UI & Graphics

- Progress: `0 / 73` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0054`..`TASK-0055`, `TASK-0055B`, `TASK-0055C`, `TASK-0056`, `TASK-0056B`, `TASK-0057`..`TASK-0059`, `TASK-0060B`, `TASK-0061`..`TASK-0064`, `TASK-0067B`, `TASK-0069`..`TASK-0076`, `TASK-0076B`, `TASK-0080B`, `TASK-0080C`, `TASK-0082`..`TASK-0083`, `TASK-0085`..`TASK-0088`, `TASK-0091`..`TASK-0100`, `TASK-0100B`, `TASK-0101`..`TASK-0102`, `TASK-0104`..`TASK-0106`, `TASK-0113`..`TASK-0122`, `TASK-0125`, `TASK-0127`..`TASK-0128`, `TASK-0150`, `TASK-0156`, `TASK-0169`, `TASK-0170B`, `TASK-0171`, `TASK-0176`, `TASK-0199`..`TASK-0200`, `TASK-0207`..`TASK-0208`, `TASK-0215`, `TASK-0275`

### Text, IME, I18N & Accessibility

- Progress: `0 / 9` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0077`, `TASK-0146`..`TASK-0149`, `TASK-0151`, `TASK-0175`, `TASK-0240`..`TASK-0241`

### Media & Creative

- Progress: `0 / 5` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0155`, `TASK-0184`..`TASK-0185`, `TASK-0218`, `TASK-0254`

### Messaging, Search, Store & Sharing

- Progress: `0 / 9` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0122C`, `TASK-0123`, `TASK-0126C`, `TASK-0126D`, `TASK-0153`..`TASK-0154`, `TASK-0213`..`TASK-0214`, `TASK-0226`

### DSL, App Platform & SDK

- Progress: `0 / 14` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0077B`, `TASK-0077C`, `TASK-0078`, `TASK-0078B`, `TASK-0079`, `TASK-0122B`, `TASK-0163`..`TASK-0166`, `TASK-0169B`, `TASK-0274`, `TASK-0280B`, `TASK-0284B`

### DevX, Config & Tooling

- Progress: `1 / 10` done (`10%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0045`..`TASK-0046`, `TASK-0138`, `TASK-0222`, `TASK-0252`, `TASK-0262`, `TASK-0266`, `TASK-0268`, `TASK-0273`, `TASK-0285`

## Done (Ongoing, Cumulative)

| Task | Title | Status | Notes |
|------|-------|--------|-------|
| ✅ TASK-0001 | Runtime roles & boundaries | Done | Single-authority model locked |
| ✅ TASK-0002 | Userspace VFS proof | Done | Marker-gated proof in QEMU |
| ✅ TASK-0003 | Networking: virtio-net + smoltcp + dsoftbusd | Done | OS transport complete |
| ✅ TASK-0003B | DSoftBus Noise XK OS | Done | Handshake + identity binding |
| ✅ TASK-0003C | DSoftBus UDP discovery OS | Done | Loopback discovery |
| ✅ TASK-0004 | Networking: dual-node + identity binding | Done | Identity enforcement |
| ✅ TASK-0005 | Cross-VM DSoftBus + remote proxy | Done | 2-VM harness established |
| ✅ TASK-0006 | Observability v1: logd + crash reports | Done | Journal + nexus-log sink |
| ✅ TASK-0007 | Updates & Packaging v1.0 | Done | A/B skeleton + markers |
| ✅ TASK-0008 | Security hardening v1: policy + audit | Done | Policy engine + audit trail |
| ✅ TASK-0008B | Device identity keys v1 | Done | Keygen flow complete |
| ✅ TASK-0009 | Persistence v1: virtio-blk + statefs | Done | State persistence baseline complete |
| ✅ TASK-0010 | Device MMIO access model | Done | Capability-gated device access complete |
| ✅ TASK-0011 | Kernel simplification phase A | Done | Simplification baseline complete |
| ✅ TASK-0011B | Kernel Rust idioms pre-SMP | Done | Idiom cleanup complete |
| ✅ TASK-0012 | Kernel SMP v1 | Done | Baseline complete |
| ✅ TASK-0012B | Kernel SMP v1b hardening bridge | Done | Hardening complete |
| ✅ TASK-0013 | Perf/Power v1: QoS ABI + timed coalescing | Done | QoS/timing contract complete |
| ✅ TASK-0013B | IPC liveness hardening v1 | Done | Bounded retry/correlation proof complete |
| ✅ TASK-0014 | Observability v2: metrics + tracing | Done | Local observability v2 complete |
| ✅ TASK-0015 | DSoftBusd refactor v1: modular OS daemon structure | Done | Modular daemon baseline complete |
| ✅ TASK-0016 | DSoftBus Remote-FS v1: Remote PackageFS proxy | Done | RFC-0028 gates complete |
| ✅ TASK-0016B | Netstackd refactor v1: modular structure + loop hardening | Done | Seam and governance sync complete |
| ✅ TASK-0017 | DSoftBus Remote-StateFS v1 | Done | Deterministic ACL/audit + 1-VM/2-VM proof complete |
| ✅ TASK-0018 | Crashdumps v1: deterministic minidump + host symbolization | Done | Final hardening + drift lock complete |
| ✅ TASK-0019 | Security v2 (OS): userland ABI syscall guardrails | Done | Kernel-untouched guardrail closure with authenticated profile distribution and deterministic proofs complete |
| ✅ TASK-0020 | DSoftBus Streams v2: mux + flow-control + keepalive | Done | Legacy 0001..0020 production closure gates proven (host/OS/2-VM/perf/soak/release-evidence); closeout synced |
| ✅ TASK-0021 | DSoftBus QUIC v1 host-first scaffold | Done | Real host QUIC transport + QUIC/mux payload proof + deterministic OS fallback markers + strict-mode fail-closed closure synced |
| ✅ TASK-0022 | DSoftBus core refactor: no_std-compatible core + transport abstraction | Done | `dsoftbus-core` no_std crate boundary extracted, required `test_reject_*` + deterministic perf/zero-copy trait evidence green, closure sync complete |
| ✅ TASK-0023 | DSoftBus QUIC v2 OS enabled (gated) | Done | Real OS QUIC-v2 UDP session path shipped: `transport selected quic` + auth/session markers proven; fallback markers rejected in QUIC-required profile |

Current queue head: `TASK-0023B` (`Draft`, in-flight; Phases 1–5 closed 2026-04-17; Phase 6 remaining = replay capability + bounded bisect + cross-host floor).
Current TASK-0023 closure checkpoint: host floors (`just test-dsoftbus-quic`, `quic_selection_contract`, `quic_host_transport_contract`, `quic_feasibility_contract`) and OS QUIC marker floor (`REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`) are green; required markers are `dsoftbusd: transport selected quic`, `dsoftbusd: auth ok`, `dsoftbusd: os session ok`, and `SELFTEST: quic session ok`, with fallback markers forbidden in this profile.
Current TASK-0023B Phase-4 closure checkpoint (2026-04-17): `proof-manifest.toml` is the SSOT for the marker ladder + harness profiles (`full / smp / dhcp / dhcp-strict / os2vm / quic-required`) + runtime profiles (`bringup / quick / ota / net / none`). `nexus-proof-manifest` host CLI (`list-markers / list-env / list-forbidden / list-phases / verify / verify-uart`) drives `scripts/qemu-test.sh` + `tools/os2vm.sh`; `selftest-client/build.rs` generates `markers_generated.rs` (zero hand-written marker literals outside the generated file + `markers.rs`). `arch-gate` is 6/6 mechanical rules (Rule 6: no `REQUIRE_*` env literals in `test-*`/`ci-*` justfile recipe bodies); `[marker_emission]` allowlist is empty. `just test-os PROFILE=…` is canonical (`test-smp / test-os-dhcp / test-os-dhcp-strict / test-dsoftbus-2vm / test-network` deleted; replaced by `ci-os-smp / ci-os-dhcp / ci-os-dhcp-strict / ci-os-os2vm / ci-network`). `verify-uart` deny-by-default analyzer runs after every QEMU pass. `TASK-0024` is now unblocked (Phase-4 dependency dropped). QEMU `SELFTEST:` ladder for `PROFILE=full` byte-identical to the pre-Phase-4 baseline.
Current TASK-0023B Phase-5 closure checkpoint (2026-04-17): every `just test-os PROFILE=…` run writes `target/evidence/<utc>-<profile>-<git-sha>.tar.gz` (manifest tar + uart.log + trace.jsonl + config.json (+ signature.bin when seal is required)). New host-only crate `nexus-evidence` owns canonicalization + Ed25519 sign/verify + secret scan; 102-byte signature wire format with `KeyLabel::{Ci, Bringup}` baked into the signature so `verify --policy=ci` rejects bringup-signed bundles. CI key resolved from env (`NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64`); bringup key from `~/.config/nexus/bringup-key/private.ed25519` with mandatory mode `0600` check. Deny-by-default secret scanner refuses to seal bundles with leaked key material. Post-pass evidence pipeline wired into `scripts/qemu-test.sh` (single bundle) + `tools/os2vm.sh` (per-node A/B bundles). CI gate: `CI=1` ⇒ seal mandatory + rejects `NEXUS_EVIDENCE_DISABLE=1`. `tools/{seal,verify,gen-bringup-key,gen-ci-key}-evidence.sh` + `keys/README.md` rotation procedure shipped. P5-00 prepended at session start: `proof-manifest.toml` (1433 LoC) split into a `proof-manifest/` directory tree (`manifest.toml` + `phases.toml` + `markers/*.toml` + `profiles/*.toml`) with `[meta] schema_version = "2"` + `[include]` glob expansion (lex-sorted, conflict-checked); v1 single-file back-compat retained. 40 tests across 6 integration files in `nexus-evidence` clean; `just dep-gate` clean (zero new forbidden deps; `ed25519-dalek` was already in OS graph via `userspace/updates`). QEMU `SELFTEST:` ladder for `PROFILE=full` byte-identical to pre-Phase-5 baseline.
Next sequencing policy: `TASK-0023B` Phase 6 (replay capability + bounded bisect + cross-host floor, 6 cuts) → `TASK-0024` (reliability/recovery/congestion features) → `TASK-0044` (advanced tuning breadth).
Production closure program note: `RFC-0034` is now done for legacy `TASK-0001..0020` production closure scope.
Task contract seed note: `RFC-0035` is the `TASK-0021` host-first QUIC scaffold contract (strict fallback + no silent downgrade, with explicit `TASK-0022` boundary).
Current TASK-0021 slice status: ✅ Done. Phase-B host proof (real QUIC transport + selection/reject contract + QUIC+mux smoke payload), Phase-C OS fallback marker proof, and Phase-D deterministic perf budget proof are green; host runtime transport selection wiring (`DSOFTBUS_TRANSPORT=tcp|quic|auto`) and targeted gate (`just test-dsoftbus-quic`) are closed and synchronized.

---

## Planned UI/DSL Insertions

These draft tasks intentionally create an earlier visible UI/DSL path so app and SystemUI work can be tested in a real
QEMU window before the later display/system migration tasks fully land.

| Task | Purpose |
|------|---------|
| TASK-0054B | kernel/UI perf floor (zero-copy + trusted scheduling + SMP hardening carry-ins) |
| TASK-0054C | kernel IPC fastpath v1 for short control messages |
| TASK-0054D | kernel MM perf floor for VMO/surface reuse |
| TASK-0055B | visible QEMU scanout bootstrap |
| TASK-0055C | `windowd` visible present + SystemUI first frame |
| TASK-0055D | deterministic QEMU dev display/profile presets (`phone/tablet/laptop/laptop-pro/convertible` + orientation + shell mode + Hz) |
| TASK-0056B | visible input v0 (cursor/focus/click) |
| TASK-0056C | present/input perf polish (latency + coalescing + skip paths) |
| TASK-0060B | glass materials + backdrop cache + deterministic degrade |
| TASK-0062B | animation frame-budget discipline + canonical perf scenes |
| TASK-0067B | clipboard history DSL overlay/app |
| TASK-0076B | visible DSL OS mount + first DSL frame |
| TASK-0080B | bootstrap SystemUI DSL shell (host-first) |
| TASK-0080C | bootstrap SystemUI DSL shell (OS/QEMU) |
| TASK-0100B | Audio Mixer DSL app/SystemUI surface |
| TASK-0122B | shared DSL app platform |
| TASK-0122C | shared DSL app integration kit |

---

## RFC Done (Ongoing, Cumulative)

| RFC | Description | File |
|-----|-------------|------|
| ✅ RFC-0001 | Kernel Simplification | `docs/rfcs/RFC-0001-kernel-simplification.md` |
| ✅ RFC-0002 | Process-Per-Service Architecture | `docs/rfcs/RFC-0002-process-per-service-architecture.md` |
| ✅ RFC-0003 | Unified Logging Infrastructure | `docs/rfcs/RFC-0003-unified-logging.md` |
| ✅ RFC-0004 | Loader Safety & Shared-Page Guards | `docs/rfcs/RFC-0004-safe-loader-guards.md` |
| ✅ RFC-0005 | Kernel IPC & Capability Model | `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` |
| ✅ RFC-0006 | Userspace Networking v1 | `docs/rfcs/RFC-0006-userspace-networking-v1.md` |
| ✅ RFC-0007 | DSoftBus OS Transport v1 | `docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md` |
| ✅ RFC-0008 | DSoftBus Noise XK v1 | `docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md` |
| ✅ RFC-0009 | no_std Dependency Hygiene v1 | `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md` |
| ✅ RFC-0010 | DSoftBus Cross-VM Harness v1 | `docs/rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md` |
| ✅ RFC-0011 | logd journal + crash reports v1 | `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` |
| ✅ RFC-0012 | Updates & Packaging v1.0 (A/B skeleton) | `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` |
| ✅ RFC-0013 | Boot gates v1 | `docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md` |
| ✅ RFC-0014 | Testing contracts v1 | `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` |
| ✅ RFC-0015 | Policy Authority & Audit Baseline v1 | `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` |
| ✅ RFC-0016 | Device Identity Keys v1 | `docs/rfcs/RFC-0016-device-identity-keys-v1.md` |
| ✅ RFC-0017 | Device MMIO Access Model v1 | `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` |
| ✅ RFC-0018 | StateFS Journal Format v1 | `docs/rfcs/RFC-0018-statefs-journal-format-v1.md` |
| ✅ RFC-0019 | IPC Request/Reply Correlation v1 | `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` |
| ✅ RFC-0020 | Kernel ownership + Rust idioms pre-SMP v1 | `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` |
| ✅ RFC-0021 | Kernel SMP v1 contract | `docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md` |
| ✅ RFC-0022 | Kernel SMP v1b hardening contract | `docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md` |
| ✅ RFC-0023 | QoS ABI + timed coalescing contract v1 | `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md` |
| ✅ RFC-0024 | Observability v2 local contract | `docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md` |
| ✅ RFC-0025 | IPC liveness hardening v1 | `docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md` |
| ✅ RFC-0026 | IPC performance optimization v1 | `docs/rfcs/RFC-0026-ipc-performance-optimization-contract-v1.md` |
| ✅ RFC-0027 | DSoftBusd modular daemon structure v1 | `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md` |
| ✅ RFC-0028 | DSoftBus remote packagefs RO v1 | `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md` |
| ✅ RFC-0029 | Netstackd modular daemon structure v1 | `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md` |
| ✅ RFC-0030 | DSoftBus remote statefs RW v1 | `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md` |
| ✅ RFC-0031 | Crashdumps v1 + host symbolization | `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md` |
| ✅ RFC-0032 | ABI syscall guardrails v2 (userland, kernel-untouched) | `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md` |

Current RFC closure status: `RFC-0033`, `RFC-0034`, `RFC-0035`, `RFC-0036`, and `RFC-0037` are `Done/Complete`.

---

## TRACK Index (Complete List)

| Track | File |
|-------|------|
| TRACK-ADS-SAFETY-FAMILYMODE | `tasks/TRACK-ADS-SAFETY-FAMILYMODE.md` |
| TRACK-APP-STORE | `tasks/TRACK-APP-STORE.md` |
| TRACK-ARCADE-APP | `tasks/TRACK-ARCADE-APP.md` |
| TRACK-AUTHORITY-NAMING | `tasks/TRACK-AUTHORITY-NAMING.md` |
| TRACK-CORE-UTILITIES | `tasks/TRACK-CORE-UTILITIES.md` |
| TRACK-CREATIVE-APPS | `tasks/TRACK-CREATIVE-APPS.md` |
| TRACK-DAW-APP | `tasks/TRACK-DAW-APP.md` |
| TRACK-DEVSTUDIO-IDE | `tasks/TRACK-DEVSTUDIO-IDE.md` |
| TRACK-DRIVERS-ACCELERATORS | `tasks/TRACK-DRIVERS-ACCELERATORS.md` |
| TRACK-DSL-V1-DEVX | `tasks/TRACK-DSL-V1-DEVX.md` |
| TRACK-FEEDS-APP | `tasks/TRACK-FEEDS-APP.md` |
| TRACK-KEYSTONE-GATES | `tasks/TRACK-KEYSTONE-GATES.md` |
| TRACK-LIVE-STUDIO-APP | `tasks/TRACK-LIVE-STUDIO-APP.md` |
| TRACK-LOCATION-STACK | `tasks/TRACK-LOCATION-STACK.md` |
| TRACK-MAIL-APP | `tasks/TRACK-MAIL-APP.md` |
| TRACK-MAPS-APP | `tasks/TRACK-MAPS-APP.md` |
| TRACK-MEDIA-APPS | `tasks/TRACK-MEDIA-APPS.md` |
| TRACK-NETWORKING-DRIVERS | `tasks/TRACK-NETWORKING-DRIVERS.md` |
| TRACK-NEXUSACCOUNT | `tasks/TRACK-NEXUSACCOUNT.md` |
| TRACK-NEXUSFRAME | `tasks/TRACK-NEXUSFRAME.md` |
| TRACK-NEXUSGAME-SDK | `tasks/TRACK-NEXUSGAME-SDK.md` |
| TRACK-NEXUSGFX-SDK | `tasks/TRACK-NEXUSGFX-SDK.md` |
| TRACK-NEXUSINFER-SDK | `tasks/TRACK-NEXUSINFER-SDK.md` |
| TRACK-NEXUSMEDIA-SDK | `tasks/TRACK-NEXUSMEDIA-SDK.md` |
| TRACK-NEXUSNET-SDK | `tasks/TRACK-NEXUSNET-SDK.md` |
| TRACK-NEXUSSOCIAL | `tasks/TRACK-NEXUSSOCIAL.md` |
| TRACK-NEXUSVIDEO | `tasks/TRACK-NEXUSVIDEO.md` |
| TRACK-NOTES-APP | `tasks/TRACK-NOTES-APP.md` |
| TRACK-OFFICE-SUITE | `tasks/TRACK-OFFICE-SUITE.md` |
| TRACK-PASSWORD-MANAGER | `tasks/TRACK-PASSWORD-MANAGER.md` |
| TRACK-PIM-SUITE | `tasks/TRACK-PIM-SUITE.md` |
| TRACK-PINBALL-APP | `tasks/TRACK-PINBALL-APP.md` |
| TRACK-PODCASTS-APP | `tasks/TRACK-PODCASTS-APP.md` |
| TRACK-PRODUCTION-GATES-KERNEL-SERVICES | `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` |
| TRACK-PUZZLE-APP | `tasks/TRACK-PUZZLE-APP.md` |
| TRACK-RECIPES-APP | `tasks/TRACK-RECIPES-APP.md` |
| TRACK-REFERENCE-GAMES | `tasks/TRACK-REFERENCE-GAMES.md` |
| TRACK-REMOVABLE-STORAGE | `tasks/TRACK-REMOVABLE-STORAGE.md` |
| TRACK-SCORE-APP | `tasks/TRACK-SCORE-APP.md` |
| TRACK-SYSTEM-DELEGATION | `tasks/TRACK-SYSTEM-DELEGATION.md` |
| TRACK-TELEPROMPTER-APP | `tasks/TRACK-TELEPROMPTER-APP.md` |
| TRACK-TERMINAL-APP | `tasks/TRACK-TERMINAL-APP.md` |
| TRACK-VIDEO-EDITOR-APP | `tasks/TRACK-VIDEO-EDITOR-APP.md` |
| TRACK-WEATHER-APP | `tasks/TRACK-WEATHER-APP.md` |
| TRACK-ZEROCOPY-APP-PLATFORM | `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md` |

---

## Related

- Sequential execution order: `tasks/IMPLEMENTATION-ORDER.md`
- Task workflow rules: `tasks/README.md`
- RFC process: `docs/rfcs/README.md`
