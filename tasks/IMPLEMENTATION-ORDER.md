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
| Updates/OTA-Lane | RFC-0089 end-state lane (0198P1 → 0036 → 0314/0260/0315 → 0289A → 0179 → 0289B → 0140 → 0034 → Phase B 0321 → 0035) — see lane section below | Packages 0–11 delivered 2026-09-01; Phase B (0321) Done 2026-09-04, 0035 Done 2026-09-05 — **OTA lane complete** |
| Sub-80 Phase 1 (ohne Netz) | 0321 → 0035 → 0028 → 0043 → 0052 — see „Sub-80 Tracking“ | Started 2026-09-03 (Ledger auf End-State umgeschrieben); Netz-Familie HOLD |

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
| ⤳ TASK-0011B | Kernel Rust idioms pre-SMP — **Superseded** (idiom scope absorbed by the SMP hardening lane) | — |
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
| ✅ TASK-0025 | StateFS v1b: authenticity envelopes + anti-rollback + write budgets | 2026-08-18 |
| ✅ TASK-0026 | StateFS v2a: 2PC crash-atomicity + bounded compaction + fsck | 2026-08-18 |
| ✅ TASK-0027 | StateFS v2b: opt-in record encryption at rest | 2026-08-18 |
| ✅ TASK-0029 | Supply-Chain v1: SBOM + repro metadata + signature allowlist policy | 2026-04-22 |
| ✅ TASK-0031 | Zero-copy VMOs v1: shared RO buffers + handle transfer | 2026-04-23 |
| ✅ TASK-0032 | PackageFS v2: RO image index + fastpath | 2026-04-23 |
| ✅ TASK-0039 | Sandboxing v1: VFS namespaces + CapFd + manifest permissions | 2026-04-24 |
| ✅ TASK-0042 | SMP v2: affinity + QoS budgets + kernel ABI | 2026-07-26 (reconciled) |
| ✅ TASK-0045 | DevX nx-cli v1 | 2026-04-24 |
| ✅ TASK-0046 | Config v1: configd + JSON Schema + layering + 2PC reload | 2026-04-26 |
| ✅ TASK-0047 | Policy as Code v1: unified policy engine | 2026-04-26 |
| ✅ TASK-0048 | Crashdump v2a: host pipeline (.nxcd container + nxsym + nx crash) | 2026-08-14 |
| ✅ TASK-0049 | Reliability v1a: fault & exhaustion truth + crash-proof reanimation | 2026-08-20 |
| ✅ TASK-0049B | Reliability v1b: service supervision v1 (tiers/restart/backoff/re-resolve) | 2026-08-20 |
| ✅ TASK-0049C | Reliability v1c: persistent evidence journal (logd → statefs spill) | 2026-08-20 |
| ✅ TASK-0050 | Reliability v1d: system reset (SBI SRST) + boot targets via bootctld | 2026-08-24 |
| ✅ TASK-0051 | Reliability v1e: recovery ops surface (fsck op + bootctld ops + nx diagnose) | 2026-08-24 |
| ✅ TASK-0051B | Reliability v1f: crash evidence at rest (on-device .nxcd + retention + redaction) | 2026-08-24 |
| ✅ TASK-0053 | Security v3: .nxra signed recovery action tokens (RFC-0088) | 2026-08-24 |
| ✅ TASK-0054 | UI v1a: BGRA8888 CPU renderer + damage tracking + headless snapshots | 2026-04-27 |
| ✅ TASK-0055 | UI v1b: windowd compositor + surfaces/layers IPC + VMO buffers + vsync | 2026-04-27 |
| ✅ TASK-0055B | UI v1c: visible QEMU scanout bootstrap | 2026-04-29 |
| ✅ TASK-0055C | UI v1d: windowd visible present + SystemUI first frame in QEMU | 2026-04-30 |
| ✅ TASK-0055D | UI v1e: dev display/profile presets for QEMU (manifest catalog + `nx ui preset` + `just start-preset`; guest ingestion → 0322) | 2026-09-03 |
| ✅ TASK-0056 | UI v2a: double-buffered surfaces + present scheduler + input routing | 2026-04-30 |
| ✅ TASK-0056B | UI v2a: visible input — cursor + hover + focus + click | 2026-05-03 |
| ✅ TASK-0252 | Input v1.0a: host HID/touch/keymaps/repeat/pointer-accel core | 2026-05-04 |
| ✅ TASK-0253 | Input v1.0b: OS/QEMU hidrawd + touchd + inputd + windowd/IME hooks | 2026-05-11 |
| ✅ TASK-0056C | UI v2a: present/input perf latency + coalescing + no-damage-skip + idle-cheap | 2026-05-11 |
| ✅ TASK-0059 | UI v3b: clip + scroll + backdrop effects + shadow pipeline + IME + MSDF/SDF rendering | 2026-06-05 |
| ✅ TASK-0062 | UI v5a: Deterministic Animation + NexusGfx 2D Pipeline + GPU Driver Contract | 2026-06-10 |
| ✅ TASK-0063 | UI v5b: virtualized list + scene graph + dual-panel GPU blur + virgl pipeline + theme tokens | 2026-06-22 |
| ✅ TASK-0064 | UI v6a: window management v1 — ShellWindow N-window WM (chat instance + title-bar/X/drag/z-order) | 2026-06-22 |
| ✅ TASK-0057 | UI v2b: asset pipeline + theme system + SVG/PNG/JPG + text shaping + cursor pipeline | 2026-07-19 (reconciled; first slice 2026-05-15) |
| ✅ TASK-0058 | UI v3a: deterministic layout engine (flex/grid/stack) + text wrapping + host goldens | 2026-07-19 (reconciled; first slice 2026-05-17) |
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
| ✅ TASK-0146 | IME v2 Part 1a: ime-core dead/compose engine + DSL focused-field model + wire codecs | 2026-07-21 |
| ✅ TASK-0147 | IME v2 Part 1b: imed service real + typing lands in apps + OSK wiring | 2026-07-22 |
| ✅ TASK-0149 | IME v2 Part 2a: JP/KR/ZH engines in ime-core (bounded user dicts) | 2026-07-22 |
| ✅ TASK-0150 | IME v2 Part 2b: candidate strip in ime-ui + CJK OSK layouts | 2026-07-22 |
| ✅ TASK-0203 | IME v2.1a: deterministic adaptive ranking (Q8.8 freq/recency) | 2026-07-24 |
| ✅ TASK-0204 | IME v2.1b: personalization store on statefsd (state:/ime) + live proof | 2026-07-24 |
| ✅ TASK-0240 | i18n v2a: locale-pack compiler in nx build + PackLocaleSource | 2026-07-21 |
| ✅ TASK-0241 | i18n v2b: runtime locale switch via OP_SURFACE_REGION push | 2026-07-21 |
| ✅ TASK-0247 | RISC-V bring-up v1.1b: SMP (SBI HSM/IPI) + per-hart timers + virtioblkd + packagefs | — |
| ✅ TASK-0276 | Parallelism v1: deterministic threadpools + policy contract | 2026-07-19 (reconciled) |
| ✅ TASK-0277 | Kernel SMP parallelism policy v1 (deterministic) | 2026-07-19 (reconciled) |
| ✅ TASK-0283 | Kernel per-CPU ownership wrapper v1 | 2026-07-19 (reconciled) |
| ✅ TASK-0285 | RFC-0014 Phase 2: QEMU harness phased failure output + phase early-exit | — |
| ✅ TASK-0288 | Kernel runtime closure v1c: latency budgets + stress proofs | 2026-07-19 (reconciled) |
| ✅ TASK-0291 | VFS ReadDir + svc.files + filemanager role + stash real listing | 2026-07-15 |
| ✅ TASK-0292 | nxfs v1 core (host-first): engine + fsck + crash-injection | 2026-07-15 |
| ✅ TASK-0293 | nxfs /data OS bring-up (2nd blk device + vfsd DataStore) | 2026-07-15 |
| ✅ TASK-0294 | MIME SSOT: nexus-mime-icons + stash filetype icons | 2026-07-15 |
| ✅ TASK-0295 | Zero-copy read/write via VMO splice (OP_READ_VMO CAP_MOVE) | 2026-07-15 |
| ✅ TASK-0314 | Block driver v2: multi-sector runs + real queue depth + IRQ machinery | 2026-08-25 |
| ✅ TASK-0315 | Single GPT disk: virtioblkd sole owner + partition-scoped block plane | 2026-08-25 |
| ✅ TASK-0296 | nexus-wire: declarative service frame codec + nexus-abi identity split | 2026-07-20 |
| ✅ TASK-0297 | Time v1: goldfish rtcd + timed walltime + tz-lite + live clock | 2026-07-21 |
| ✅ TASK-0298 | Settings spine: region/keymap/time keys + OP_WATCH push propagation | 2026-07-21 |
| ✅ TASK-0301 | IPC last-sender EOF + app self-exit on window close (RFC-0079) | 2026-07-23 |
| ✅ TASK-0302 | Shared glyph-atlas RO VMO (RFC-0080; app-host ELF 5.9→1.66 MB) | 2026-07-23 |
| ✅ TASK-0303 | Process reaper: service-driven zombie reclaim (RFC-0081) | 2026-07-24 |
| ✅ TASK-0307 | Settings distribution v2: single authority + versioned snapshots (RFC-0083) | 2026-07-27 |
| ✅ TASK-0310 | Kernel-owned VA allocation: vm_map/vm_unmap/mmio_map_auto (RFC-0085) | 2026-07-28 |

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

