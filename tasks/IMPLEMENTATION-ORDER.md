# Implementation Order: Sequential by Task Number

This file provides a **sequential execution view** over `tasks/TASK-*.md`.

**Primary rule**: Tasks are executed in **numerical order** (TASK-0001, TASK-0002, ...).

This file is **not authoritative** for scope/DoD; each `TASK-*.md` remains execution truth.

For Kanban-style status view, see: `tasks/STATUS-BOARD.md`.

---

## How Tasks and TRACKs Work Together

### Tasks (`TASK-XXXX-*.md`)

- **Atomic work units** with clear stop conditions (Definition of Done)
- Executed in **numerical order**
- Each task has proofs (host tests, QEMU markers)
- Status: `Draft` → `In Progress` → `In Review` → `Done`

### Tracks (`TRACK-*.md`)

- **Vision documents** that describe a larger feature area or product direction
- TRACKs are **not executed directly** — they spawn tasks
- A TRACK contains:
  - High-level goals and constraints
  - Candidate tasks (`CAND-*`) to be extracted into real `TASK-XXXX` files
  - Gates (RED/YELLOW/GREEN) that block extraction
  - Phase map showing progression

**Workflow**:
1. TRACKs define **what** we want to build (vision + constraints)
2. When a TRACK's gates are satisfied, extract a `CAND-*` into a real `TASK-XXXX`
3. The new task gets the next available number and enters the sequential queue
4. Execute tasks in numerical order

**Example**:
- `TRACK-DRIVERS-ACCELERATORS.md` defines the GPU/NPU/VPU vision
- Once `TASK-0010` (MMIO) and `TASK-0031` (VMO) are done, `CAND-DRV-000` can become `TASK-0280`
- `TASK-0280` then executes in its numerical position

---

## In Progress

| Task | Title | Status |
|------|-------|--------|
| — | — | — |

---

## In Review

| Task | Title | Status |
|------|-------|--------|
| — | — | — |

---

## Done

| Task | Title | Completed |
|------|-------|-----------|
| ✅ TASK-0001 | Runtime roles & boundaries | — |
| ✅ TASK-0002 | Userspace VFS proof | — |
| ✅ TASK-0003 | Networking: virtio-net + smoltcp + dsoftbusd OS | — |
| ✅ TASK-0003B | DSoftBus Noise XK OS | — |
| ✅ TASK-0003C | DSoftBus UDP discovery OS | — |
| ✅ TASK-0004 | Networking: DHCP/ICMP + dual-node identity | — |
| ✅ TASK-0005 | Cross-VM DSoftBus + remote proxy | — |
| ✅ TASK-0006 | Observability v1: logd journal + crash reports | — |
| ✅ TASK-0007 | Updates & Packaging v1.0: A/B skeleton | — |
| ✅ TASK-0008 | Security hardening v1: policy engine + audit trail | 2026-01-25 |
| ✅ TASK-0008B | Device identity keys v1 (virtio-rng + rngd + keystored keygen) | — |
| ✅ TASK-0009 | Persistence v1 (virtio-blk + statefs) | — |
| ✅ TASK-0010 | Device MMIO access model | — |
| ✅ TASK-0011 | Kernel simplification phase A | — |
| ✅ TASK-0011B | Kernel Rust idioms pre-SMP | — |
| ✅ TASK-0012 | Kernel SMP v1 (per-CPU runqueues + IPIs) | — |
| ✅ TASK-0012B | Kernel SMP v1b hardening bridge (scheduler + SMP internals) | — |
| ✅ TASK-0013 | Perf/Power v1: QoS ABI + timed coalescing | — |
| ✅ TASK-0013B | IPC liveness hardening v1: bounded retry/correlation | — |
| ✅ TASK-0014 | Observability v2: metrics + tracing | — |
| ✅ TASK-0015 | DSoftBusd refactor v1: modular OS daemon structure | — |
| ✅ TASK-0016 | DSoftBus Remote-FS v1: Remote PackageFS proxy (read-only) | — |
| ✅ TASK-0016B | Netstackd refactor v1: modular OS daemon structure + loop/idiom hardening | — |
| ✅ TASK-0017 | DSoftBus Remote-StateFS v1 | — |
| ✅ TASK-0018 | Crashdumps v1: minidump + host symbolization | — |
| ✅ TASK-0019 | Security v2 (OS): userland ABI syscall guardrails | — |
| ✅ TASK-0020 | DSoftBus Streams v2: multiplexing + flow control + keepalive | — |
| ✅ TASK-0021 | DSoftBus QUIC v1: host QUIC transport + OS UDP scaffold + TCP fallback | — |
| ✅ TASK-0022 | DSoftBus core refactor: no_std-compatible core + transport abstraction | — |
| ✅ TASK-0023 | DSoftBus QUIC v2 OS enablement (session path closure) | — |
| ✅ TASK-0023B | Selftest-client production-grade deterministic test architecture refactor v1 | 2026-04-20 |
| ✅ TASK-0029 | Supply-Chain v1: SBOM + repro metadata + signature allowlist policy | 2026-04-22 |
| ✅ TASK-0031 | Zero-copy VMOs v1: shared RO buffers + handle transfer | 2026-04-23 |
| ✅ TASK-0032 | PackageFS v2: RO image index + fastpath | 2026-04-23 |
| ✅ TASK-0039 | Sandboxing v1: VFS namespaces + CapFd + manifest permissions | 2026-04-24 |
| ✅ TASK-0045 | DevX nx-cli v1 | 2026-04-24 |
| ✅ TASK-0046 | Config v1: configd + JSON Schema + layering + 2PC reload | 2026-04-26 |
| ✅ TASK-0047 | Policy as Code v1: unified policy engine | 2026-04-26 |
| ✅ TASK-0054 | UI v1a: BGRA8888 CPU renderer + damage tracking + headless snapshots | 2026-04-27 |
| ✅ TASK-0055 | UI v1b: windowd compositor + surfaces/layers IPC + VMO buffers + vsync | 2026-04-27 |

