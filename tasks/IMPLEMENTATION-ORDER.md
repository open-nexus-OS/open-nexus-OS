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
| ✅ TASK-0055B | UI v1c: visible QEMU scanout bootstrap | 2026-04-29 |
| ✅ TASK-0055C | UI v1d: windowd visible present + SystemUI first frame in QEMU | 2026-04-30 |
| ✅ TASK-0056 | UI v2a: double-buffered surfaces + present scheduler + input routing | 2026-04-30 |
| ✅ TASK-0056B | UI v2a: visible input — cursor + hover + focus + click | 2026-05-03 |
| ✅ TASK-0252 | Input v1.0a: host HID/touch/keymaps/repeat/pointer-accel core | 2026-05-04 |
| ✅ TASK-0253 | Input v1.0b: OS/QEMU hidrawd + touchd + inputd + windowd/IME hooks | 2026-05-11 |
| ✅ TASK-0056C | UI v2a: present/input perf latency + coalescing + no-damage-skip + idle-cheap | 2026-05-11 |
| ✅ TASK-0057 | UI v2b: Minimal DisplayServer v0 — Mocu cursor, JPEG wallpaper, Inter text, input targets | 2026-05-15 |
| ✅ TASK-0058 | UI v3a: layout engine (flex/grid/stack) + text wrapping + host goldens — production-grade windowd integration | 2026-05-17 |
| ✅ TASK-0059 | UI v3b: clip + scroll + backdrop effects + shadow pipeline + IME + MSDF/SDF rendering | 2026-06-05 |
| ✅ TASK-0062 | UI v5a: Deterministic Animation + NexusGfx 2D Pipeline + GPU Driver Contract | 2026-06-10 |
| ✅ TASK-0063 | UI v5b: virtualized list + scene graph + dual-panel GPU blur + virgl pipeline + theme tokens | 2026-06-22 |
| ✅ TASK-0064 | UI v6a: window management v1 — ShellWindow N-window WM (chat instance + title-bar/X/drag/z-order) | 2026-06-22 |
| ✅ TASK-0057 | UI v2b: text shaping + font fallback/cache + SVG pipeline | 2026-07-19 (reconciled; ui v2b markers) |
| ✅ TASK-0058 | UI v3a: layout wrapping + deterministic box model | 2026-07-19 (reconciled; host goldens) |
| ✅ TASK-0060 | UI v4a: tiled compositor + clip-stack + atlases + perf | 2026-07-19 (reconciled; ui v4 markers) |
| ✅ TASK-0060B | UI v4b: glass materials + backdrop-cache + degrade | 2026-07-19 (reconciled) |
| ✅ TASK-0061 | UI v4b: gestures + a11y semantics (a11y-hardening folded → TASK-0114) | 2026-07-19 (reconciled) |
| ✅ TASK-0062B | UI v5a: animation frame-budget + perf scenes | 2026-07-19 (reconciled) |
| ✅ TASK-0065 | UI v6b: app lifecycle + navigation (notifications folded → TASK-0123–0125) | 2026-07-19 (reconciled) |
| ✅ TASK-0065B | Session/Login v0: greeter/dev-session + SystemUI shell handoff | 2026-07-19 (reconciled; greeter boot-proven) |
| ✅ TASK-0075 | DSL v0.1a: lexer/parser → AST + Scene-IR + lowering + nx dsl CLI | 2026-07-19 (reconciled; dsl_conformance) |
| ✅ TASK-0076 | DSL v0.1b: interpreter + snapshots + OS demo hook | 2026-07-19 (reconciled; dsl_goldens) |
| ✅ TASK-0077 | DSL v0.2a: state/nav/i18n core | 2026-07-19 (reconciled) |
| ✅ TASK-0078 | DSL v0.2b: service stubs + CLI demo | 2026-07-19 (reconciled) |
| ✅ TASK-0080 | DSL v0.3b: perf-bench + OS AOT demo | 2026-07-19 (reconciled) |
| ✅ TASK-0080B | SystemUI DSL bootstrap shell (host-first): bg + launcher + app launch | 2026-07-19 (reconciled; bootstrap host test) |
| ✅ TASK-0080C | SystemUI DSL bootstrap shell: OS-wiring + QEMU markers | 2026-07-19 (reconciled) |
| ✅ TASK-0080D | DSL app runtime lifecycle + surface contract | 2026-07-19 (reconciled) |
| ✅ TASK-0130 | Packages v1b: bundlemgrd install/upgrade/uninstall + trust policy | 2026-07-19 (reconciled; bundlemgrd markers) |
| ✅ TASK-0269 | Boot gates v1: readiness + spawn-reason + resource sentinel | 2026-07-19 (reconciled; kselftest markers) |
| ✅ TASK-0070 | UI v8b: WM resize/move/snap/dock (shortcuts = Non-Goal; overlays → 0072) | 2026-07-19 (reconciled; wm.rs/snap.rs/dock.rs + 23 tests) |
| ✅ TASK-0072 | UI v9b: settingsd + settings panel DSL app (prefsd→settingsd; quick-settings dropped) | 2026-07-19 (reconciled; settingsd markers + settings.rs test) |
| ✅ TASK-0073 | UI v10a: design-system primitives + goldens | 2026-07-19 (reconciled; 37 widgets + 74 goldens) |
| ✅ TASK-0078B | DSL v0.2b: QuerySpec v1 (paging + hash) | 2026-07-19 (reconciled; nexus-query/queryd + paging tests; boot-wiring = Non-Goal) |
| ✅ TASK-0119 | SystemUI→DSL Phase 1a: Launcher + Control-Center DSL pages | 2026-07-19 (reconciled; dsl_apps_conformance) |
| ✅ TASK-0120 | SystemUI→DSL Phase 1b: OS wiring | 2026-07-19 (reconciled; `systemui: dsl shell on`) |
| ✅ TASK-0121 | SystemUI→DSL Phase 2a: Settings + Notifications Center surface (notif delivery → 0123–0125) | 2026-07-19 (reconciled; settings.rs test) |