## Storage End-State Ladder (seeded 2026-08-14) — 0314–0320 + statefs lane

User decision 2026-08-14: build the **end architecture** (RFC-0071 "Apple-like" contract +
ADR-0044 topology), production grade, no further transitional solutions — and fix that the
filesystem is measurably slow (512 B/queue-depth-1 block driver, whole-file rewrites, FLUSH per
txn, zero caching, no VMO write path, no perf contract anywhere). Full rationale + milestone
table: `tasks/TRACK-STASH-USER-DATA-FS.md` (milestones 6–12).

| Task | Title | Status |
|------|-------|--------|
| ✅ TASK-0314 | Block driver v2: multi-sector + real queue depth + IRQ completion (perf multiplier for statefs AND nxfs) | Done 2026-08-25 (OTA-lane package 3; IRQ endpoint provisioning → 0315) |
| ✅ TASK-0315 | Block topology consolidation: ONE GPT device + virtioblkd sole queue owner (ADR-0044 end state) | Done 2026-08-25 (OTA-lane package 5) |
| TASK-0316 | nxfs engine v2: format v2 (contracted fields + volume table) + block-granular CoW + group commit + cache | Draft |
| TASK-0317 | nxfsd process extraction + vfs.capnp v2 write surface + VMO write path (RFC-0072 P2 closed) | Draft |
| TASK-0318 | Storage performance contract + benchmark gate (RFC-0071 perf amendment, real-number markers, regression gates) | Draft |
| TASK-0319 | nxfs Phase 3: CoW metadata tree (ADR first) + snapshots/clones + data checksums | Draft |
| TASK-0320 | nxfs Phase 4: encryption classes (per-extent XChaCha20-Poly1305, keystored HKDF) | Draft |

