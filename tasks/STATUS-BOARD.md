# Status Board: Task and Track Overview

This board stays intentionally simple:

- one **ongoing Done list** (not capped to a fixed range),
- one **complete TRACK index**.

Source of truth for task status remains each `tasks/TASK-*.md` header.
Sequential execution order remains `tasks/IMPLEMENTATION-ORDER.md`.

---

## Task Groups

This section adds a navigation layer over the full `TASK-*` set. Task files remain the execution truth; the groups below are for drift-free review, gate planning, and fast kernel/service scanning.

> **Reconciliation note (2026-07-19):** The cumulative Done list below was reconciled against real proof markers/tests (post-0064 UI/DSL, SMP core 0042/0276/0277/0283/0288, filesystem 0291–0295, boot-gates 0269, bundlemgr 0130). The per-group `Done / Total` counters are hand-maintained approximate mirrors and lag the Done list; treat the Done list + each `TASK-*.md` header as authoritative. Counters for the groups touched by this reconciliation (Storage, Windowing) are refreshed here; the rest are refreshed opportunistically.

| Group | Done / Total | Progress | Kernel-touch tasks | Notes |
|------|---------------|----------|--------------------|-------|
| Kernel Core & Runtime | 6 / 30 | 20% | `TASK-0001`, `TASK-0010`..`TASK-0011`, `TASK-0011B`, `TASK-0012`, `TASK-0012B`, `TASK-0013`, `TASK-0013B`, `TASK-0042`, `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0188`, `TASK-0237`, `TASK-0245`, `TASK-0247`, `TASK-0269`, `TASK-0281`..`TASK-0283`, `TASK-0286`..`TASK-0288`, `TASK-0290` | Kernel scheduling, IPC, MM, QoS, OOM, and hardening authority. |
| DSoftBus & Distributed | 7 / 27 | 26% | — | Distributed session, transport, mux, and remote-service stack. |
| Networking & Transport | 1 / 8 | 12% | — | Netstack, dev networking, ingress, and OS transport services. |
| Observability, Crash, Perf & Diagnostics | 3 / 33 | 9% | — | Logs, traces, crash evidence, perf gates, soak, and diagnostics. |
| Accounts, Ability & Sessions | 1 / 9 | 11% | `TASK-0065B` | Accounts, ability lifecycle, sessions, greeter, and delegation surfaces. |
| Security, Policy & Identity | 4 / 36 | 11% | `TASK-0008`, `TASK-0019`, `TASK-0028`, `TASK-0043`, `TASK-0047` | Policy authority, identity, sandboxing, ABI guardrails, and security surfaces. |
| Storage, PackageFS & Content | 9 / 30 | 30% | `TASK-0031` | Persistent state, VFS/content contracts, packagefs, quotas, and zero-copy content paths. FS ladder `TRACK-STASH-USER-DATA-FS` (RFC-0071/0072/0073 → TASK-0291..0295) Done. |
| Updates, Packaging & Recovery | 1 / 21 | 5% | `TASK-0289` | Updates, packages, provisioning, installer, rollback, and recovery tooling. |
| Bringup, Hardware & Drivers | 0 / 11 | 0% | `TASK-0244`, `TASK-0251` | RISC-V bringup, device-class services, display/audio, and driver-facing tracks. |
| Windowing, UI & Graphics | 27 / 76 | 36% | — | Early renderer, windowing, compositor, UI/input performance floor, and Orbital-Level UX gates. |
| Text, IME, I18N & Accessibility | 0 / 7 | 0% | — | Text stack, input methods, locale, and accessibility foundations. |
| Media & Creative | 0 / 5 | 0% | — | Media sessions, audio/video/camera, and creative/media UX slices. |
| Messaging, Search, Store & Sharing | 0 / 9 | 0% | — | Search, sharing, notifications, store, and user-facing data exchange. |
| DSL, App Platform & SDK | 2 / 14 | 14% | — | DSL, app platform, scene/runtime scaffolding, and SDK layers. |
| DevX, Config & Tooling | 2 / 10 | 20% | — | CLI/dev tooling, config/schema plumbing, and repo hygiene. |

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
| Bringup, Hardware & Drivers | `beta-floor` | hardware bringup beta | RISC-V `virt` closure, display/audio/battery/thermal/sensor bringup, driver contract proofs |
| Windowing, UI & Graphics | `production-floor` | consumer | live QEMU pointer/scroll/launcher proof, first-frame/present/input smoothness, SVG-source UI assets, no-trail/no-mosaic floor, frame-budget evidence |
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