---

## Post-0064 — SMP + Filesystem (built after the UI Fast Lane)

These tracks were executed after the Fast-Lane cut and are boot-/host-proven.

### SMP / parallelism (kernel)

| Task | Title | Completed |
|------|-------|-----------|
| ✅ TASK-0012 | Kernel SMP v1 (per-CPU runqueues + IPIs) | (earlier) |
| ✅ TASK-0012B | Kernel SMP v1b hardening bridge (scheduler + SMP internals) | (earlier) |
| ✅ TASK-0042 | SMP v2: affinity + QoS budgets + kernel ABI | 2026-07-19 (kselftest smp/bkl markers) |
| ✅ TASK-0276 | Parallelism v1: deterministic threadpools + policy contract | 2026-07-19 |
| ✅ TASK-0277 | Kernel SMP parallelism policy v1 (deterministic) | 2026-07-19 |
| ✅ TASK-0283 | Kernel per-CPU ownership wrapper v1 | 2026-07-19 |
| ✅ TASK-0288 | Kernel runtime closure v1c: latency budgets + stress proofs | 2026-07-19 |

**Still open (SMP closure / release-blockers per STATUS-BOARD):** `0281`, `0282`, `0286`, `0287`, `0290`.

### Filesystem (nxfs / stash user-data)

| Task | Title | Completed |
|------|-------|-----------|
| ✅ TASK-0291 | VFS ReadDir + svc.files + filemanager role + stash real listing | 2026-07-19 (boot-proven) |
| ✅ TASK-0292 | nxfs v1 core (host-first): engine + fsck + crash-injection | 2026-07-19 (host-proven, 17 tests) |
| ✅ TASK-0293 | nxfs /data OS bring-up (2nd blk device + vfsd DataStore) | 2026-07-19 (write + cold-boot persistence boot-proven) |
| ✅ TASK-0294 | MIME SSOT: nexus-mime-icons + stash filetype icons | 2026-07-19 (39-icon SSOT boot-proven) |
| ✅ TASK-0295 | Zero-copy read/write via VMO splice (OP_READ_VMO CAP_MOVE) | 2026-07-19 (boot-proven) |

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