Parallel statefs lane (separate store, ADR-0043): `0025` → `0026` → `0027` — **all Done
2026-08-18** (see Defer-Bucket note; 0026 defused the `/state` replay-limit boot time bomb,
0027 shipped opt-in record AEAD).
Superseded by this ladder: `0264`/`0265` (pre-nxfs write-path drafts); `0135` needs a
`/data`-first rescope before execution.

---

## Reliability Spine (seeded 2026-08-18, **delivered 2026-08-24**) — recut sub-80 recovery lane

User decision 2026-08-18: the old `0049 → 0050 → 0051` sequence built forensics and
operator escape hatches for a system that cannot yet survive a failure (no OS-side
supervision, no reset path, RAM-only evidence, every fault = `exit(-22)`, TASK-0018's
OS proof retired). The lane was recut to the **end architecture** in dependency order:
detection → supervision → evidence → boot state → operations → authorization. No
transitional solutions; conservation principles from the S3K analysis live in the
contracts (restart = derivation, exhaustion = event, supervision right-of-way), the
slice mechanism is rejected — mechanism study parked in `TRACK-TIME-AS-RESOURCE`
(no tasks; gates RED).

Contracts: RFC-0087 (failure model) · ADR-0055 (`bootctld` = boot-state authority) ·
ADR-0056 (exit reasons in the kernel ABI) · ADR-0057 (restart/capability re-resolve).

| # | Task | Title | Status |
|---|------|-------|--------|
| 1 | ✅ TASK-0049 | Fault & exhaustion truth + crash-proof reanimation (rewritten; old crashd scope → 0051B) | Done 2026-08-20 |
| 2 | ✅ TASK-0049B | Service supervision v1: tiers + restart/backoff/crash-loop + re-resolve | Done 2026-08-20 |
| 3 | ✅ TASK-0049C | Persistent evidence journal (logd → statefs spill) | Done 2026-08-20 |
| 4 | ✅ TASK-0050 | System reset (SBI SRST) + boot targets via `bootctld` (rewritten) | Done 2026-08-24 |
| 5 | ✅ TASK-0051 | Recovery operations surface: fsck op + slot/target ops + `nx diagnose` (rewritten) | Done 2026-08-24 |
| 6 | ✅ TASK-0051B | Crash evidence at rest: on-device `.nxcd` + retention/GC + redaction | Done 2026-08-24 |
| 7 | ✅ TASK-0053 | `.nxra` signed recovery actions (rewritten: enforcement on the 0051 ops surface) | Done 2026-08-24 — **Reliability Spine complete** |
| — | TASK-0050B | Recovery bringup console — **Deferred by decision** (no shell in the consumer end state) | Deferred |

⤳ `0178` **Superseded 2026-08-18** (absorbed by 0050/ADR-0055). Rebased against this
lane: `0036` (amendment: `bootargd`/`healthd` dead, soft-reboot sim → real reset proof),
`0141`/`0142` (crash pipeline reownership), `0179` (updated = bootctld client),
`0227` (`nx diagnose` = the ONE bundle; consumes 0049C/0050/0051 edges), `0228` (oomd
kills carry ADR-0056 reasons; supervisor owns restarts), `0234`/`0235` (kill-reason
taxonomy = ADR-0056), `0260`/`0261` (boot-state seam; `rebootd`/initrd/`nx-diag`
dropped), `0289` (rollback indices anchor in the bootctld record). `0052` left the
lane (Networking/Ingress, unchanged).

---

## Updates/OTA Lane (seeded 2026-08-25) — ACTIVE — end-state full-image A/B, bundle-set-ready

User decisions 2026-08-25: production-grade end architecture, no interim solutions;
target UX = **bundle-set granularity**, built as full-image OTA first with the
bundle evolution structurally prepared (component manifest from day 1, chain-of-trust
layering so the loader never needs rework, reserved system-a/b volumes — RFC-0089 §12);
storage substrate 0314+0315 pulled INTO the lane; existing ledgers REWRITTEN, not
re-seeded. Ground truth at seeding: the bootctld machine is real and proven, but no
second system copy exists anywhere (one flat boot image via the VMM kernel option;
device "slot switch" = an atomic u8 + synthetic build.prop), and `.nxs` signatures
were verified against a key from the archive itself (live hole; closed by package 1).

