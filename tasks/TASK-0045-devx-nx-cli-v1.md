---
title: TASK-0045 DevX: nx CLI v1 (scaffold + idl helpers + inspect + postflight runner + doctor)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Existing postflights: tools/postflight-*.sh
  - Existing IDL tool: tools/nexus-idl/
  - Packaging tool: tools/nxb-pack/
  - Testing contract: justfile + scripts/qemu-test.sh
---

## Context

We have many small developer tools (`nxb-pack`, `pkgr`, `arch-check`, `qemu-run`) and multiple postflight scripts.
We want a **single, consistent entrypoint** for common developer workflows that:

- reduces cognitive load (“one tool, consistent output”),
- avoids fake success (delegates to canonical proof mechanisms),
- stays host-first and does not affect OS runtime behavior.

Repo reality today:

- `tools/nexus-idl` is currently a stub (prints help only).
- Cap’n Proto bindings in `userspace/nexus-idl-runtime` are currently checked in under `src/manual/`.
- Postflight scripts exist and are already “defanged” to delegate to canonical proofs.

So `nx` v1 should integrate what exists, and make codegen incremental/opt-in until the repo’s IDL workflow is finalized.

Scope note:

- SDK v1 IDL freeze + deterministic codegen + SemVer/wire gates are tracked as `TASK-0163`.
- SDK v1 typed clients + app templates + `nx sdk` UX (with optional `nx-sdk` shim) are tracked as `TASK-0164`.
- SDK v1 Part 2 dev workflow (lints/pack/sign/CI) is tracked as `TASK-0165` (host-first) and `TASK-0166` (OS install/launch proofs, gated).

## Goal

Ship `tools/nx` (host CLI) with:

1. **Scaffolding** for new services/apps/tests that matches repo layout and doc standards.
2. **IDL helpers**:
   - list schemas,
   - validate schema inventory,
   - optional codegen/check mode (only where the repo already supports it).
3. **Inspect** common artifacts (initially: `.nxb` directories and related metadata).
4. **Postflight runner**:
   - topic → existing `tools/postflight-*.sh` mapping,
   - pretty summary of success/failure,
   - never defines success via log greps (exit code is authoritative).
5. **Doctor**: check common local dependencies and print actionable fixes.

## Non-Goals

- Changing OS runtime behavior.
- Replacing canonical proof scripts (`scripts/qemu-test.sh`, `cargo test`, `just test-*`).
- Full IDL pipeline redesign (this task can provide plumbing and checks, but does not force a new workflow).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No `unwrap/expect`; no blanket `allow(dead_code)` in the new tool.
- Deterministic behavior:
  - scaffolding must be stable (repeatable) and not reorder workspace members unpredictably,
  - postflight mapping is declarative and versioned.
- No fake success:
  - `nx postflight` must treat the underlying command exit code as truth.

## Red flags / decision points

- **YELLOW (IDL workflow drift)**:
  - Until we decide whether generated capnp bindings are checked in (and where), `nx idl codegen --check`
    must be optional and scoped to known paths (e.g. `tools/nexus-idl/schemas` only).
- **YELLOW (workspace edits)**:
  - Auto-editing `Cargo.toml` is convenient but risky; must be safe, idempotent, and produce a minimal diff.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic host tests (`tests/nx_cli_host/`):

- `nx new service foo` creates expected paths and does not break workspace parsing.
- `nx inspect` prints a stable JSON summary for a known fixture bundle.
- `nx postflight <topic>`:
  - passes when the underlying script succeeds (use a fixture script in tests),
  - fails when the underlying script fails.
- `nx doctor`:
  - reports missing tools with actionable hints (test with PATH overrides).

## Touched paths (allowlist)

- `tools/nx/` (new)
- `tools/nx/templates/` (new)
- `tools/nexus-idl/` (optional: minimal enhancements, but not required)
- `docs/devx/nx-cli.md` (new)
- `docs/testing/index.md` (add “developer loop” snippet)