### Schritt 2 — Sichtbare UI + Input-Spine (54–56B, 252–253, 56C)

Vom CPU-Renderer bis zum sichtbaren deterministischen Input-Proof und dann direkt zur echten Input-Architektur.

| Task | Title |
|------|-------|
| TASK-0054 | UI v1a: BGRA8888 CPU renderer + damage tracking + headless snapshots (Done; host renderer/snapshot proof floor green) |
| TASK-0055 | UI v1b: windowd compositor + surfaces/layers IPC + VMO buffers + vsync (Done; headless present + generated IDL roundtrip + reject proofs green) |
| TASK-0055B | UI v1c: visible QEMU scanout bootstrap (Done; marker-honesty hardening + full closure gates green) |
| TASK-0055C | UI v1d: windowd visible present + SystemUI first frame in QEMU (Done; composed-frame visible-present proof + closure gates green) |
| TASK-0056 | UI v2a: double-buffered surfaces + present scheduler + input routing (Done; host/reject/QEMU proofs + fmt/clippy/ci-network + make clean/build/test/run green) |
| TASK-0056B | UI v2a: visible input — cursor + hover + focus + click (Done; deterministic host/reject/QEMU proofs + closure gates green; live device input follows in 0252/0253) |
| TASK-0252 | Input v1.0a: host HID/touch/keymaps/repeat/pointer-accel core (Done; host-first contract closed with full gate reruns green) |
| TASK-0253 | Input v1.0b: OS/QEMU hidrawd + touchd + inputd + windowd/IME hooks (Done; live QEMU pointer/keyboard floor, full closure gates green) |
| ✅ TASK-0056C | UI v2a: embedded reactor/runtime floor + present/input perf latency + coalescing (Done; host-first coalescing + no-damage-skip + idle-cheap proofs green; QEMU marker ladder + diag-os pending downstream) |

**Defer aus diesem Bereich:** `0054B/C/D` (Kernel UI/IPC/MM perf floor — Perf-Polish nach Baseline), `0055D` (dev display presets).

**Eingebetteter Reactor/Runtime-Faden (kein separater Parallel-Track):**
`TASK-0056C` setzt das Mindestniveau fuer eine fluessige echte Desktop-UI:
demand-aware present, deterministisches Motion-Coalescing, no-damage/unchanged-state skip,
common-case caches, Kettenmetriken und ein billiger Idle-Pfad ueber `inputd` -> `fbdevd` -> `windowd`.
`TASK-0059`, `TASK-0062`, `TASK-0063` und `TASK-0064` bauen genau diesen Faden weiter aus
fuer Scroll/Clip/Damage, Runtime/Animation, Virtualisierung/Invalidation und WM/Scene-Transitions,
statt ein separates Runtime-Subsystem neben der Fast Lane aufzubauen.

### Orbital-Level UX Gate (vor 0119/0120)

`Orbital-Level` ist hier ein UX-Mindestniveau, nicht die Orbital-Architektur. Die
Architektur bleibt service-/capability-orientiert nach der Open-Nexus-Linie:
`inputd` normalisiert Events, `windowd` besitzt Hit-Test/Hover/Focus/Click,
SystemUI besitzt Shell/Launcher/Session-Flächen, Apps bekommen nur eigene Surfaces und Events.

Bevor `TASK-0119`/`TASK-0120` als Desktop-/Launcher-Qualität gelten dürfen, muss die
Fast Lane mindestens beweisen:

- sichtbarer Login/Greeter oder Dev-Session,
- live QEMU Pointer und minimales Keyboard,
- Cursor, Hover, Focus, Click, Scroll,
- Launcher/Dock/Taskbar oder äquivalente Shell-Fläche,
- App starten, App-Fenster sichtbar, Fokus/Close/Move mindestens v0,
- SVG-Quellassets für Icons/UI-Vektoren; PNG nur als Golden/Screenshot/abgeleitetes Artefakt,
- einfache Settings/Quick Settings,
- keine globalen Input-Leaks an Apps, keine Marker-only Desktop-Claims.