---

## UI Fast Lane — Ziel: 119–122C

Statt aller Tasks 24–118 sequenziell werden nur die für die UI-Kette notwendigen Tasks abgearbeitet.
Alle anderen Tasks kommen nach 122C in den **Defer-Bucket** und werden danach ergänzt.

**Gesamtumfang Fast Lane: ~40 Tasks statt ~98.**

---

### Schritt 1 — Fundamente (Pre-54)

Minimale Voraussetzungen für den UI-Stack. Alles andere aus dem 24–53 Bereich wird übersprungen.

| Task | Title | Warum nötig |
|------|-------|-------------|
| TASK-0029 | Supply Chain v1: SBOM + repro metadata + signature allowlist | Harte Dep von TASK-0031 (VMOs); host closure + docs sync landed, QEMU supply-chain marker profile verified |
| TASK-0031 | Zero-copy VMOs v1: shared RO buffers + handle transfer | Kritisch: VMO-backed Surfaces für windowd-Compositor |
| TASK-0032 | PackageFS v2: RO image index + fastpath | App-Asset-Laden für Launcher |
| TASK-0039 | Sandboxing v1: VFS namespaces + CapFd + manifest permissions | App-Isolation |
| TASK-0045 | DevX nx-cli v1 | `nx dsl build/lint/fmt` für DSL-Workflow |
| TASK-0046 | Config v1: configd + JSON Schema + layering + 2PC reload | UI-Profil-Broker für windowd + input |
| TASK-0047 | Policy as Code v1: unified policy engine | Asset-Zugriff + Permissions für UI-Services |

**Übersprungen (24–53):** `0024` (DSoftBus UDP sec), `0025–0027` (StateFS hardening/encryption), `0028` (ABI filters v2), `0030` (DSoftBus discovery authz), `0033` (PackageFS VMO-splice), `0034–0037` (OTA/delta updates), `0038` (Tracing v2), `0040` (Remote observability), `0041` (Lock profiling), `0042` (SMP v2 voll — minimaler QoS-Slice kommt via 0054B), `0043–0044` (Security sandbox quotas / QUIC tuning), `0048–0053` (Crashdump v2 / Recovery / Security v3).

