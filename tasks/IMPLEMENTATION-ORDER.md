# Implementation Order — the execution view

**What this file is:** the ONE sequential/lane view over `tasks/TASK-*.md`. It says what is
being built now, in which order, and what is deliberately parked. It is **not** authoritative
for scope or Definition of Done — every `TASK-*.md` header is the execution truth, and
`tasks/STATUS-BOARD.md` is the only Done list (this file links, it does not duplicate).

Condensed 2026-09-09 (sub-80 reconciliation): the per-task Done table, the per-package
Phase-1 rows and the per-task tables of finished lanes moved out (they live in the ledgers,
the board and `CHANGELOG.md`); what remains is what an agent needs to pick the next package.

## Tasks and TRACKs

- **Task** (`TASK-XXXX-*.md`): atomic work unit with proofs (host tests + QEMU markers) and a
  Definition of Done. Status: `Draft` → `In Progress` → `Done` (or `Superseded` / closed by
  decision, always with the actual solution recorded in the ledger).
- **Track** (`TRACK-*.md`): vision document; never executed directly — it spawns tasks (next
  free number) once its gates clear.
- Every task is built to the **end system**, production-grade, no interim solutions; a new
  mechanism REPLACES the old one and the old one is deleted in the same package with a gate
  against its return (user rules 2026-09-03 / 2026-09-09; see `CLAUDE.md`).
- Ledger discipline: `In Progress` when a task starts, `Done` + docs sweep (CHANGELOG, docs,
  board, RFC status) when it closes; counters on the board are recomputed mechanically from
  ledger headers, never nudged.

---

## Active now (2026-09-09)

| # | Lane / Task | State | Next |
|---|---|---|---|
| 1 | **TASK-0324** Display handoff deterministic by construction (build truth, one wiring structure, stage fence, handoff v2) | **Done 2026-09-15** — P0 build truth + pixel lane, P1 RFC-0093/ADR-0062, P2 `@ready`, P3 routing v2, P4a–f ONE slot topology (191 → 0 positional slots), P5a–c stage fence, P6a–d handoff v2, P7a–d waits without clocks (gate at zero), P8 init without clocks + closure docs, P9 8/8 visible boots; every package `just test-all`-proven | Phase 2 (Sub-80) starts: `0054C → 0033 → 0077B → …` |
| 2 | **Sub-80 Phase 2** (below) | **Started 2026-09-15** — process: each task is reviewed against the code (idea → best realization, end state), its ledger rewritten, built, proven green, pushed, then the next is planned; **TASK-0054C Done 2026-09-18** (P0–P6), **TASK-0033 Done 2026-09-18** (P0–P3) — next: 0077B | order `0054C → 0033 → 0077B → 0077C → 0074 → 0066 → 0067 → 0067B → 0068` |
| 3 | **Network family** (`0024`, `0030`, `0038`, `0040`, NET-W1) | HOLD — joint discussion pending (user wants changes) | not before the discussion |

---

## Sub-80 tracking (mission: every task < 0080 Done)

Rules (user, 2026-09-03): Phase 1 = every open task < 0054 without network scope, then Phase 2
= 0054–0079; prerequisite tasks are pulled INTO the lane (0321 before 0035), never bypassed;
the network family stays on HOLD until the joint discussion. Vocabulary: ✅ Done · ⤳ Superseded
· `Draft` / `In Progress`.

### Phase 1 — sub-54 without network — ✅ COMPLETE 2026-09-08 (5/5 Done, 24 packages)

| Task | Delivered | Done |
|---|---|---|
| ✅ TASK-0321 | Phase B: verified system volume `system-a/b` (NXSV, pkgimg v3), 14 services + apps off the boot image, bundle reuse, bulk volume reads | 2026-09-04 (30f64e18) |
| ✅ TASK-0035 | Bundle-set deltas: stage journal + resume, host reuse index, `bundle-delta` kind, three OTA lanes | 2026-09-05 (e91bdeed) |
| ✅ TASK-0028 | Policy profile v2 (RFC-0091): argument matchers, learn/enforce, `OP_ABI_EVAL`, statefsd seam | 2026-09-07 (11be16f8) |
| ✅ TASK-0043 | statefs quotas (EDQUOTA), netstackd identity + egress seam, one deny taxonomy + counters | 2026-09-08 (b16434db) |
| ✅ TASK-0052 | Service exposure contract (RFC-0092) + `ingressd` gateway end to end, facade loopback/hairpin | 2026-09-08 (1ea180b7) |