Contracts: RFC-0089 (OTA v2 end-to-end; supersedes RFC-0012 in part) · ADR-0058
(BSB dual-actor write matrix) · ADR-0059 (nxboot boot chain + measured handoff).

| # | Task | Title | Status |
|---|------|-------|--------|
| 1 | ✅ TASK-0198 P1 | Device publisher trust anchor + verifier verdict authority (closes the self-key hole) | Delivered 2026-08-25 (test-all green; ledger In Progress for P2+ sigchain/translog/rotation) |
| 2 | ✅ TASK-0036-A | Health-commit v2 in bootctld: record v3 + quorum + wall-clock deadline | Delivered 2026-08-25 (test-all green) |
| 3 | ✅ TASK-0314 | virtio-blk driver v2 (multisector/queue/IRQ) — request ring + 16 KiB runs + IRQ machinery (endpoint provisioning → 0315) | Delivered 2026-08-25 (test-all green) |
| 4 | ✅ TASK-0260 | `nx image build/verify/patch/ota` — deterministic GPT assembler + NXBD signer + factory BSB (rewritten) | Image scope delivered 2026-08-25 (test-all green; ledger In Progress for the flasher/factory-reset residual) |
| 5 | ✅ TASK-0315 | Single GPT disk: virtioblkd sole owner + OTA partitions + IRQ endpoint — boot still direct | Delivered 2026-08-25 (test-all green) |
| 6 | TASK-0289-A | `nxboot` first-stage loader + boot flip + measured handoff (flag-day; kernel-touch) | ✅ Phase A complete 2026-08-30 — flip LIVE in every lane |
| 7 | ✅ TASK-0036-B | bootctld BSB projection (record first, BSB second, idempotent resync) | Delivered 2026-08-30 — full loader↔bootctld loop proven |
| 8 | ✅ TASK-0179 | updated apply engine v2 + offline feed — **CROWN PROOF: first real slot flip, new build id visible** (rewritten) | Delivered 2026-08-31 — crown proof green, gated in test-all |
| 9 | ✅ TASK-0289-B | Boot trust floor closure: loader backstops (tamper/downgrade/tries-exhausted) + measured surface | Delivered 2026-08-31 — three lanes green + gated (`ci-os-ota-backstops`), measured cross-check required in every proof lane |
| 10 | TASK-0140 | Settings→Updates page + `nx update` CLI over the real engine (rewritten; lands after UI handoff tracks) | ✅ Done 2026-09-01 |
| 11 | TASK-0034/0035 | Delta as component kinds (`boot-image-delta`; format RFC at execution) | 0034 ✅ Done 2026-09-01 (RFC-0090); 0035 Draft (hinter Phase B) |
| 12 | TASK-0321 | Phase B: verified system volume (`system-a/b`) + service migration out of the boot image + `bundle` components with unchanged-bundle reuse (RFC-0089 §12) | Seeded 2026-09-03 (paper; unparks 0035) |

After the lane (contracted, not built): Phase B bundle-set = row 12 / TASK-0321 (RFC-0089 §12 — services
leave the embedded image; `bundle` components + system volumes; seeded 2026-09-03), network transport
(after the 2-VM CI repair), `0261` flashd/provisioning (rebased on the GPT layout),
`0239` per-app A/B, `0197`/`0198` P2+ (sigchain/translog/rotation). Paper hygiene done
at seeding: `0089`/`0090` → Windowing group, `0174` → Text/IME group (miscounted in
Updates), `.nxs`/`.nxdelta`/`pkgimg`/NXBD/BSB/`nxboot`/`updated` registered in
TRACK-AUTHORITY-NAMING, RFC-0012 supersession note, 0178 stale links removed.

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
| ✅ TASK-0029 | Supply Chain v1: SBOM + repro metadata + signature allowlist | Harte Dep von TASK-0031 (VMOs); host closure + docs sync landed, QEMU supply-chain marker profile verified |
| ✅ TASK-0031 | Zero-copy VMOs v1: shared RO buffers + handle transfer | Kritisch: VMO-backed Surfaces für windowd-Compositor |
| ✅ TASK-0032 | PackageFS v2: RO image index + fastpath | App-Asset-Laden für Launcher |
| ✅ TASK-0039 | Sandboxing v1: VFS namespaces + CapFd + manifest permissions | App-Isolation |
| ✅ TASK-0045 | DevX nx-cli v1 | `nx dsl build/lint/fmt` für DSL-Workflow |
| ✅ TASK-0046 | Config v1: configd + JSON Schema + layering + 2PC reload | UI-Profil-Broker für windowd + input |
| ✅ TASK-0047 | Policy as Code v1: unified policy engine | Asset-Zugriff + Permissions für UI-Services |