- Progress: `0 / 9` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0065`, `TASK-0065B`, `TASK-0109`..`TASK-0110`, `TASK-0126B`, `TASK-0159`, `TASK-0223`..`TASK-0224`, `TASK-0235`

### Security, Policy & Identity

- Progress: `3 / 36` done (`8%`)
- Kernel-touch tasks: `TASK-0008`, `TASK-0019`, `TASK-0028`, `TASK-0043`
- Tasks: `TASK-0008`, `TASK-0008B`, `TASK-0019`, `TASK-0027`..`TASK-0029`, `TASK-0039`, `TASK-0043`, `TASK-0047`, `TASK-0053`, `TASK-0066`..`TASK-0068`, `TASK-0103`, `TASK-0107`..`TASK-0108`, `TASK-0111`, `TASK-0124`, `TASK-0126`, `TASK-0130`, `TASK-0136`..`TASK-0137`, `TASK-0139`, `TASK-0160`, `TASK-0162`, `TASK-0167`..`TASK-0168`, `TASK-0181`..`TASK-0182`, `TASK-0189`, `TASK-0191`..`TASK-0192`, `TASK-0221`, `TASK-0238`, `TASK-0259`, `TASK-0263`

### Storage, PackageFS & Content

- Progress: `9 / 30` done (`30%`)
- Kernel-touch tasks: `TASK-0031`
- Tasks: `TASK-0002`, `TASK-0009`, `TASK-0025`, `TASK-0031`..`TASK-0033`, `TASK-0051`, `TASK-0081`, `TASK-0084`, `TASK-0112`, `TASK-0132`..`TASK-0135`, `TASK-0161`, `TASK-0186`..`TASK-0187`, `TASK-0203`..`TASK-0204`, `TASK-0225`, `TASK-0232`..`TASK-0233`, `TASK-0246`, `TASK-0265`, `TASK-0284`, `TASK-0291`..`TASK-0295`
- Notes (2026-07-15): `TASK-0025`..`TASK-0027` rebased onto shipped statefs v1; `TASK-0033` superseded by `TASK-0295`; `TASK-0182`/`TASK-0183` superseded by RFC-0071 (nxfs encryption classes); ladder + contracts in `tasks/TRACK-STASH-USER-DATA-FS.md`.

### Updates, Packaging & Recovery

- Progress: `1 / 21` done (`5%`)
- Kernel-touch tasks: `TASK-0289`
- Tasks: `TASK-0007`, `TASK-0034`..`TASK-0037`, `TASK-0050`, `TASK-0089`..`TASK-0090`, `TASK-0129`, `TASK-0131`, `TASK-0140`, `TASK-0174`, `TASK-0178`..`TASK-0180`, `TASK-0197`..`TASK-0198`, `TASK-0239`, `TASK-0260`..`TASK-0261`, `TASK-0289`

### Bringup, Hardware & Drivers

- Progress: `0 / 11` done (`0%`)
- Kernel-touch tasks: `TASK-0244`, `TASK-0251`
- Tasks: `TASK-0055D`, `TASK-0244`, `TASK-0250`..`TASK-0251`, `TASK-0255`..`TASK-0258`, `TASK-0271`..`TASK-0272`, `TASK-0280`

### Windowing, UI & Graphics

- Progress: `27 / 76` done (`36%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0054`..`TASK-0055`, `TASK-0055B`, `TASK-0055C`, `TASK-0056`, `TASK-0056B`, `TASK-0056C`, `TASK-0057`..`TASK-0059`, `TASK-0060B`, `TASK-0061`..`TASK-0064`, `TASK-0067B`, `TASK-0069`..`TASK-0076`, `TASK-0076B`, `TASK-0080B`, `TASK-0080C`, `TASK-0082`..`TASK-0083`, `TASK-0085`..`TASK-0088`, `TASK-0091`..`TASK-0100`, `TASK-0100B`, `TASK-0101`..`TASK-0102`, `TASK-0104`..`TASK-0106`, `TASK-0113`..`TASK-0122`, `TASK-0125`, `TASK-0127`..`TASK-0128`, `TASK-0146`..`TASK-0147`, `TASK-0150`, `TASK-0156`, `TASK-0169`, `TASK-0170B`, `TASK-0171`, `TASK-0176`, `TASK-0199`..`TASK-0200`, `TASK-0207`..`TASK-0208`, `TASK-0215`, `TASK-0252`..`TASK-0253`, `TASK-0275`

### Text, IME, I18N & Accessibility

- Progress: `0 / 7` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0077`, `TASK-0148`..`TASK-0149`, `TASK-0151`, `TASK-0175`, `TASK-0240`..`TASK-0241`

### Media & Creative