---

### Schritt 2 — Sichtbare UI (54–56B)

Vom CPU-Renderer bis zum ersten echten sichtbaren Input in QEMU.

| Task | Title |
|------|-------|
| TASK-0054 | UI v1a: BGRA8888 CPU renderer + damage tracking + headless snapshots (Done; host renderer/snapshot proof floor green) |
| TASK-0055 | UI v1b: windowd compositor + surfaces/layers IPC + VMO buffers + vsync (Done; headless present + generated IDL roundtrip + reject proofs green) |
| TASK-0055B | UI v1c: visible QEMU scanout bootstrap (simplefb window + first visible frame) |
| TASK-0055C | UI v1d: windowd visible present + SystemUI first frame in QEMU |
| TASK-0056 | UI v2a: double-buffered surfaces + present scheduler + input routing |
| TASK-0056B | UI v2a: visible input — cursor + focus + click |

**Defer aus diesem Bereich:** `0054B/C/D` (Kernel UI/IPC/MM perf floor — Perf-Polish nach Baseline), `0055D` (dev display presets), `0056C` (present/input perf latency).

---

### Schritt 3 — UI-Inhalt (57–65)

Text, Layout, Gesten, Animation, Window Management, App-Lifecycle.

| Task | Title |
|------|-------|
| TASK-0057 | UI v2b: text shaping (HarfBuzz) + font fallback/cache + SVG pipeline |
| TASK-0058 | UI v3a: layout wrapping + deterministic box model |
| TASK-0059 | UI v3b: clip/scroll/effects + IME/TextInput |
| TASK-0061 | UI v4b: gestures + a11y semantics |
| TASK-0062 | UI v5a: reactive runtime + animation/transitions |
| TASK-0063 | UI v5b: virtualized list + theme tokens |
| TASK-0064 | UI v6a: window management + scene transitions |
| TASK-0065 | UI v6b: app lifecycle + notifications + navigation |

**Defer aus diesem Bereich:** `0060` (tiled compositor/clip-stack/atlases), `0060B` (glass/backdrop-cache), `0062B` (animation frame-budget perf-scenes), `0066` (WM split/snap).

---

### Schritt 4 — Shell-Infra (70–74)

WM-Overlays, Settings-Panel, Design System, App Shell.

| Task | Title |
|------|-------|
| TASK-0070 | UI v8b: WM resize/move + shortcuts + settings overlays |
| TASK-0072 | UI v9b: prefsd + settings panels + quick settings |
| TASK-0073 | UI v10a: design system primitives + goldens |
| TASK-0074 | UI v10b: app shell adoption + modals |

**Defer aus diesem Bereich:** `0067` (DnD/clipboard v2 — kommt via 0122C), `0068` (screenshot/share), `0069` (notifications v2 advanced), `0071` (searchd/command palette).

---

### Schritt 5 — DSL-Fundament (75–80C)

Vollständige DSL-Kette: Lexer → Interpreter → AOT → State/Nav → Bootstrap-Shell. Voraussetzung für 119+.

| Task | Title |
|------|-------|
| TASK-0075 | DSL v0.1a: lexer/parser → AST + Scene-IR + lowering + nx dsl CLI |
| TASK-0076 | DSL v0.1b: interpreter + snapshots + OS demo hook |
| TASK-0076B | DSL v0.1c: visible OS mount + first DSL frame in windowd/SystemUI |
| TASK-0077 | DSL v0.2a: state/nav/i18n core |
| TASK-0078 | DSL v0.2b: service stubs + CLI demo |
| TASK-0078B | DSL v0.2b: QuerySpec v1 foundation (paging + hash) |
| TASK-0079 | DSL v0.3a: AOT codegen + incremental assets |
| TASK-0080B | SystemUI DSL bootstrap shell (host-first): desktop bg + launcher + app launch |
| TASK-0080C | SystemUI DSL bootstrap shell: OS-wiring + QEMU markers |