**Übersprungen (24–53):** `0024` (DSoftBus UDP sec), ✅ `0025–0027` (StateFS hardening/encryption — **alle Done 2026-08-18**, siehe Defer-Bucket), `0028` (ABI filters v2), `0030` (DSoftBus discovery authz), `0033` (PackageFS VMO-splice — ⤳ superseded by 0295), `0034–0037` (OTA/delta updates), `0038` (Tracing v2), `0040` (Remote observability), `0041` (Lock profiling), `0042` (SMP v2 voll — inzwischen Done; 0054B war der QoS-Slice-Träger und ist Superseded), `0043–0044` (Security sandbox quotas / QUIC tuning — 0044 Superseded 2026-08-14), ✅ `0048` (Crashdump v2a host, Done 2026-08-14), ✅ `0049–0051B`/`0053` (→ **Reliability Spine**, Sektion oben — **alle 7 Done 2026-08-20..24**), `0052` (Ingress, in der Networking-Lane).

---

### Schritt 2 — Sichtbare UI + Input-Spine (54–56B, 252–253, 56C)

Vom CPU-Renderer bis zum sichtbaren deterministischen Input-Proof und dann direkt zur echten Input-Architektur.

| Task | Title |
|------|-------|
| ✅ TASK-0054 | UI v1a: BGRA8888 CPU renderer + damage tracking + headless snapshots (Done; host renderer/snapshot proof floor green) |
| ✅ TASK-0055 | UI v1b: windowd compositor + surfaces/layers IPC + VMO buffers + vsync (Done; headless present + generated IDL roundtrip + reject proofs green) |
| ✅ TASK-0055B | UI v1c: visible QEMU scanout bootstrap (Done; marker-honesty hardening + full closure gates green) |
| ✅ TASK-0055C | UI v1d: windowd visible present + SystemUI first frame in QEMU (Done; composed-frame visible-present proof + closure gates green) |
| ✅ TASK-0055D | UI v1e: dev display/profile presets (Done 2026-09-03; 7 preset manifests + registry resolver + `test_reject_*` suite + `nx ui preset` + `just start-preset`; honest recuts Hz=120/mode≤1280×800/guest profile → TASK-0322) |
| TASK-0322 | UI dev presets guest side: fw_cfg `ui-preset` → settingsd default overlay → `systemui: profile …` + `SELFTEST: ui preset boot ok` (+ optional mode-driven pacer Hz) | Draft (seeded 2026-09-03) |
| ✅ TASK-0056 | UI v2a: double-buffered surfaces + present scheduler + input routing (Done; host/reject/QEMU proofs + fmt/clippy/ci-network + make clean/build/test/run green) |
| ✅ TASK-0056B | UI v2a: visible input — cursor + hover + focus + click (Done; deterministic host/reject/QEMU proofs + closure gates green; live device input follows in 0252/0253) |
| ✅ TASK-0252 | Input v1.0a: host HID/touch/keymaps/repeat/pointer-accel core (Done; host-first contract closed with full gate reruns green) |
| ✅ TASK-0253 | Input v1.0b: OS/QEMU hidrawd + touchd + inputd + windowd/IME hooks (Done; live QEMU pointer/keyboard floor, full closure gates green) |
| ✅ TASK-0056C | UI v2a: embedded reactor/runtime floor + present/input perf latency + coalescing (Done; host-first coalescing + no-damage-skip + idle-cheap proofs green; QEMU marker ladder + diag-os pending downstream) |

**Defer aus diesem Bereich (Stand 2026-08-14):** ⤳ `0054B`/`0054D` Superseded (geliefert via 0042/0277/0283/0288 bzw. 0310/0309/0302); `0054C` (IPC-Fastpath, rebased) und `0055D` (dev display presets, rebased) in der Sub-80-Umsetzung.

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
| ✅ TASK-0146 | IME v2 Part 1a: ime-core dead/compose + DSL focused-field model + wire codecs (Done 2026-07-21, host-proven; RFC-0075 Phase 0) |
| TASK-0147 | IME v2 Part 1b: imed real + typing lands in apps + OSK app ime-ui (rewritten 2026-07-21, RFC-0075) — **active track, still open** |
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
| TASK-0074 | UI v10b: **modals/modal-manager residual** — still open, aber stark reduziert (rebased 2026-08-14: App-Shell-Kit existiert längst via TASK-0308/`userspace/apps/window-kit` (RFC-0084); W6-Konvergenz anders erledigt; Overlays = app-owned `.nx`; Rest = Modal-Manager-Semantik NICHT in windowd) |

**Defer aus diesem Bereich:** `0067` (rebased 2026-08-14: clipboardd-SERVICE + DnD-Routing — 0122C ist nur die DSL-Bridge), `0068` (rebased: capture-only, Share → 0126–0128); ⤳ `0069` Superseded → 0123–0125, ⤳ `0071` Superseded → 0151–0154.

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

## Sub-80 Tracking (Defer-Bucket, umgebaut 2026-09-03) — HIER Fortschritt mitverfolgen