---

### Schritt 3 — UI-Inhalt (57–65)

Text, Layout, Gesten, Animation, Window Management, App-Lifecycle.

**Gemeinsame Visible-Proof-Surface-Regel fuer Schritt 3-5:**
Diese Tasks duerfen neue UI-Faehigkeiten nicht nur per Host-Golden, Marker oder isolierter Demo claimen.
Sie muessen in eine gemeinsame sichtbare Proof-Surface auf dem echten QEMU-/Desktop-Bildschirm einlaufen:

- Text-/Wrapping-Target mit echtem sichtbarem Text,
- SVG-/Icon-Target und sichtbarer Cursor-Asset-Pfad; sobald die SVG-Pipeline live ist, soll der Cursor auf den in
  `docs/dev/ui/foundations/visual/cursor-themes.md` beschriebenen Mocu-Cursor-Pfad umgestellt werden,
- kleines Scroll-/Clip-/Gesture-Fenster,
- Animations-/Transition-Zone,
- Virtual-List-/Datenfenster,
- Settings-/Overlay-/Modal-Flaeche,
- Launcher-/App-Window-/Shell-Flaeche,
- DSL-Seiten, die genau diese sichtbaren Targets uebernehmen statt eigene Sonder-Demos aufzubauen.

Wenn ein Task Text, SVG, Scroll, Gesten, Animation, Listen, Settings, Launcher oder DSL-UI einfuehrt,
muessen die entsprechenden Test-Targets sichtbar auf dieser Proof-Surface erscheinen und live pruefbar sein.

| Task | Title |
|------|-------|
| ✅ TASK-0057 | UI v2b: text shaping (HarfBuzz) + font fallback/cache + SVG pipeline (Done, reconciled) |
| ✅ TASK-0058 | UI v3a: layout wrapping + deterministic box model (Done, reconciled) |
| ✅ TASK-0059 | UI v3b: clip/scroll/effects + IME/TextInput (Done; ShadowArena + per-box caching + `compositor/` module refactor; IME engine folded → TASK-0146) |
| TASK-0146 | IME/Text v2 Part 1a: imed core + US/DE keymaps + deterministic host tests (pulled forward after 0059) — **still open** |
| TASK-0147 | IME/Text v2 Part 1b: OSK overlay + focus routing + OS/QEMU proofs (pulled forward after 0146) — **still open** |
| ✅ TASK-0061 | UI v4b: gestures + a11y semantics (Done; a11y-hardening folded → TASK-0114) |
| ✅ TASK-0062 | UI v5a: reactive runtime + animation/transitions (Done) |
| ✅ TASK-0063 | UI v5b: virtualized list + theme tokens (Done; scene graph + virgl GPU blur + soft-real-time pacing, boot-verified) |
| ✅ TASK-0064 | UI v6a: window management + scene transitions (Done; ShellWindow N-window WM, chat instance + drag/z-order, boot-verified; Crossfade/Slide → TASK-0064B) |
| ✅ TASK-0065 | UI v6b: app lifecycle + navigation (Done; notifications folded → TASK-0123–0125) |
| ✅ TASK-0065B | Session/Login v0: greeter/dev-session + SystemUI shell handoff (Done, boot-proven) |

**Now Done (reconciled 2026-07-19):** `0060` (tiled compositor/clip-stack/atlases), `0060B` (glass/backdrop-cache), `0062B` (animation frame-budget perf-scenes). Still deferred: `0066` (WM split/snap).