**Defer aus diesem Bereich:** `0077B` (DSL DevX ergonomics), `0080` (DSL v0.3b perf-bench/OS AOT demo).

---

### Schritt 6 — SystemUI DSL Migration (119–122C)

| Task | Title |
|------|-------|
| TASK-0119 | SystemUI→DSL Phase 1a: Launcher + Quick Settings DSL pages (host) |
| TASK-0120 | SystemUI→DSL Phase 1b: OS wiring + postflight markers |
| TASK-0121 | SystemUI→DSL Phase 2a: Settings + Notifications Center (host) |
| TASK-0122 | SystemUI→DSL Phase 2b: OS wiring + feature flags + selftests + docs |
| TASK-0122B | DSL App Platform v1: shared app shell + launch/open contract + host proofs |
| TASK-0122C | DSL App Integration Kit v1: picker + clipboard + share + print bridges |

---

## Defer-Bucket (nach 122C ergänzen)

Tasks die für den UI-Fast-Lane-Pfad nicht nötig sind, aber danach folgen:

**DSoftBus / Networking:**
`0024`, `0030`, `0044` (DSoftBus follow-ons)

**StateFS / Storage:**
`0025`, `0026`, `0027` (StateFS hardening + encryption)

**Security / Compliance:**
`0028`, `0043`, `0052`, `0053` (ABI filters, sandbox quotas, ingress policy, signed recovery)

**OTA / Updates / Supply Chain:**
`0033`, `0034`, `0035`, `0036`, `0037` (PackageFS VMO-splice, delta updates, OTA v2)

**Observability / Debug:**
`0038`, `0040`, `0041`, `0048`, `0049` (Tracing v2, remote observability, lock profiling, crashdump v2)

**Recovery:**
`0050`, `0051` (Recovery v1a/v1b)

**SMP v2 (voll):**
`0042` (nur minimaler QoS-Slice via TASK-0054B vorgezogen)

**UI Perf-Polish:**
`0054B`, `0054C`, `0054D`, `0055D`, `0056C`, `0060`, `0060B`, `0062B`

**Advanced UI Features:**
`0066`, `0067`, `0068`, `0069`, `0071`, `0077B`, `0080`

**Apps + Plattform (81–118):**
`0081–0118` (MIME registry, browser, kamera, office apps etc. — nach DSL App Platform 0122B/C)

---

## Active TRACKs (spawn tasks when gates clear)

| Track | Purpose | Blocked by |
|-------|---------|------------|
| TRACK-DRIVERS-ACCELERATORS | GPU/NPU/VPU device-class services | TASK-0010, TASK-0031, TASK-0012B |
| TRACK-NETWORKING-DRIVERS | NIC drivers, offload, netdevd | TASK-0003, TASK-0010, TASK-0012B |
| TRACK-NEXUSGFX-SDK | Graphics SDK for apps | UI tasks (0054+) |
| TRACK-NEXUSINFER-SDK | On-device ML runtime (CPU ref + future NPU), hybrid IPC | TASK-0031, TASK-0010, TASK-0280 |
| TRACK-NEXUSMEDIA-SDK | Audio/video/image SDK | UI tasks, codec tasks |
| TRACK-ZEROCOPY-APP-PLATFORM | RichContent + OpLog + connectors | TASK-0031, TASK-0087 |
| TRACK-APP-STORE | Distribution + publishing | Packaging tasks |
| TRACK-DEVSTUDIO-IDE | Developer IDE | DSL tasks (0075+) |

---

## Rules

1. **Sequential by number**: Execute TASK-XXXX in order (0001, 0002, 0003, ...)
2. **Skip if blocked**: If a task has unsatisfied prereqs, note it and move to the next
3. **TRACKs don't execute**: TRACKs spawn tasks; the spawned task gets the next number
4. **100% rule**: Only mark a task Done when all stop conditions are met
5. **No fake success**: Markers/proofs must reflect real behavior

---

## Related

- **Status board (Kanban view)**: `tasks/STATUS-BOARD.md`
- **Task workflow rules**: `tasks/README.md`
- **RFC process**: `docs/rfcs/README.md`