Regeln (User-Entscheidung 2026-09-03): **Phase 1 = alle offenen Tasks < 0054 OHNE
Netzwerk-Bezug**, danach **Phase 2 = 0054–0079**. Voraussetzungs-Tasks gehören in die Lane
(0321 → 0035). Die Netz-Familie (dsoftbus + Proof-Lane-Reparatur) steht auf **HOLD** bis zur
gemeinsamen Besprechung. Jeder Task ist erst fertig, wenn sein Ledger `Done` ist; jedes Paket
wird production-grade auf das End-System gebaut (keine Interimslösung — Ledger-Sektion
„End-state rewrite 2026-09-03“ ist die Scope-Wahrheit). Zähler werden mechanisch aus den
Ledgern nachgeführt, nie geschätzt. Vokabular: ✅ delivered · ⤳ superseded · `Draft` /
`In Progress` / `Done <date>` / `Delivered <date> (test-all green)`.

### A — Phase 1: Sub-54 ohne Netz — ACTIVE · Pakete 11/24 delivered · Tasks 2/5 Done

Reihenfolge: **0321 → 0035 → 0028 → 0043 → 0052**.

| # | Task / Paket | Inhalt (End-State) | Status |
|---|---|---|---|
| 1 | ✅ TASK-0321 P0 | RFC-0089 §12-Amendment (NXSV, Kind 6 `system-volume`, Ordnung + `commit_set`, Pairing, Gate-Matrix) + ADR-0060 (bundlemgrd = Verifier, init = Spawner) | Delivered 2026-09-03 (paper) |
| 2 | ✅ TASK-0321 P1 | Host: pkgimg v3 (Bundle-Tabelle, per-Entry/Bundle-Digests, Launch-Params) + `bootfmt::nxsv` + `nx image build --system-bundles / verify / ota --bundle-set` + `scripts/build.sh` System-Bundles + Budget-Zeile `system-a` | Delivered 2026-09-03 (host tests + check green; disk builds with system-a) |
| 3 | ✅ TASK-0321 P2 | OS: op-aware virtioblkd-Gates, bundlemgrd `volume.rs` (NXSV-Verify, Bundle-ELF über VMO), init `service_source` + zweiter Spawn-Pass, Pilot **metricsd** vom Volume | Delivered 2026-09-03 (headless green: volume verified → bundle served → spawn from volume → metricsd ready → deny probe) |
| 4 | ✅ TASK-0321 P3 | OS: Kinds 2/6 Apply (generischer `volume_apply::VolumeAssembler` + `updated/volume_os.rs`, Reuse aus aktivem Volume, NXSV LAST, `restage clean`), Fixture `bundle-set.nxs`, Lane `ota-bundle` (zwei Boots, ein uart → `SELFTEST: ota bundle-set ok`) | Delivered 2026-09-03 (test-all green; ota-bundle: Volume verified auf Slot b, metricsd@1.0.1 vom Volume gespawnt) |
| 5 | ✅ TASK-0321 P4a | Boot-Wellen-Umbau (`core_plane.rs`: Welle 0 = policyd/virtioblkd/bundlemgrd, Volume-Pass VOR allen Mints), 12 Dienste aufs Volume (metricsd…pinched), Respawn aus dem Volume-Mapping, Reuse-Beweis (`--reuse-from`, 11 reused) | Delivered 2026-09-04 (headless/ota-bundle green; FUND: 6-KiB-Block-Plane ≈ 1 ms/Roundtrip → gpud/windowd = +1,3 s Boot → bleiben bis P4b eingebettet) |
| 5b | ✅ TASK-0321 P4b | Bulk-Volume-Read (`blockproto OP_ARM/READ/RELEASE_VMO`, virtioblkd streamt Geräte-Runs direkt ins VMO, bundlemgrd 1 Read pro Fenster) + Migration gpud/windowd (14 Dienste vom Volume) | Delivered 2026-09-04 (windowd 7 MB: read 38 ms, hash 881 ms unter TCG → Rest = SHA-256-Emulation, Follow-up Zknh/parallel) |
| 6 | ✅ TASK-0321 P5 | Boot-Image-Floor: Apps als Volume-Bundles (`nx app compile`, `meta/app.properties`, `payload.nxir`), bundlemgrd-Registry + GET_PAYLOAD vom Volume, packagefsd `pkg:/` = Index (`GET_INDEX`) + Dateien on demand (`GET_FILE_VMO`), FETCH_IMAGE/Transcode retired, `system`-Bundle | Delivered 2026-09-04 (FUND: packagefsd→bundlemgrd-Route war nonce-los falsch aufgelöst — der RAM-Seed lief still) → **TASK-0321 Done** |
| 7 | ✅ TASK-0035 P1 | Stage-Journal `NXSJ` (Sektor 1, an NXSV-Digests gebunden, CRC) + per-Bundle-Resume mit Readback-Verifikation (`updated: restage resume (bundles=k/N)`), Lane `ota-bundle-resume` (QMP-Power-Cut mid-stage) | Delivered 2026-09-04 (Host-Matrix + Lane, siehe CHANGELOG) |
| 8 | ✅ TASK-0035 P2 | Host-Reuse-Index (`nx image ota --bundle-set --reuse-from <active.img|dir>`, nur geänderte Bundles, JSON-Reuse-Manifest) | Delivered 2026-09-04 (Host-Test gegen Disk-Image + Verzeichnis) |
| 9 | ✅ TASK-0035 P3 | `bundle-delta` Kind 4 (DeltaAdapter-Bundle-Modus, `SetSink`, `VolumeBase`, `delta-base` vor jedem Write), `nx image ota --delta-from-volume`, Lane `ota-bundle-delta` | Delivered 2026-09-05 (3 Host-Tests + Lane, siehe CHANGELOG) |
| 10 | ✅ TASK-0035 P4 | Close: RFC-0089 Phase 10 Kind 4 ✅ + Phase-B-Checkliste, docs/updates/delta.md, CHANGELOG, DoD-Abgleich | Delivered 2026-09-05 → **TASK-0035 Done** |
| 11 | TASK-0028 P0 | RFC-Seed „Policy-Profil v2“ (Schema: statefs / net.bind+Adresse / net.connect / limits / epoch; longest-prefix, deny-beats-allow) — EIN nexus-abi-Approval für 0028/0043/0052 | Draft |
| 12 | TASK-0028 P1 | Matcher + Codec v2 in BEIDEN Parsern + Reject-Suite (`test_reject_regex_dos`, `_argument_injection`, `_stale_profile_epoch`, `_unauthenticated_mode_switch`, `test_learn_roundtrip`) | Draft |
| 13 | TASK-0028 P2 | Learn-Pipeline (`policyd.learn` → logd, Sampling/Token-Bucket) + `nx policy learn-gen` | Draft |
| 14 | TASK-0028 P3 | OS: `SetAbiMode` (auth + epoch), echte Enforcement-Aufrufe in statefsd/netstackd, Marker `SELFTEST: abi …` | Draft |
| 15 | TASK-0043 P0 | RFC-0072-Amendment `EDQUOTA` (statefs 12 / VfsError 14); Quota-Modell = TASK-0133 (soft/hard), Enforcement statefsd | Draft |
| 16 | TASK-0043 P1 | statefs-Quota-Accounting host (`tests/state_quota_host/`) + OS (`statefs: quota deny`, `SELFTEST: quota deny ok`) | Draft |
| 17 | TASK-0043 P2 | netstackd-Identität (`sid` → Facade) + `STATUS_DENY` + policyd-Authorize an connect/listen/bind — gemeinsamer Vorbau mit 0052 | Draft |
| 18 | TASK-0043 P3 | Egress-Policy (`net.connect` CIDR/Port) host + OS (`net-egress: enforced`, `SELFTEST: egress deny/allow ok`) | Draft |
| 19 | TASK-0043 P4 | Audit-Taxonomie (`AuditReason` += Quota/Egress/Ingress/AbiRule) + Counter + Docs | Draft |
| 20 | TASK-0052 P0 | RFC-Seed „Service Exposure Contract“ (`ExposeIntent`-IDL) + ADR „Exposure-Intent statt freier Binds“ | Draft |
| 21 | TASK-0052 P1 | Schicht A: `ingress`-Policy-Domäne + `net.bind`-Adressdimension (loopback-only default) an der Facade, `SELFTEST: ingress deny ok` | Draft |
| 22 | TASK-0052 P2 | `ingressd` host (`tests/ingress_host/`: allow / cidr deny / rate) | Draft |
| 23 | TASK-0052 P3 | `ingressd` OS (`ingressd: ready`, `port open`, `deny (reason=…)`, `SELFTEST: ingress allow/deny/rate ok`; Loopback-Beweis im Einzel-VM-Profil; TLS = Contract-Slot, Umsetzung am Netz-Track) | Draft |