- Progress: `0 / 5` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0155`, `TASK-0184`..`TASK-0185`, `TASK-0218`, `TASK-0254`

### Messaging, Search, Store & Sharing

- Progress: `0 / 9` done (`0%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0122C`, `TASK-0123`, `TASK-0126C`, `TASK-0126D`, `TASK-0153`..`TASK-0154`, `TASK-0213`..`TASK-0214`, `TASK-0226`

### DSL, App Platform & SDK

- Progress: `2 / 14` done (`14%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0077B`, `TASK-0077C`, `TASK-0078`, `TASK-0078B`, `TASK-0079`, `TASK-0122B`, `TASK-0163`..`TASK-0166`, `TASK-0169B`, `TASK-0274`, `TASK-0280B`, `TASK-0284B`

### DevX, Config & Tooling

- Progress: `2 / 9` done (`22%`)
- Kernel-touch tasks: —
- Tasks: `TASK-0045`..`TASK-0046`, `TASK-0138`, `TASK-0222`, `TASK-0262`, `TASK-0266`, `TASK-0268`, `TASK-0273`, `TASK-0285`

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
| ✅ TASK-0029 | Supply-Chain v1: SBOM + repro metadata + signature allowlist policy | Done | Host reject-path proofs + OS supply-chain marker gate green; docs and tracking synced |
| ✅ TASK-0031 | Zero-copy VMOs v1: shared RO buffers + handle transfer | Done | Host-first + OS-gated VMO plumbing closure complete; kernel production-grade dependencies remain explicit follow-up scope |
| ✅ TASK-0046 | Config v1: configd + JSON Schema + layering + 2PC reload | Done | Host-first config authority closure complete; `RFC-0044` done, JSON-only authoring enforced, `configd`/`nx config` contract synced |
| ✅ TASK-0054 | UI v1a: BGRA8888 CPU renderer + damage tracking + headless snapshots | Done | Host-first renderer/snapshot proof floor complete; no OS/QEMU present marker claim |
| ✅ TASK-0055 | UI v1b: windowd compositor + surfaces/layers IPC + VMO buffers + vsync | Done | Headless state-machine, generated IDL roundtrip, marker, postflight, and reject proofs complete |
| ✅ TASK-0055B | UI v1c: visible QEMU scanout bootstrap | Done | Visible QEMU `ramfb` bootstrap path proven with marker-honesty hardening and full closure gates green |
| ✅ TASK-0055C | UI v1d: windowd visible present + SystemUI first frame in QEMU | Done | Composed-frame visible present proven (host full frame + OS row path) with QEMU marker ladder and closure gates green |
| ✅ TASK-0056 | UI v2a: double-buffered surfaces + present scheduler + input routing | Done | Host/reject/QEMU proofs and closure gates (`fmt-clippy-deny`, `test-all`, `ci-network`, `make clean/build/test/run`) are green |
| ✅ TASK-0056B | UI v2a: visible input — cursor + hover + focus + click | Done | Deterministic host/reject/QEMU proofs and closure gates are green; live QEMU pointer/keyboard follow-up remains `TASK-0252`/`TASK-0253` |
| ✅ TASK-0252 | Input v1.0a: host HID/touch/keymaps/repeat/pointer-accel core | Done | Host-first proofs + reject suites green; closure gates rerun green; live QEMU marker closure remains `TASK-0253` |
| ✅ TASK-0253 | Input v1.0b: OS/QEMU hidrawd + touchd + inputd + windowd/IME hooks + selftests | Done | Real `virtio-input -> hidrawd -> inputd -> windowd -> fbdevd -> ramfb` chain is closed with focused proofs and broad closure gates green |
| ✅ TASK-0056C | UI v2a: present/input perf latency + coalescing + no-damage-skip + idle-cheap | Done | Host-first coalescing + no-damage-skip + idle-cheap proofs green; host-integration tests proving semantic-edge preservation; QEMU diag-os pending downstream |
| ✅ TASK-0057 | UI v2b: Minimal DisplayServer v0 (cursor/wallpaper/text/assets/input targets) | Done | Mocu cursor, JPEG wallpaper, Inter proof text, hover/click/scroll/key targets; visible-bootstrap ladder green; just test-all green |
| ✅ TASK-0058 | UI v3a: layout wrapping + deterministic box model | Done | Flex/grid/stack layout, text wrapping, host goldens; production-grade windowd integration |
| ✅ TASK-0059 | UI v3b: clip + scroll + backdrop effects + shadow pipeline + IME + MSDF/SDF rendering | Done | ShadowArena, per-box caching, compositor/ module refactor; host/reject/QEMU proofs green |
| ✅ TASK-0062 | UI v5a: Deterministic Animation + NexusGfx 2D Pipeline + GPU Driver Contract | Done | Animation engine, NexusGfx SDK, gpud, windowd integration; all phases 0-6e proven; RFC-0059 Complete |
| ✅ TASK-0063 | UI v5b: virtualized list + scene graph + dual-panel GPU blur + virgl pipeline + theme tokens | Done | `nexus-virtual-list` + `nexus-theme`, scene-graph compositor, virgl 3D GPU blur (CPU fallback), soft-real-time pacing (RFC-0033 spine); boot-verified over `GPU_MODE=virgl just start`; RFC-0063 Complete |
| ✅ TASK-0064 | UI v6a: window management v1 — ShellWindow N-window WM (chat instance + title-bar/X/drag/z-order) | Done | `ShellWindow` + host-tested `window_frame::Frame`, N-window WM (chat + search instances), drag/bounds-clamp/z-order, Lucide X-close; marker ladder (`windowd: wm on`, `chat button click ok`, `chat window open/close/drag ok`, `SELFTEST: ui v6 wm ok`); boot-verified over virgl. Scene-transitions (Crossfade/Slide) → TASK-0064B |
| ✅ TASK-0060 | UI v4a: tiled compositor + clip-stack + atlases + perf | Done | Reconciled 2026-07-19; ui v4 markers |
| ✅ TASK-0060B | UI v4b: glass materials + backdrop-cache + degrade | Done | Reconciled 2026-07-19 |
| ✅ TASK-0061 | UI v4b: gestures + a11y semantics | Done | Reconciled 2026-07-19; a11y-hardening folded → TASK-0114 |
| ✅ TASK-0062B | UI v5a: animation frame-budget + perf scenes | Done | Reconciled 2026-07-19 |
| ✅ TASK-0075 | DSL v0.1a: lexer/parser → AST + Scene-IR + lowering + nx dsl CLI | Done | Reconciled 2026-07-19; dsl_conformance |
| ✅ TASK-0076 | DSL v0.1b: interpreter + snapshots + OS demo hook | Done | Reconciled 2026-07-19; dsl_goldens |
| ✅ TASK-0077 | DSL v0.2a: state/nav/i18n core | Done | Reconciled 2026-07-19 |
| ✅ TASK-0078 | DSL v0.2b: service stubs + CLI demo | Done | Reconciled 2026-07-19 |
| ✅ TASK-0080 | DSL v0.3b: perf-bench + OS AOT demo | Done | Reconciled 2026-07-19 |
| ✅ TASK-0080B | SystemUI DSL bootstrap shell (host-first) | Done | Reconciled 2026-07-19; bootstrap host test |
| ✅ TASK-0080C | SystemUI DSL bootstrap shell: OS-wiring + QEMU markers | Done | Reconciled 2026-07-19 |
| ✅ TASK-0080D | DSL app runtime lifecycle + surface contract | Done | Reconciled 2026-07-19 |
| ✅ TASK-0130 | Packages v1b: bundlemgrd install/upgrade/uninstall + trust policy | Done | Reconciled 2026-07-19; bundlemgrd markers; mimed-handler line deferred (no successor) |
| ✅ TASK-0269 | Boot gates v1: readiness + spawn-reason + resource sentinel | Done | Reconciled 2026-07-19; kselftest markers |
| ✅ TASK-0042 | SMP v2: affinity + QoS budgets + kernel ABI | Done | kselftest smp/bkl markers |
| ✅ TASK-0276 | Parallelism v1: deterministic threadpools + policy contract | Done | Reconciled 2026-07-19 |
| ✅ TASK-0277 | Kernel SMP parallelism policy v1 (deterministic) | Done | Reconciled 2026-07-19 |
| ✅ TASK-0283 | Kernel per-CPU ownership wrapper v1 | Done | Reconciled 2026-07-19 |
| ✅ TASK-0288 | Kernel runtime closure v1c: latency budgets + stress proofs | Done | Reconciled 2026-07-19 |
| ✅ TASK-0291 | VFS ReadDir + svc.files + filemanager role + stash real listing | Done | Boot-proven (was In Review) |
| ✅ TASK-0292 | nxfs v1 core (host-first): engine + fsck + crash-injection | Done | Host-proven, 17 tests (was In Review) |
| ✅ TASK-0293 | nxfs /data OS bring-up (2nd blk device + vfsd DataStore) | Done | Write + cold-boot persistence boot-proven (was In Review) |
| ✅ TASK-0294 | MIME SSOT: nexus-mime-icons + stash filetype icons | Done | 39-icon SSOT boot-proven (was In Review) |
| ✅ TASK-0295 | Zero-copy read/write via VMO splice (OP_READ_VMO CAP_MOVE) | Done | Boot-proven (was In Review) |
| ✅ TASK-0070 | UI v8b: WM resize/move/snap/dock | Done | Reconciled 2026-07-19; wm.rs/snap.rs/dock.rs + 23 tests. Shortcuts = Non-Goal (rejected by design); overlays → 0072 |
| ✅ TASK-0072 | UI v9b: settingsd + settings panel DSL app | Done | Reconciled 2026-07-19; settingsd markers + settings.rs test. prefsd→settingsd; quick-settings dropped |
| ✅ TASK-0073 | UI v10a: design-system primitives + goldens | Done | Reconciled 2026-07-19; 37 widget crates + 74 goldens + a11y lints |
| ✅ TASK-0078B | DSL v0.2b: QuerySpec v1 (paging + hash) | Done | Reconciled 2026-07-19; nexus-query (1264 LOC) + queryd + paging tests + pinned hash golden |
| ✅ TASK-0119 | SystemUI→DSL Phase 1a: Launcher + Control-Center DSL pages | Done | Reconciled 2026-07-19; dsl_apps_conformance (desktop-shell); re-arch path |
| ✅ TASK-0120 | SystemUI→DSL Phase 1b: OS wiring | Done | Reconciled 2026-07-19; `systemui: dsl shell on` boot-proven |
| ✅ TASK-0121 | SystemUI→DSL Phase 2a: Settings + Notifications Center surface | Done | Reconciled 2026-07-19; settings host-tested; real notif delivery folded → 0123–0125 |
| ⤳ TASK-0076B | DSL v0.1c: visible OS mount + first DSL frame | Superseded | Superseded by TASK-0080C (own demo retired; capability lives in 0080C) |

`TASK-0065` / UI v6b app lifecycle + notifications + navigation — **DONE (2026-06-23)**. RFC-0065 + ADR-0036/0037; `bundlemgrd` registry **generated from real `bundles/<app>/manifest.toml`** at build time (no hardcoded list; phantom `notes` removed; `windowd: apps ok (n=2)` chat/search); `abilitymgr` real service + lifecycle broker + **manifest-caps launch authority** (fail-closed `STATUS_DENIED`; `abilitymgr: caps ok app=<id>`); policyd `BundleQuery` gating + greppable `!route-deny`/`!cap-deny`; real `.nxb` bundles + Cap'n Proto manifests; per-app-surface model (ADR-0037); `search-app` (no_std) owns its data, windowd hosts it. 25 abilitymgr + 2 nxb-pack + 126 windowd + 10 search-app tests, riscv-checked. **Descoped to follow-ups:** apps as spawned processes w/ own surfaces → DSL App Runtime **`TASK-0080D`** (execd only runs asm stubs today; needs a userspace app runtime + surface handoff) + `TASK-0234`/`0235` + SystemUI DSL phases.
`TASK-0065B` / Session v1: sessiond session authority + login greeter + SystemUI shell selection — **DONE (2026-07-02)**. `sessiond` = the session authority (host-tested `Greeter → Active` state machine, `Locked`/`OP_LOCK` reserved; manifest user registry `users.toml` with optional `auto_login`; wire protocol `nexus_abi::sessiond` GET_STATE/LOGIN, golden-frame-tested). Login greeter in windowd: blurred+dimmed wallpaper baked into Plane 1 (separable box blur, no atlas cost), round SDF avatar + Lucide `circle-user` + name, hover, click → `OP_LOGIN` → session shell via SystemUI `resolve_product` (the user's `product` selects the shell — profiles.md contract; greeter appearance from `manifests/greeter/default/greeter.toml`). Pre-session gating at BOTH layers: windowd suppresses all shell affordances (host-tested `resolve_click_session`) AND `abilitymgr` refuses `OP_LAUNCH` fail-closed via injected `SessionGate` + live sessiond query (`abilitymgr: launch denied (session)`). Never bricks: bounded probe → `windowd: session unavailable (auto shell)` (proven via an OS_SKIP=sessiond boot). Proof injector logs in like a user; ladder + `docs/dev/ui/shell/session.md` shipped; windowd heap → 2MiB (`heap-2m`). Boot-verified over virgl (greeter → click → desktop). Follow-ups: credential auth behind OP_LOGIN, lock/unlock UI, session switching, multi-user avatar grid.

**Dynamic Apps menu — authority + model DONE (host-verified)**: `bundlemgrd` OS-lite now serves the real app list via `OP_LIST_APPS` (`nexus_abi::bundlemgrd` wire + seeded registry chat/search/notes; windowd+abilitymgr allow-listed senders); windowd `app_menu::AppMenu` parses it with a Chat/Search **seed fallback** (no boot regression). Tests: nexus-abi 25, bundlemgrd 13, windowd 120 (+5 app_menu), riscv clean.
**Boot-gated next**: the windowd compositor *consumption* — fetch `OP_LIST_APPS` at init + size the dropdown atlas from the count + render/hit-test/dispatch from `AppMenu` (replaces `const DROPDOWN_ITEMS`). This is surgery in the fragile `runtime/mod.rs` (init ordering + atlas + render) → needs a real boot to verify. Then the chat/search compositor swap.
Recommend: boot+commit P0–here (watch for `bundlemgrd: list_apps.ok`), then do the compositor consumption with boot feedback.
**Boot 2026-06-22T17:17 finding + fix**: `abilitymgr: ready` appeared but the service then crashed (`route fallback slots`→`recv error`→exit, non-fatal) because `nexus-init` had no `abilitymgr` orchestrator arm (fell into `_ => {}`) → no server endpoint. **Fixed** (committed-ready, host+riscv green): `route_table.rs` `ServiceId::Abilitymgr=14` + a best-effort `"abilitymgr"` orchestrator arm that creates a server endpoint + transfers recv/send to slots 3/4 (can't brick boot). Boot-verify: `init: abilitymgr slots recv=0x3 send=0x4` + `abilitymgr: ready` stays up (no recv error). Crash-fix boot-confirmed (run 17:35: `init: abilitymgr slots recv=0x3 send=0x4`, 0× recv-error, abilitymgr stays up).
**Architectural pivot → RFC-0066 (production-grade service chain).** The repeated boot-only failures (abilitymgr crash, X→bundlemgrd CAP_MOVE) are an unstable-architecture symptom: init hand-wires every pair (1500-line match), CAP_MOVE is copy-pasted per client, the chain isn't host-testable. RFC-0066 adopts mature service-framework principles incrementally: **declarative capability routes + one typed brokered client + in-process chain tests.** Phase 1 done (additive, zero boot risk): `nexus_ipc::connection::Connection` — one reusable typed request/reply client over a `Transport`, host-tested via mock (3 tests, the abilitymgr→bundlemgrd `KernelClient` probe that failed `unreachable` will be replaced by this); `route_table::REQUIRED_ROUTES` declarative route SSOT + consistency tests. Tests: nexus-ipc 25, riscv clean, no regressions (abilitymgr 21 / windowd 120 / bundlemgrd 13). Note: `route_table` is os-gated → host-testability of the router is Phase 2. Phase 2 = broker connect via samgrd; Phase 3 = data-driven orchestrator from REQUIRED_ROUTES (kills the abilitymgr-class bug structurally); Phase 4 = IDL-typed proxies.
**RFC-0066 Phase 2 (testability) done**: extracted `nexus-init::service_topology` — a **host-compilable** declarative SSOT (`ServiceId` + `REQUIRED_ROUTES` + `ServiceSpec`), decoupled from the OS capability binding (`route_table`/`Rights` stays os-only, re-exports `ServiceId`). The `.cml`-equivalent: pure data, host-tested. 4 host tests cross-validate the two SSOTs so a service whose route is added in one place but not the other is a `cargo test` failure (the abilitymgr-class bug, caught at declaration level on the host). Tests: nexus-init 6, riscv clean, no regressions (nexus-ipc 25 / nexus-abi 25 / bundlemgrd 13 / abilitymgr 21 / windowd 120). Remaining P2: samgrd-brokered `Connection::connect`.
**RFC-0066 Phase 3 started (data-driven orchestrator, incremental + safe)**: added `service_topology::spec_for`/`exposes_server` (host-tested) + a generic `provision_server_endpoint` helper + a guarded orchestrator arm — services whose `ServiceSpec.exposes_server` is true and that aren't `is_bespoke_wired` get their server endpoint **provisioned from the declarative spec**, not a hand-written arm. abilitymgr is the first migrated (its bespoke arm removed; behavior boot-identical — same `init: abilitymgr slots recv=0x3 send=0x4`). Bespoke services migrate off `is_bespoke_wired` incrementally → the 1500-line match collapses, the "forgot to wire a service" crash becomes structurally impossible. nexus-init 7 host tests, riscv clean, no regressions (nexus-ipc 25 / abilitymgr 21 / windowd 120 / bundlemgrd 13). Boot-verify: abilitymgr still `ready` + stays up (unchanged).
**RFC-0066 convergence — abilitymgr→bundlemgrd resolve wired end-to-end (data-driven route + CAP_MOVE):** the orchestrator now provisions abilitymgr's outbound route **from `ServiceSpec.routes_to`** (reply inbox + send cap to bundlemgrd's request endpoint → existing `chan.bnd_send_slot`/`reply_*` fields → existing `build_route_table` registration), best-effort. abilitymgr's startup probe replaced the dead `KernelClient` path with the real CAP_MOVE request/reply (`route_blocking(bundlemgrd)` + `@reply` inbox + `cap_move`) — the proven rngd→policyd mechanics. riscv clean, host 21/7/25/13/120 green. **Boot-verify next run:** `init: abilitymgr route->bundlemgrd ok` + `abilitymgr: registry ok (n=3)` + `bundlemgrd: list_apps.ok` (was `registry unreachable`). This is the first real cross-service hop over the production bahn (declarative route + bounded CAP_MOVE), not copy-paste.
**Capability gating wired (policyd, boot-safe):** new reusable `nexus_ipc::policyd` client — one host-tested delegated capability check (`check_cap_delegated`, `CapDecision::Allow/Deny/Unreachable`) replacing per-service copy-pasted CAP_MOVE. bundlemgrd's authorization now = static allowlist fast-path **OR** `policyd.check_cap_delegated(sender, "bundle.query")` Allow — boot-safe (known services pass without policyd; capabilities become real as policyd grants more). nexus-ipc 28 host tests, bundlemgrd 13, riscv clean. **Capability namespace SSOT + direct error markers (done):** `nexus_ipc::capabilities::Capability` — typed SSOT for every gated capability name (`rng.entropy`/`bundle.query`/`app.launch`/`process.spawn`/`window.surface`); a new cap = an enum variant (compile-enforced everywhere, host-tested naming/uniqueness/round-trip) → a cap can never be typo'd or forgotten again. `nexus_ipc::policyd::authorize(subject, Capability, enforcer)` emits a **direct greppable error** on denial: `!cap-deny: enforcer=<x> cap=<name> subject=0x<id>` — a caps/policyd failure surfaces immediately, no hunting. bundlemgrd migrated onto it. nexus-ipc 31 host tests, riscv clean, no regressions. **Allowlist/copy-paste consolidation started:** survey found keystored/statefsd already policy-gate correctly — the chaos is the **copy-pasted ~90-line `policyd_allows`** per service (each with its own fixed init-wired slots), not allowlists. Extracted the shared CAP_MOVE check as `nexus_ipc::policyd::check_cap_on(send_slot, reply_send, reply_recv, subject, cap)` (each service keeps its slots; `check_cap_delegated` now routes + calls it). **statefsd migrated** off its hand-rolled copy → `check_cap_on(0x07,0x06,0x05,…)`, ~106 lines deleted (incl. its dup `decode_delegated_cap_decision` + tests, now the shared host-tested `decode_decision`). Behaviour-preserving (same slots/logic). riscv clean, no warnings, host green (statefsd 2 / nexus-ipc 31 / bundlemgrd 13). **Copy-pasted `policyd_allows` fully eliminated:** statefsd + keystored (fixed slots → `check_cap_on`) and rngd (dynamic route → `check_cap_delegated`) all migrated to the shared `nexus_ipc::policyd` helper — **~330 lines of duplicate CAP_MOVE boilerplate deleted**, plus 3 duplicate `decode_delegated_cap_decision` fns + their tests (now the shared host-tested `decode_decision`). Behaviour-preserving (each kept its own slots/routing). All green: rngd 12 / keystored 21 / statefsd 2 / bundlemgrd 13 / nexus-ipc 31 / abilitymgr 21, riscv clean. The policy-check chaos ("many copies") is now ONE host-tested path. **ROOT CAUSE of `abilitymgr: registry unreachable` found + fixed (declarative):** the responder gates every route via policyd `ipc.core`; **abilitymgr + windowd were entirely missing from the policy SSOT** `policies/base.toml` `[allow]` → their route requests were policy-denied → silent "unreachable". Fixed: added `abilitymgr = ["ipc.core","bundle.query","proc.spawn"]` + `windowd = ["ipc.core","bundle.query"]`. **Better errors for the future (user ask):** (1) responder now emits a greppable `!route-deny: <from> -> <to> (policy: missing ipc.core grant?)` instead of a silent denial; (2) a host test `routing_services_are_granted_ipc_core_in_policy` cross-validates `service_topology` ↔ `base.toml` — a routing service without its grant is now a `cargo test` failure, not a boot hunt. policyd rebuilds clean (25 tests), nexus-init 8, riscv clean. **BOOT-CONFIRMED (run 10:57): `abilitymgr: registry ok (n=3)`** — the cross-service chain is LIVE: nxb manifest → bundlemgrd registry → abilitymgr resolve, over the production bahn (declarative policy + route + bounded CAP_MOVE). n=3 = chat/search/notes from the manifests. **The new `!route-deny` marker immediately earned its keep**: surfaced a previously-silent latent bug — `timed -> metricsd` denied (timed was also missing from base.toml) → fixed (`timed = ["ipc.core"]`). policyd 25 / nexus-init 8 (incl. cross-SSOT guard) green, riscv clean. Now windowd→bundlemgrd works over the same proven bahn → ready for the dynamic Apps menu. Remaining: logd/execd allowlists; manifest `required_caps` at launch; (optional) de-dup the `!route-deny` marker to one line per pair.
Current contract status: `RFC-0058` is `Done`; `RFC-0059` is `Complete`; `RFC-0063` is `Complete`; `RFC-0064` is `Complete`; `RFC-0065` is `Done` (TASK-0065 closed 2026-06-23; spawned-process/per-app-surface runtime → TASK-0080D + SystemUI DSL phases); `TASK-0064` WM v1 proven (Crossfade/Slide deferred to TASK-0064B).
Service-split note (ADR-0036): the v6b lifecycle broker is **`abilitymgr`** (not a new `appmgrd`); the app registry is **`bundlemgrd`** (gains `enumerate`); chat + search become **real apps**.

**IN PROGRESS (started 2026-07-03): `TASK-0070` + `TASK-0072` combined WM/settings track.** Both task files rewritten to IST + new scope (the old drafts assumed v7a snap zones that never landed and planned keyboard snap shortcuts — rejected; `prefsd` dropped for extending the existing `settingsd` per the `TASK-0225` direction). W-track (`TASK-0070`, phases 1–7): window collection + z-order/focus wired onto the existing-but-unused `window_scene::composition_order()` SSOT, header buttons (– □ ×) + minimize dock + fullscreen-over-chrome, drag-to-edge snap + edge resize + vendored resize-cursor shapes, backdrop correctness (glass/corners composite over real underlying content via destination-so-far sampling; the GL backend lands its real gaussian backdrop pass), SVG conflation-seam fix (coverage accumulation), runtime text via build-time A8 glyph atlases of the vendored UI face, scroll unification (`scroll_id` per-layer GPU offset + layout→List→VirtualList everywhere). S-track (`TASK-0072`, phases 8–10): settingsd typed registry persisted via statefsd, dual light/dark theme runtime, Options menu → settings panel window with a working light/dark switch end-to-end. Every phase boot-gated (host tests + riscv + headless diff + user gtk boot + user commit).

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
| TASK-0056B | visible input v0 (cursor/hover/focus/click) |
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

Current RFC closure status: `RFC-0060` (formerly RFC-0033), `RFC-0034`, `RFC-0035`, `RFC-0036`, and `RFC-0037` are `Done/Complete`.

---

## TRACK Index (Complete List)

| Track | File |
|-------|------|
| TRACK-ADS-SAFETY-FAMILYMODE | `tasks/TRACK-ADS-SAFETY-FAMILYMODE.md` |
| TRACK-APP-STORE | `tasks/TRACK-APP-STORE.md` |
| TRACK-ARCADE-APP | `tasks/TRACK-ARCADE-APP.md` |
| TRACK-AUTHORITY-NAMING | `tasks/TRACK-AUTHORITY-NAMING.md` |
| TRACK-CONSOLE-AND-TOOLCHAINS | `tasks/TRACK-CONSOLE-AND-TOOLCHAINS.md` |
| TRACK-CORE-UTILITIES | `tasks/TRACK-CORE-UTILITIES.md` |
| TRACK-CREATIVE-APPS | `tasks/TRACK-CREATIVE-APPS.md` |
| TRACK-DAW-APP | `tasks/TRACK-DAW-APP.md` |
| TRACK-DEVELOPER-EXPERIENCE-SURFACES | `tasks/TRACK-DEVELOPER-EXPERIENCE-SURFACES.md` |
| TRACK-DEVSTUDIO-IDE | `tasks/TRACK-DEVSTUDIO-IDE.md` |
| TRACK-DRIVERS-ACCELERATORS | `tasks/TRACK-DRIVERS-ACCELERATORS.md` |
| TRACK-DSL-V1-DEVX | `tasks/TRACK-DSL-V1-DEVX.md` |
| TRACK-FEEDS-APP | `tasks/TRACK-FEEDS-APP.md` |
| TRACK-GROUP-AND-DEVICE-MANAGEMENT | `tasks/TRACK-GROUP-AND-DEVICE-MANAGEMENT.md` |
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
| TRACK-OPEN-POINTS-2026-07 | `tasks/TRACK-OPEN-POINTS-2026-07.md` |
| TRACK-OS-PROOF-INFRASTRUCTURE | `tasks/TRACK-OS-PROOF-INFRASTRUCTURE.md` |
| TRACK-PASSWORD-MANAGER | `tasks/TRACK-PASSWORD-MANAGER.md` |
| TRACK-PIM-SUITE | `tasks/TRACK-PIM-SUITE.md` |
| TRACK-PINBALL-APP | `tasks/TRACK-PINBALL-APP.md` |
| TRACK-PODCASTS-APP | `tasks/TRACK-PODCASTS-APP.md` |
| TRACK-PRODUCTION-GATES-KERNEL-SERVICES | `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` |
| TRACK-PUZZLE-APP | `tasks/TRACK-PUZZLE-APP.md` |
| TRACK-RECIPES-APP | `tasks/TRACK-RECIPES-APP.md` |
| TRACK-REFERENCE-GAMES | `tasks/TRACK-REFERENCE-GAMES.md` |
| TRACK-REMOVABLE-STORAGE | `tasks/TRACK-REMOVABLE-STORAGE.md` |
| TRACK-REPO-HYGIENE-FOLLOWUPS | `tasks/TRACK-REPO-HYGIENE-FOLLOWUPS.md` |
| TRACK-SCORE-APP | `tasks/TRACK-SCORE-APP.md` |
| TRACK-SETTINGS-FAMILY-MODE | `tasks/TRACK-SETTINGS-FAMILY-MODE.md` |
| TRACK-STASH-USER-DATA-FS | `tasks/TRACK-STASH-USER-DATA-FS.md` |
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