Package-level history: `CHANGELOG.md` (2026-09-03 … 2026-09-08) and the ledgers' "End-state
rewrite 2026-09-03" sections.

### Sub-80 closed by reconciliation 2026-09-09 (goal reached by a better/other solution)

Each ledger carries a "Closure 2026-09-09" section with the actual solution and evidence.

| Task | Actual solution |
|---|---|
| ✅ TASK-0011B | Kernel Rust idioms — delivered under RFC-0020 (Complete): newtypes, phantom cap tags, explicit Send/Sync, `SysResult` |
| ✅ TASK-0037 | Boot slot decided by the boot chain — TASK-0289 `nxboot` + measured handoff record (`KSELFTEST: boot handoff ok (measured)`), not bootargs |
| ✅ TASK-0041 | Lock visibility — kernel lock budgets `core/trap/budgets.rs`, `KSELFTEST: bkl budget ok`, ADR-0049; no userspace profiler in the end system |
| ✅ TASK-0054B | Kernel/UI perf floor — QoS/affinity ABI (0042), init affinity placement, `SELFTEST: ui runtime floor ok` + budget gates (0277/0283/0288); hop/queue counters stay with TASK-0290 |
| ✅ TASK-0054D | MM perf floor — RFC-0085 kernel-owned VA (0310) + shared RO atlas VMO (0302); reuse counters stay with TASK-0290 |
| ✅ TASK-0076B | Visible DSL mount — app-host path (0080C/0080D); `APPHOST: mounted hash=` now gated in the `visible` lane |
| ✅ TASK-0050B | Recovery console — resolved by decision: none in the end system; `nx recovery` + bootctld targets + `.nxra` (0050/0051/0053) |
| ✅ TASK-0079 | AOT codegen — retired by decision: the app-host interpreter is the one execution tier; AOT docs/stubs deleted; runtime scale work → TASK-0077C |
| ⤳ TASK-0033 | **Done 2026-09-18**: the `pkg:/` VMO pass-through residual of the TASK-0295 supersession is discharged |
| ⤳ TASK-0044 | QUIC tuning → TASK-0024 (network family) · ⤳ TASK-0069 → 0123–0125 · ⤳ TASK-0071 → 0151–0154 (successors > 0080 own them; user decision 2026-09-09) |

### Phase 2 — 0054–0079 (after TASK-0324) — ledgers rewritten to end state 2026-09-09

Order and reason (API/codec/runtime foundations first so nothing is migrated later):