Nicht zählende Sub-54-Einträge: TASK-0050B **Deferred by decision** (Aktivierungsgate: reale
Hardware); TASK-0011 Done. Superseded < 0054: 0011B, 0033 (→0295), 0037 (→0289), 0041
(→ADR-0049), 0044 (→0024). Done-Lanes: Storage 0025–0027 (2026-08-18), Reliability Spine
0049/0049B/0049C/0050/0051/0051B/0053 (2026-08-24), OTA-Pakete 0–11 (2026-09-01).

### B — Netz-Familie — HOLD (gemeinsame Besprechung ausstehend, User braucht Änderungen)

| Task | Inhalt (Ledger-Stand) | Stand |
|---|---|---|
| NET-W1/W2/W3h | TRACK-NETWORK-PROOF-LANES: Cross-Device-Discovery tot (`OS2VM_E_DISCOVERY_TIMEOUT`, A=0 B=0 seit ≥2026-07-24); `quic-required` verlangt Peer im Einzel-VM-Profil; Runner-Härtung; `ci-network` in keinem Gate | 1/5 Exit-Kriterien |
| TASK-0024 | QUIC-v2-Reliability im OS-Datapath (DATA/ACK, Retransmit, cwnd, Pacing) | Draft, OS-Beweis braucht W1 |
| TASK-0030 | Discovery-Härtung auf NXSB (TTL/Backoff, Pre-Session-ACL, Rate-Limits) | Draft, OS-Beweis braucht W1 |
| TASK-0038 | Tracing v2 Cross-Node über mux_v2 (RFC-Seed, u64 TraceId) | Draft, Cross-VM braucht W1 |
| TASK-0040 | Remote Observability v1 (Collector, greenfield) | Draft, Cross-Node braucht W1 |