**Fast-Lane-Gap-Check vor 0119/0120:**
Nach dem aktuellen Uplift müssen vor einem ehrlichen Orbital-Level-Claim mindestens
`0252`, `0253`, `0146`, `0147`, `0065B`, `0080B` und `0080C` in der Lane bleiben.
`0060`, `0062B`, `0066`, `0067`, `0068`, `0069`, `0071`, `0077B`, `0080`,
`0095`, `0096`, `0114` und `0136` bleiben bewusst deferred: sie verbessern
Breite/Polish/Perf/Rich-Text/A11y/Policy, sind aber nicht zwingend für den
Minimalzustand Login/Launcher/Maus/Tastatur/Text/Scroll/App-Fenster. `0147`
deckt dafür nur den minimalen OSK-/Focus-/A11y-Announcement-/Policy-Hook-Floor ab.

---

### Schritt 4 — Shell-Infra (70–74)

WM-Overlays, Settings-Panel, Design System, App Shell.

| Task | Title |
|------|-------|
| ✅ TASK-0070 | UI v8b: WM resize/move/snap/dock (Done; keyboard shortcuts = Non-Goal by design; settings overlays descoped → 0072) |
| ✅ TASK-0072 | UI v9b: settingsd + settings panel DSL app (Done; prefsd replaced by settingsd; quick-settings dropped as Non-Goal) |
| ✅ TASK-0073 | UI v10a: design system primitives + goldens (Done; 37 widgets + 74 goldens + a11y lints) |
| TASK-0074 | UI v10b: app shell adoption + **modals** — **still open** (not started; no AppWindow kit, no modal/overlay widgets) |

**Defer aus diesem Bereich:** `0067` (DnD/clipboard v2 — kommt via 0122C), `0068` (screenshot/share), `0069` (notifications v2 advanced), `0071` (searchd/command palette).

---

### Schritt 5 — DSL-Fundament (75–80C)

Vollständige DSL-Kette: Lexer → Interpreter → AOT → State/Nav → Bootstrap-Shell. Voraussetzung für 119+.

| Task | Title |
|------|-------|
| ✅ TASK-0075 | DSL v0.1a: lexer/parser → AST + Scene-IR + lowering + nx dsl CLI (Done) |
| ✅ TASK-0076 | DSL v0.1b: interpreter + snapshots + OS demo hook (Done) |
| ⤳ TASK-0076B | DSL v0.1c: visible OS mount + first DSL frame — **Superseded by TASK-0080C** (own demo retired; capability lives in 0080C) |
| ✅ TASK-0077 | DSL v0.2a: state/nav/i18n core (Done) |
| ✅ TASK-0078 | DSL v0.2b: service stubs + CLI demo (Done) |
| ✅ TASK-0078B | DSL v0.2b: QuerySpec v1 foundation (paging + hash) (Done; nexus-query/queryd + tests; boot-wiring = Non-Goal) |
| TASK-0079 | DSL v0.3a: AOT codegen + incremental assets — **still open (not started; no codegen dir; interpreter-only)** |
| ✅ TASK-0080B | SystemUI DSL bootstrap shell (host-first): desktop bg + launcher + app launch (Done) |
| ✅ TASK-0080C | SystemUI DSL bootstrap shell: OS-wiring + QEMU markers (Done) |

**Now Done (reconciled 2026-07-19):** `0080` (DSL v0.3b perf-bench/OS AOT demo). Still deferred: `0077B` (DSL DevX ergonomics).

---

### Schritt 6 — SystemUI DSL Migration (119–122C)

| Task | Title |
|------|-------|
| ✅ TASK-0119 | SystemUI→DSL Phase 1a: Launcher + Control-Center DSL pages (Done; `dsl_apps_conformance`; re-arch path under `apps/desktop-shell`) |
| ✅ TASK-0120 | SystemUI→DSL Phase 1b: OS wiring (Done; `systemui: dsl shell on` boot-proven via 0080C path) |
| ✅ TASK-0121 | SystemUI→DSL Phase 2a: Settings + Notifications Center surface (Done; settings host-tested; real notif delivery folded → 0123–0125) |
| TASK-0122 | SystemUI→DSL Phase 2b: OS wiring + feature flags + selftests + docs — **still open** (depends on notifd feed) |
| TASK-0122B | DSL App Platform v1: shared app shell + launch/open contract — **still open** (no launch/open contract yet) |
| TASK-0122C | DSL App Integration Kit v1: picker + clipboard + share + print bridges — **still open** |

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
✅ `0042` Done (see "Post-0064 — SMP + Filesystem" section above); SMP closure `0281`/`0282`/`0286`/`0287`/`0290` still open.