| # | Task | End-state content (see ledger "End-state rewrite 2026-09-09") | Size | Needs |
|---|---|---|---|---|
| 1 | TASK-0054C | IPC performance contract v2 + `ipc_call` / `ipc_reply_recv` fastpath — **Done 2026-09-18**. An exchange cost **208 µs and five kernel entries** and costs **41–43 µs and two**, asserted per boot (`KSELFTEST: ipc call budget ok`). One kernel allocation per message instead of two, none for the 72–93 % that fit inline (`IPC_SHORT_MAX = 32`, measured over 13 windows, not the 64 the RFC had guessed). `E2BIG` replaces `EINVAL` at the hard cap. The userspace consolidation that made the seam flippable in ONE function came first (P2-a…P2-g), and the one scheduler change the contract proposed was measured and WITHDRAWN (the runqueue hop is 0.41 % of an exchange). RFC-0096 Implemented, ADR-0064 Accepted | L | done |
| 2 | TASK-0033 | `pkg:/` reads = VMO pass-through vfsd → packagefsd → bundlemgrd; ONE payload-VMO header codec (`NXVR`, `NXPL` deleted) — **Done 2026-09-18** (RFC-0097 Implemented). It was **not a performance task**: `pkg:/` could serve 93 of the volume's 115 entries and a request for one of the other 22 ended packagefsd. Every lane was green because the only `pkg:/` file any proof read was `build.prop`, 19 bytes. Three defects the work uncovered: `STATUS_MALFORMED` and `PAYLOAD_STATUS_OK` were both `1` so an error header read as success; the size bound had no owner and its errno depended on which layer tripped first; packagefsd's response endpoint had three readers on one queue | M | done |
| 3 | TASK-0077B | Keyed per-instance `$state` (single-use rule deleted), Slider/Select/Stepper binds, async recipes, NX0407/0409 → errors, IR v1.3 | S–M | — |
| 4 | TASK-0077C | Runtime long-session & large-data contract: emit-generation arena (flat heap over N interactions), subtree re-emit, `heap-16m` workaround deleted; store-window rule documented (recut 2026-09-09: VirtualList/Table/Timeline/NativeWidget retired — paging is QuerySpec + `tail()`) | M | 0077B |
| 5 | TASK-0074 | Modal semantics in the DSL runtime: `.overlay(modal|transient)`, bounded stack, ESC/backdrop dismissal, focus trap, ONE windowd verb `CONTROL_WIN_MODAL`, toast as transient overlay | M | 0077B; 0324 P4a |
| 6 | TASK-0066 | WM zones: halves + occupancy-driven thirds, `zones.rs` replaces `snap.rs`, reflow on mode change, snap state in the window feed (RFC-0086 bits), fail-closed deny, registered markers — verify the snap-release→fullscreen wedge first | M | 0324 P4a/P6 |
| 7 | TASK-0067 | `clipboardd` = single content-transfer authority (multi-MIME, history 16, focus-gated reads pushed by windowd), `svc.clipboard.*` binding, DnD routing in windowd (RFC-0094, ADR); placeholders deleted; absorbs 0087 + 0122C clipboard bridge | L | 0324 P3/P4; 0054C; 0033; 0066 |
| 8 | TASK-0067B | Clipboard history panel in the desktop shell (DSL) + copy-back | S | 0067; 0074 |
| 9 | TASK-0068 | `screencapd` over the ONE readback authority (gpud `OP_READBACK` from 0324 P6; P0's `scanout_sample` replaced), windowd geometry/secure gate, consent = policy, caps (RFC-0095) | M | 0324 P6 (hard); 0074; 0033; 0054C |

### Network family — HOLD

| Task | Content | State |
|---|---|---|
| NET-W1/W2/W3h | `TRACK-NETWORK-PROOF-LANES`: cross-device discovery dead (`OS2VM_E_DISCOVERY_TIMEOUT` since ≥ 2026-07-24); `quic-required` demands a peer in the single-VM profile; `ci-network` in no gate | 1/5 exit criteria |
| TASK-0024 | QUIC v2 reliability in the OS datapath | Draft — OS proof needs W1 |
| TASK-0030 | Discovery hardening on NXSB (TTL/backoff, pre-session ACL, rate limits) | Draft — OS proof needs W1 |
| TASK-0038 | Tracing v2 cross-node over mux_v2 (context lib, sampler, collector are host-only buildable; propagation needs W1) | Draft |
| TASK-0040 | Remote observability v1 (collector, greenfield) | Draft — cross-node needs W1 |

---

## Completed lanes (reference — details in the ledgers, the board and CHANGELOG)

- **UI Fast Lane (steps 1–6, 2026-06…07):** foundations 0029/0031/0032/0039/0045/0046/0047 →
  visible UI + input spine 0054/0055/0055B/0055C/0056/0056B/0252/0253/0056C → UI content
  0057–0065B → shell infra 0070/0072/0073 → DSL foundation 0075–0080C → SystemUI DSL migration
  0119–0121. Still open from the lane: `0122` (needs the notifd feed), `0122B` (launch/open
  contract), `0122C` (integration kit — its clipboard bridge moved into 0067), `0147` (IME v2
  part 1b, active track RFC-0075), `0322` (dev-preset guest ingestion), and the Phase-2 rows
  above.
- **SMP / parallelism (kernel):** 0012, 0012B, 0042, 0276, 0277, 0283, 0288 Done (ADR-0045…0049,
  BKL wait 90.8 → ~6 ms). Open closure: `0281`, `0282`, `0286`, `0287`, `0290` (release blockers
  per the board).
- **Filesystem / user data:** 0291–0295 Done (RFC-0071/0072/0073, VMO splice reads, nxfs v1);
  statefs lane 0025/0026/0027 Done 2026-08-18. **Storage end-state ladder** (seeded 2026-08-14):
  0314 + 0315 Done 2026-08-25 (block driver v2, single GPT disk); `0316`–`0320` Draft (nxfs v2,
  nxfsd extraction, perf contract + bench gate, CoW/snapshots, encryption classes) —
  `TRACK-STASH-USER-DATA-FS` milestones 6–12.
- **Reliability spine (2026-08-18 → 24):** 0049 → 0049B → 0049C → 0050 → 0051 → 0051B → 0053 all
  Done (RFC-0087, ADR-0055/0056/0057; supervision, evidence journal, SBI reset + boot targets,
  recovery ops + `nx diagnose`, `.nxra`). 0050B closed by decision (above).
- **Updates / OTA lane (2026-08-25 → 09-05):** 0198 P1, 0036-A/B, 0314, 0260, 0315, 0289-A/B,
  0179 (crown proof: first real slot flip), 0140, 0034, then Phase B 0321 + 0035 (RFC-0089,
  ADR-0058/0059/0060, RFC-0090). Contracted, not built: network transport (after NET-W1),
  `0261` flashd/provisioning, `0239` per-app A/B, `0197`/`0198` P2+ (sigchain/translog/rotation),
  `0323` ingress follow-ups (UDP data plane, listener close, TLS).
- **Security lane (2026-09-05 → 08):** 0028 → 0043 → 0052 (RFC-0091/0092, ADR-0061) — Phase 1 above.

---

## After 80 (reference)

Genuinely open themes (no daemon/app/marker exists yet — honest floor, reconciled 2026-07-19):

- **Notifications:** `0123`, `0124`, `0125` (minimal surface shipped in 0065; ⤳ `0069`).
- **Search / command palette:** `0151`–`0154` (⤳ `0071`).
- **Share / DnD consumers:** `0087` (absorbed by 0067), `0126`–`0128` (share v2 over intentsd).
- **Content / files apps:** `0081`–`0093`, `0232`, `0233`.
- **Media / audio:** `0099`–`0102`, `0155`, `0156`, `0184`–`0187`, `0217`–`0220`, `0254`, `0255`.
- **Accessibility:** `0114`–`0118` (semantics tree shipped in 0061; a11yd hardening open).
- **Camera / privacy:** `0103`–`0106`, `0191`, `0192` (0105 recorder consumes 0068).
- **Webview:** `0111`–`0113`, `0176`, `0177`, `0205`, `0206`.
- **Store / distribution:** `0180`, `0181`, `0221`, `0222`.
- **Backup / L10n / power / sensors:** `0161`, `0162`, `0236`, `0237`, `0240`/`0241` (i18n v2, RFC-0077,
  active), `0256`–`0259`, `0271`, `0272`.
- **Time / general management:** ✅ `0297`, ✅ `0298`; seeds `0299` (SNTP), `0300` (IME-store encryption).
- **Renderer / compositor v2:** `0171`, `0199`, `0200`, `0207`, `0208`, `0215`, `0216`.
- **Session / accounts / lifecycle continuation:** `0234`, `0235`, `0109`, `0110`, `0223`, `0224`,
  `0126B`, `0159`.
- **IME (active track, RFC-0075):** `0147`, `0149`, `0150`, `0203`, `0204`.
- **Apps + platform** `0081`–`0118` after the DSL app platform (`0122B`/`0122C`).

## Active TRACKs (spawn tasks when gates clear)

| Track | Purpose | Blocked by |
|-------|---------|------------|
| TRACK-DRIVERS-ACCELERATORS | GPU/NPU/VPU device-class services | TASK-0010, TASK-0031, TASK-0012B |
| TRACK-NETWORKING-DRIVERS | NIC drivers, offload, netdevd | TASK-0003, TASK-0010, TASK-0012B |
| TRACK-NETWORK-PROOF-LANES | 2-VM proof-lane repair (W1–W3h) | joint network discussion |
| TRACK-NEXUSGFX-SDK | Graphics SDK for apps | UI tasks (0054+) |
| TRACK-NEXUSINFER-SDK | On-device ML runtime (CPU ref + future NPU), hybrid IPC | TASK-0031, TASK-0010, TASK-0280 |
| TRACK-NEXUSMEDIA-SDK | Audio/video/image SDK | UI tasks, codec tasks |
| TRACK-STASH-USER-DATA-FS | User-data FS ladder (vfs v2 → nxfs → zero-copy → CoW/enc) + stash | milestones 1–5 Done (0291–0295); 6–12 = 0314–0320 |
| TRACK-TIME-AS-RESOURCE | Conserved time/budget rights along the init tree (slice model rejected) | gates RED: bounded kernel ops/revoke (BKL), TASK-0286/0287 |
| TRACK-ZEROCOPY-APP-PLATFORM | RichContent + OpLog + connectors | TASK-0031, TASK-0067 (clipboardd) |
| TRACK-APP-STORE | Distribution + publishing | packaging tasks |
| TRACK-DEVSTUDIO-IDE | Developer IDE | DSL tasks (0075+) |
| TRACK-DSL-V1-DEVX | DSL language/runtime masterplan | phases 1–6 Done; 7 (AOT) retired; runtime scale = 0077C |

---

## Rules

1. **Lane order over task number.** Execute the active lane in its stated order; outside a
   lane, numerical order. Skip a blocked task, note why, move on.
2. **End system only.** No interim solution; a new structure replaces the old one and the old
   one is deleted in the same package, with a gate against its return. Prerequisites are
   pulled into the lane, not bypassed.
3. **100 % rule.** A task is Done only when every stop condition is met and the ledger, board,
   CHANGELOG and affected docs are swept; counters are recomputed from ledger headers.
4. **No fake success.** Markers prove real behaviour (`stub`/`placeholder`, never `ok`, for
   stubs); a marker is a contract — change it together with `scripts/qemu-test.sh`,
   `tools/nx/chains/markers.txt` and the proof manifest.
5. **Visible-proof-surface rule (UI tasks).** A UI capability is not claimed from host goldens,
   markers or an isolated demo alone: it must appear on the shared visible proof surface of the
   real QEMU desktop (text/wrapping target, SVG/icon + cursor path, scroll/clip/gesture window,
   animation zone, data window, settings/overlay/modal area, launcher/app-window/shell area,
   DSL pages that reuse exactly these targets) and be live-checkable there.
6. **Orbital-level UX gate (desktop/launcher quality claims).** Before any "desktop-quality"
   claim: visible login/greeter or dev session; live QEMU pointer + minimal keyboard; cursor,
   hover, focus, click, scroll; launcher/dock/taskbar; app start with visible window and
   focus/close/move ≥ v0; SVG sources for icons (PNG only as golden/derived); simple settings /
   quick settings; no global input leaks to apps; no marker-only desktop claims. Architecture
   stays service/capability-oriented: `inputd` normalizes, `windowd` owns hit-test/hover/focus/
   click, the DSL shell owns shell/launcher/session surfaces, apps get only their surfaces.
7. **Boundary.** windowd is the compositor SERVICE (single present authority); window UI lives
   in widgets and the DSL shell app; no Linux/Wayland paths; ADR for any kernel↔userspace,
   service↔service, host↔OS or policy-authority crossing.

## Related

- Status board (Kanban view, the Done list, group counters): `tasks/STATUS-BOARD.md`
- Task workflow rules: `tasks/README.md` · RFC process: `docs/rfcs/README.md`
- Agent guide (rules, gates, commands): `CLAUDE.md`