### C — Phase 2: 0054–0079 (nach Phase 1)

Reihenfolge: **0067 → 0067B → 0068 → 0074 → 0066 → 0077B → 0077C → 0079 → 0054C**.

| # | Task | Inhalt (rebased) | Größe | Status | Voraussetzung / Blocker |
|---|---|---|---|---|---|
| 1 | TASK-0067 | DnD-Controller (typed offers) + clipboardd-Service (MIME, History, Policy) — Boundary: Widget-UI, nicht windowd | M | Draft | keine |
| 2 | TASK-0067B | Clipboard-History-Overlay/App (DSL) + Share-Hooks | S | Draft | 0067 |
| 3 | TASK-0068 | Screenshot `screencapd` (capture-only Rebase) + Share-Sheet-Broker + Privacy-Guards | M | Draft | 0067 |
| 4 | TASK-0074 | App-Shell-Adoption + Modal-Manager + Toast-Vereinheitlichung (reduziertes Residual) | M | Draft | 0073 Done; Design-Handoff-Tracks 0305–0313 |
| 5 | TASK-0066 | WM Split/Snap Residual: Thirds, Zone-Map, Reflow, `list()`, Policy | M | Draft | ⚠ Snap-Release→Fullscreen-Wedge zuerst triagieren |
| 6 | TASK-0077B | DSL DevX: lokales `$state`, Two-Way-Bindings, Async-Recipes (~60–70 % geliefert) | S | Draft | keine |
| 7 | TASK-0077C | DSL Pro-Primitive: VirtualTable/Grid, Timeline, NativeWidget (demand-gated) | L | Draft | 0077B; Bedarf |
| 8 | TASK-0079 | DSL AOT Rust-Codegen + inkrementelle Builds + Asset-Embedding | L | Draft | 0078/0080D Done |
| 9 | TASK-0054C | Kernel-IPC-Fastpath auf phased/lockfree-Baseline | M | Draft | Design-Pass + Kernel-Approval; nur mit Mikrobench-Evidenz nach ADR-0049 |

Done/Superseded 0054–0079 (Referenz): ✅ 0055D (2026-09-03, Presets — Guest-Ingestion → 0322),
✅ 0060/0060B/0062B/0080; ⤳ 0054B/0054D, 0069 (→0123–0125), 0071 (→0151–0154), 0076B.

### Nach 80 (Referenz)

0081–0118 Apps + Plattform (nach DSL App Platform 0122B/C); SMP-Closure 0281/0282/0286/0287/0290;
Storage-Leiter 0316–0320; 0198 P2+ / 0260-Residual / 0239 / 0261 (OTA-Umfeld); Netz-Transport
für OTA nach NET-W1.

---

## Genuinely open themes (not started — no daemon/app/marker exists yet)

Honest reconciliation floor (2026-07-19). These are real, unimplemented feature areas — not paperwork drift:

- **IME / text input engine (ACTIVE TRACK 2026-07-21, RFC-0075):** `0146`, `0147`, `0149`, `0150`, `0203`, `0204` — ledgers rewritten against repo reality; `0096` Superseded, `0148` Deferred (no bidi need for Latin+CJK)
- **Notifications service:** `0123`, `0124`, `0125` (minimal surface shipped in 0065; notifd/DND/headsup open; ⤳ `0069` Superseded 2026-08-14 — actions/inline-reply-Wire-Shape in 0123 übernommen)
- **Search / command palette:** `0151–0154` (⤳ `0071` Superseded 2026-08-14)
- **Clipboard / share / DnD:** `0067`, `0087`, `0126–0128`
- **Content / files apps:** `0081–0093`, `0232`, `0233`
- **Media / audio:** `0099–0102`, `0155`, `0156`, `0184–0187`, `0217–0220`, `0254`, `0255`
- **Accessibility:** `0114–0118` (semantics tree shipped in 0061; a11yd hardening open)
- **Camera / privacy:** `0103–0106`, `0191`, `0192`
- **Webview:** `0111–0113`, `0176`, `0177`, `0205`, `0206`
- **Store / distribution:** `0180`, `0181`, `0221`, `0222`
- **Backup / L10n / power / sensors:** `0161`, `0162`, `0240`, `0241` (i18n v2 locale packs, ACTIVE TRACK, RFC-0077; `0174`/`0175` Superseded), `0236`, `0237`, `0256–0259`, `0271`, `0272`
- **Time / wall-clock + General-management settings:** ✅ `0297` Done (timed reads RTC directly + tz-lite + live clock, RFC-0076 incl. documented no-rtcd deviation), ✅ `0298` Done (settings watch spine, RFC-0078); seeds `0299` (SNTP), `0300` (IME-store encryption)
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
| TRACK-TIME-AS-RESOURCE | Conserved time/budget rights along the init tree (S3K-inspired; slice model rejected) | Gates RED: bounded kernel ops/revoke (BKL), TASK-0286/0287, hmv2 SMP direction |
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