**UI Perf-Polish:**
`0054B`, `0054C`, `0054D`, `0055D` (still deferred); ✅ `0060`, `0060B`, `0062B` now Done.

**Advanced UI Features:**
`0066`, `0067`, `0068`, `0069`, `0071`, `0077B` (still deferred); ✅ `0080` now Done.

**Apps + Plattform (81–118):**
`0081–0118` (MIME registry, browser, kamera, office apps etc. — nach DSL App Platform 0122B/C)

---

## Genuinely open themes (not started — no daemon/app/marker exists yet)

Honest reconciliation floor (2026-07-19). These are real, unimplemented feature areas — not paperwork drift:

- **IME / text input engine:** `0146`, `0147`, `0096`, `0148–0150`, `0203`, `0204` (plumbing baseline shipped in 0059; the engine itself is open)
- **Notifications service:** `0069`, `0123`, `0124`, `0125` (minimal surface shipped in 0065; notifd/DND/headsup open)
- **Search / command palette:** `0071`, `0151–0154`
- **Clipboard / share / DnD:** `0067`, `0087`, `0126–0128`
- **Content / files apps:** `0081–0093`, `0232`, `0233`
- **Media / audio:** `0099–0102`, `0155`, `0156`, `0184–0187`, `0217–0220`, `0254`, `0255`
- **Accessibility:** `0114–0118` (semantics tree shipped in 0061; a11yd hardening open)
- **Camera / privacy:** `0103–0106`, `0191`, `0192`
- **Webview:** `0111–0113`, `0176`, `0177`, `0205`, `0206`
- **Store / distribution:** `0180`, `0181`, `0221`, `0222`
- **Backup / L10n / power / sensors:** `0161`, `0162`, `0174`, `0175`, `0240`, `0241`, `0236`, `0237`, `0256–0259`, `0271`, `0272`
- **Renderer / compositor v2:** `0171`, `0199`, `0200`, `0207`, `0208`, `0215`, `0216`
- **Session / accounts / ability-lifecycle continuation** (spine Done in 0065 broker + 0065B session authority): KILL-with-reasons/backoff/crash-loop `0234`, FG/BG resource enforcement + appmgrd/samgr hooks + nx-ability CLI `0235`, lockd auto-lock/lockscreen `0109`, OOBE/Accounts app `0110`, multi-user/lockout/session-switch `0223`/`0224`, action-based delegation `0126B`, keystore v1.1 `0159`
- **Foundation partials (core done, in-title sub-deliverable open):** `0136` (policy: foreground-adapters + camera/mic perms), `0140` (updates: settings-UI page)

---

## Active TRACKs (spawn tasks when gates clear)

| Track | Purpose | Blocked by |
|-------|---------|------------|
| TRACK-DRIVERS-ACCELERATORS | GPU/NPU/VPU device-class services | TASK-0010, TASK-0031, TASK-0012B |
| TRACK-NETWORKING-DRIVERS | NIC drivers, offload, netdevd | TASK-0003, TASK-0010, TASK-0012B |
| TRACK-NEXUSGFX-SDK | Graphics SDK for apps | UI tasks (0054+) |
| TRACK-NEXUSINFER-SDK | On-device ML runtime (CPU ref + future NPU), hybrid IPC | TASK-0031, TASK-0010, TASK-0280 |
| TRACK-NEXUSMEDIA-SDK | Audio/video/image SDK | UI tasks, codec tasks |
| TRACK-STASH-USER-DATA-FS | User-data FS ladder (vfs v2 → nxfs → zero-copy → CoW/enc) + stash | Milestones 1–5 Done (0291–0295); CoW/encryption seed-when-ready; RFC-0071/0072/0073 |
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