## Plan (small PRs)

1. **Add `tools/nx` crate**
   - `clap` CLI parsing, consistent output formatting.
   - Base marker: `nx: ready (v1)` (host-only; printed only on `--version` or `nx doctor`, not during builds).

2. **Scaffolding**
   - `nx new service <name>` → `source/services/<name>/...`
   - `nx new app <appId>` → `userspace/apps/<appId>/...` (note: workspace already includes `userspace/apps/*`)
   - `nx new test <name>` → `tests/<name>_host/...`
   - Include:
     - CONTEXT headers,
     - minimal `Cargo.toml`,
     - minimal `main.rs`,
     - stub docs in `docs/stubs/` (explicit “stub” wording).
   - Workspace update:
     - only when strictly needed; idempotent edits.

3. **IDL helpers**
   - `nx idl list` lists schemas under `tools/nexus-idl/schemas/`.
   - `nx idl check` verifies:
     - schema files exist and are readable,
     - `capnp` tool exists,
     - optional: a schema inventory hash file is up-to-date (if we add one).
   - Defer “full codegen” until we standardize where generated rust lives.

4. **Inspect**
   - Start small and honest:
     - inspect `.nxb` directories:
       - list files present (`manifest.*`, `payload.elf`, `meta/*`),
       - compute sha256 of payload,
       - print JSON summary (`--json`).
   - Extend later as packaging stabilizes (`manifest.nxb`, SBOM embed).

5. **Postflight runner**
   - `nx postflight <topic>` maps topics to scripts:
     - `vfs`, `vfs-userspace`, `proc`, `policy`, `kspawn`, `loader`, etc.
   - Behavior:
     - run script as-is,
     - show elapsed time + exit code,
     - show last N lines of output (bounded).

6. **Doctor**
   - Check for:
     - `rustc`, `cargo`, `just`,
     - `qemu-system-riscv64`,
     - `capnp`,
     - optional: `rg`, `python3`.
   - Print actionable guidance and exit non-zero if required tools missing.

7. **Docs**
   - `docs/devx/nx-cli.md` with:
     - quickstart,
     - command reference,
     - how to add a postflight topic (edit a mapping file in `tools/nx/`).

## Follow-ups

- Central config system + `nx config` subcommands are tracked separately as `TASK-0046`.
- Policy as Code + `nx policy` subcommands are tracked separately as `TASK-0047`.
- Crashdump v2 tooling (`nxsym` + `nx crash`) is tracked as `TASK-0048` (host-first) and `TASK-0049` (OS).
- Recovery mode + `nx recovery` is tracked as `TASK-0050` (boot target + safe shell) and `TASK-0051` (safe tools + CLI).
- Security v3 (ingress + signed recovery actions) is tracked as `TASK-0052` (ingressd) and `TASK-0053` (.nxra).
- UI v1 is tracked as `TASK-0054` (host renderer + snapshots) and `TASK-0055` (OS windowd + IPC + markers).
- UI v2 is tracked as `TASK-0056` (present scheduler + input) and `TASK-0057` (text shaping + SVG pipeline).
- UI v3 is tracked as `TASK-0058` (layout + wrapping) and `TASK-0059` (clip/scroll/effects/IME).
- UI v4 is tracked as `TASK-0060` (tiling/occlusion/atlases/pacing) and `TASK-0061` (gestures + a11y).
- UI v5 is tracked as `TASK-0062` (runtime+animation+transitions) and `TASK-0063` (virtual list + theme tokens).
- UI v6 is tracked as `TASK-0064` (WM + scene transitions) and `TASK-0065` (app lifecycle + notifications + nav).
- UI v7 is tracked as `TASK-0066` (WM split/snap), `TASK-0067` (DnD + clipboard v2), and `TASK-0068` (screencap + share sheet).
- UI v8 is tracked as `TASK-0069` (notifications v2 actions/reply/channels) and `TASK-0070` (WM resize/move + shortcuts + settings overlays).
- Windowing/Compositor v2 integration (damage regions, input regions hit-testing, deterministic screencaps/thumbs, WM-lite + alt-tab) is tracked as `TASK-0199` (host-first proofs) and `TASK-0200` (OS/QEMU wiring + selftests + docs).
- Windowing/Compositor v2.1 (GPU-ready swapchain surfaces + acquire/release timeline fences + vsync domains + HiDPI v1 + timings overlay + nx-win extensions) is tracked as `TASK-0207` (host-first surfacecore + deterministic tests) and `TASK-0208` (OS/QEMU wiring + selftests/docs).
- Compositor v2.2 (gpu abstraction stubs + async present + plane planner primary/overlay/cursor + cursor plane + basic color spaces sRGB/Linear + metrics/CLI) is tracked as `TASK-0215` (host-first contracts/tests) and `TASK-0216` (OS/QEMU wiring + selftests/docs; /state export gated).
- UI v9 is tracked as `TASK-0071` (searchd + command palette) and `TASK-0072` (prefsd + settings panels + quick settings wiring).
- UI v10 is tracked as `TASK-0073` (design system + primitives + goldens/a11y) and `TASK-0074` (app shell + adoption + modals).
- DSL v0.1 is tracked as `TASK-0075` (syntax/IR/lowering + nx dsl fmt/lint/build) and `TASK-0076` (interpreter + snapshots + OS demo/postflight).
- DSL v0.2 is tracked as `TASK-0077` (stores/reducers/effects + routes + i18n core) and `TASK-0078` (service stubs + nx dsl run/i18n extract + demo + postflight).
- DSL v0.3 is tracked as `TASK-0079` (AOT codegen + incremental/tree-shake + asset embedding + nx dsl --aot) and `TASK-0080` (perf bench + AOT demo + OS proofs/postflight).
- UI v11 is tracked as `TASK-0081` (mimed+contentd foundations), `TASK-0082` (thumbd+recentsd), and `TASK-0083` (doc picker + open/save/open-with integration).
- UI v12 is tracked as `TASK-0084` (scoped URI grants), `TASK-0085` (fileopsd + trashd), and `TASK-0086` (Files app + progress + DnD/share/OpenWith).
- Share v2 (intent-based share pipeline) is tracked as `TASK-0126` (intentsd+policy), `TASK-0127` (chooser+targets+grants), and `TASK-0128` (sender wiring + selftests/postflight/docs).
- Packages v1 (third-party app install) is tracked as `TASK-0129` (pkgr tooling + canonical manifest.nxb alignment), `TASK-0130` (bundlemgrd install/upgrade/uninstall + trust policy), and `TASK-0131` (installer UI + Files/Launcher integration + OS proofs).
- Storage/StateFS v3 slices are tracked as `TASK-0132` (strict storage error semantics contract), `TASK-0133` (quota accounting/enforcement), `TASK-0134` (named snapshots + RO mounts + GC/compaction triggers), and `TASK-0135` (Storage settings UI + nx-state CLI).
- Sandbox & Policies v1 (app capability matrix) is tracked as `TASK-0136` (policyd capability-matrix domain + foreground guards + service adapters + audit events) and `TASK-0137` (Security & Privacy Settings UI + installer approvals + audit viewer).
- Network Basics v1 (offline, deterministic control plane) is tracked as `TASK-0138` (netcfgd/dhcpcd-sim/dnsd/timesyncd + host tests) and `TASK-0139` (Settings Network page + nx-net CLI + OS markers/postflight/docs).
- Updates v1 UI/CLI (offline) is tracked as `TASK-0140` (Settings Updates page + nx update CLI + local payload selection; built on `TASK-0007`/`TASK-0036`).
- Telemetry/Crash (local, offline) follow-ups are tracked as `TASK-0141` (crash notifications + export/redaction surface over `.nxcd.zst`) and `TASK-0142` (Problem Reporter UI).
- Performance Pass v1 is tracked as `TASK-0143` (perfd tracer + Chrome Trace export), `TASK-0144` (frame pacing instrumentation + Perf HUD + nx perf), and `TASK-0145` (deterministic perf regression gates).
- NexusGfx SDK (Metal-like, apps+games) is tracked as `tasks/TRACK-NEXUSGFX-SDK.md` (track placeholder; real tasks extracted when gates/proofs are ready).
- IME/Text v2 Part 1 is tracked as `TASK-0146` (imed core + US/DE keymaps + host tests) and `TASK-0147` (OSK overlay + focus/a11y wiring + OS proofs).
- Search v2 UI (command palette surface + deep-links) is tracked as `TASK-0151` (host-first UI + tests) and `TASK-0152` (OS router + selftests/docs; perf gates gated on `TASK-0143/0144`).
- Search v2 backend (index/analyzers/ranking/sources) is tracked as `TASK-0153` (host-first engine + `nx search` CLI + tests) and `TASK-0154` (OS wiring + selftests/docs; persistence gated on `TASK-0009`).
- Search v2.1 semantic-lite layer (hashed char n-gram embeddings + tags + query expansion + hybrid BM25+cos rerank + explain + palette chips) is tracked as `TASK-0213` (host-first backend/libs/tests) and `TASK-0214` (UI+OS wiring + selftests/docs).
- Media UX v1 (mediasessd now-playing/focus + mini-player/lockscreen + sample app + `nx media`) is tracked as `TASK-0155` (host-first core + tests) and `TASK-0156` (OS wiring + selftests/docs).
- Media UX v2 (multi-session + deterministic playback clock + handoff + tray session switcher + notif actions wiring) is tracked as `TASK-0184` (host-first semantics + playerctl + tests) and `TASK-0185` (OS UI + selftests/docs; notif actions gated).
- Media UX v2.1 (deterministic audiod stub engine + focus/ducking + per-app volume/mute + mini-player + nx-media + fixtures + selftests) is tracked as `TASK-0217` (host-first audiod engine + tests) and `TASK-0218` (OS/QEMU integration; /state metrics export gated).
- DSoftBus v1 localSim (offline discovery/pairing + reliable msg/byte streams + demo app + `nx bus`) is tracked as `TASK-0157` (host-first core + tests) and `TASK-0158` (OS wiring + selftests/docs; persistence gated on `TASK-0009`, consent gated on `TASK-0103`).
- Identity/Keystore v1.1 (keystored lifecycle/rotation + non-exportable ops + attestation stub + trust store unification + `nx key`) is tracked as `TASK-0159` (host-first core + tests) and `TASK-0160` (OS wiring + selftests/docs; `/state` and entropy gated).
- Backup/Restore v1 (NBK v1 deterministic bundles + `backupd` + Settings/CLI + selftests) is tracked as `TASK-0161` (host-first NBK engine + tests) and `TASK-0162` (OS wiring + selftests/docs; gated on `/state` and keystore seal/unseal).
- SDK v1 Part 1 is tracked as `TASK-0163` (IDL freeze + deterministic codegen + gates) and `TASK-0164` (typed clients + templates + `nx sdk`).
- SDK v1 Part 2 is tracked as `TASK-0165` (nx sdk dev-tools + lints + pack/sign + CI) and `TASK-0166` (OS offline install/launch proofs; gated on packages + /state).
- Policy v1.1 (scoped grants/expiry/runtime prompts + Privacy Dashboard + audit viewer + nx-policy) is tracked as `TASK-0167` (host-first core semantics) and `TASK-0168` (OS wiring + UI/CLI/selftests; audit aligns with logd when available).
- Renderer Abstraction v1 (Scene-IR + renderer backend trait, cpu2d default, host goldens + OS present markers) is tracked as `TASK-0169` (host-first) and `TASK-0170` (OS wiring, aligned with `TRACK-DRIVERS-ACCELERATORS` UI consumer contract).
- Renderer v1 optional wgpu backend (host-only, feature-gated; parity tests vs cpu2d) is tracked as `TASK-0171` (does not affect OS/CI defaults).
- Perf v2 refresh (perfd v2 sessions/budgets/export + deterministic scenarios + CI gates + nx-perf) is tracked as `TASK-0172` (host-first perfd core) and `TASK-0173` (scenarios/gates/cli/docs; OS markers gated on scenario deps).
- L10n/i18n v1 finalization (locale resolver + Fluent/formatting + CJK font fallback + runtime switching + Settings/CLI + tests) is tracked as `TASK-0174` (host-first core + goldens) and `TASK-0175` (OS wiring + selftests/docs).
- WebView Net v1 (fixture-gated HTTP fetch + sanitizer v2 + Scene-IR WebView control + downloads pipeline + CLI + tests/docs) is tracked as `TASK-0176` (host-first sanitizer/render/goldens) and `TASK-0177` (OS services/policy/selftests + nx-web/docs).
- WebView v1.1 (history/find/CSP-strict/session storage + file chooser via content://) is tracked as `TASK-0186` (host-first webview-core + CSP strict + tests/goldens) and `TASK-0187` (OS file chooser/leases + selftests/docs; picker/grants reuse).
- WebView v1.2 (persistent history + session restore/crash recovery + CSP report persistence/viewer/export + cookie jar v0 dev toggle + download resume devnet-gated + nx-web extensions) is tracked as `TASK-0205` (host-first models/tests) and `TASK-0206` (OS/QEMU wiring; gated on /state and devnet readiness).
- Security hardening v2+ (syscall enforcement + sandbox profiles + fuzzing) is tracked as:
  - `TASK-0188` (kernel sysfilter v1: per-task syscall allowlists + rate buckets)
  - `TASK-0189` (userspace sandbox profiles: IPC/VFS allowlists + profile distribution)
  - `TASK-0190` (host-only deterministic fuzz smoke harness pack)
- Privacy Dashboard v2 (usage timeline/stats/revoke/export) is tracked as `TASK-0191` (host-first telemetry + deterministic NDAP export + tests) and `TASK-0192` (OS privacytelemd + Settings Dashboard v2 + nx-privacy + selftests/docs; gated on logd + /state).
- Networking v1 devnet (real HTTPS under dev flag, hosts-only resolver, trust roots/pinning, fetchd integration) is tracked as `TASK-0193` (host-first) and `TASK-0194` (OS-gated; requires OS networking/MMIO and rustls viability).
- DSoftBus v1.1 (secure channels + encrypted streams + file share; UDP discovery devnet-gated) is tracked as `TASK-0195` (host-first secure channels + share protocol) and `TASK-0196` (devnet-gated UDP discovery; OS gated on networking readiness).
- DSoftBus v1.1 directory/RPC/health slice (busdir + rpcmux req/resp multiplexing + keepalive health + quotas/backpressure + nx-bus + SystemUI wiring) is tracked as `TASK-0211` (host-first) and `TASK-0212` (OS/QEMU wiring + selftests/docs).
- DSoftBus v1.2 Media Remote (media.remote@1 service, remote control + transfer/group state sync, share@1 offer fallback, SystemUI cast picker, nx-media remote CLI) is tracked as `TASK-0219` (host-first protocol/orchestrator/tests) and `TASK-0220` (OS/QEMU wiring + selftests/docs; loopback default).
- Supply-chain/OTA/Store hardening v2 (sigchain envelopes + translog + SBOM/provenance + anti-downgrade enforcement) is tracked as `TASK-0197` (host-first formats/verifiers/tools/tests) and `TASK-0198` (OS enforcement in storemgrd/updated/bundlemgrd + selftests/docs; gated on /state where needed).
- Updater v2 (offline A/B + full/delta apply + health confirm/rollback + feed) is tracked as `TASK-0178` (bootctld stub service) and `TASK-0179` (updated v2 orchestration over offline feed; trust + statefs gated).
- Store v1 (offline feed + install/update/remove + Storefront UI + ratings stub) is tracked as `TASK-0180` (host-first store services + ratings + tests) and `TASK-0181` (OS Storefront + selftests + policy + docs).
- Store v2.2 purchases/licensing (offline NLT tokens + sandbox wallet + ledger/revocations + trials/refunds + parental controls + Storefront purchase flow + nx-store) is tracked as `TASK-0221` (host-first core + tests) and `TASK-0222` (OS/QEMU wiring + selftests/docs; /state gated).
- Encryption-at-rest v1 (SecureFS overlay + unlock flow + migration) is tracked as `TASK-0182` (host-first crypto/KDF/file format + tests) and `TASK-0183` (OS securefsd + UI/CLI/selftests/docs; gated on /state + keystored).
- UI v13 is tracked as `TASK-0087` (clipboard v3), `TASK-0088` (print-to-pdf + preview), `TASK-0089` (text editor), and `TASK-0090` (image viewer).
- UI v14 is tracked as `TASK-0091` (printd text-map substrate), `TASK-0092` (PDF viewer), and `TASK-0093` (Markdown viewer + export + nx md export).
- UI v15 is tracked as `TASK-0094` (text primitives UAX/bidi), `TASK-0095` (selection/caret + TextField core), `TASK-0096` (IME + OSK + candidates), `TASK-0097` (spellcheck), and `TASK-0098` (rich text widget + app).
- Text v2.1 (hyphenation, emoji cluster safety, persistent font/glyph cache under `/state`, renderer atlas upload/damage hooks, metrics overlay, nx-text tool, selftests) is tracked as `TASK-0201` (host-first correctness/perf substrate) and `TASK-0202` (OS/QEMU wiring).
- IME v2.1 (adaptive deterministic ranking, context bigrams, forget semantics, deterministic export/import, SecureFS-backed personalization store, UI/CLI/selftests) is tracked as `TASK-0203` (host-first ranker/store/export/tests) and `TASK-0204` (OS/QEMU integration; gated on SecureFS + /state).
- UI v16 is tracked as `TASK-0099` (decoders), `TASK-0100` (audiod mixer), `TASK-0101` (mediasessd + SystemUI controls), and `TASK-0102` (music/video apps + OS proofs).
- UI v17 is tracked as `TASK-0103` (permsd+privacyd), `TASK-0104` (camerad+micd), `TASK-0105` (recorderd + capture overlay), and `TASK-0106` (camera+gallery+settings privacy + OS proofs).
- UI v18 is tracked as `TASK-0107` (identityd), `TASK-0108` (keymintd+keystore+nexus-keychain), `TASK-0109` (lockd+lockscreen), and `TASK-0110` (OOBE/Greeter/Accounts + SystemUI wiring + OS proofs).
- Accounts/Identity v1.2 (multi-user + sessiond login/lock/switch + SecureFS home mount + per-user keystore binding + Greeter/OOBE wiring) is tracked as `TASK-0223` (host-first semantics/tests) and `TASK-0224` (OS/QEMU wiring; gated on SecureFS + keystore + /state).
- UI v19 is tracked as `TASK-0111` (webviewd sandbox), `TASK-0112` (contentd saveAs downloads helper), and `TASK-0113` (browser app + open-with + OS proofs).
- UI v20 is tracked as `TASK-0114` (a11yd hardening + focus nav), `TASK-0115` (screen reader + tts stub + earcons), `TASK-0116` (magnifier + filters + high contrast), `TASK-0117` (captions), and `TASK-0118` (settings + app wiring + OS proofs).
- SystemUI→DSL Migration Phase 1 is tracked as `TASK-0119` (DSL pages + bridge + host tests) and `TASK-0120` (OS wiring + nx-dsl targets + postflight + docs).
- SystemUI→DSL Migration Phase 2 is tracked as `TASK-0121` (Settings+Notifs DSL pages + bridge + host tests/a11y audit) and `TASK-0122` (OS wiring + selftests/postflight + docs).
