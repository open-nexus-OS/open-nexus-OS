---
title: TASK-0080C SystemUI DSL shell + greeter (OS/QEMU): boot→greeter→login→shell→launcher-click→app end-to-end
status: Draft
owner: @ui @runtime
created: 2026-03-28
updated: 2026-07-06
depends-on:
  - tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md
  - tasks/TASK-0080D-dsl-app-runtime-lifecycle-surface-contract.md
follow-up-tasks:
  - tasks/TASK-0120-systemui-dsl-migration-phase1b-os-wiring-postflight.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Shell registry wiring point (EXISTS): source/services/systemui/manifests/shells/*/shell.toml
    (`dsl_root` resolves the compiled shell program; [first_frame] dims; ADR-0035)
  - Session gate (EXISTS, TASK-0065B): sessiond authority + greeter handoff,
    docs/dev/ui/shell/session.md
  - Query service boot wiring (from TASK-0078B): source/services/queryd → nexus-init topology
    (RFC-0069 service manifest)
  - Testing contract: scripts/qemu-test.sh
---

## Context (updated 2026-07-06)

The end-to-end payoff of the whole track: the OS boots into the **DSL greeter**, login
(decided by sessiond) hands off to the **DSL shell**, and a live pointer click on a
launcher entry launches a **real app process** (0080D app-host) whose frame appears on
screen. One SystemUI shell path — the shell registry's `dsl_root` now resolves to the
compiled 0080B programs; the native greeter view is replaced (same sessiond contract).

Boot-gated throughout: three user boot-verifies are expected (greeter+shell visible,
launch e2e, selftest suite green).

## Goal

1. **Shell/greeter mount wiring**: build pipeline compiles `userspace/systemui/`
   programs to `.nxir` in the image; systemui resolves the product → profile → shell
   chain (existing registry code) and mounts via the 0076B in-compositor path;
   `[first_frame]` respected; device.* env fed from the resolved profile (the
   registry-derived `DeviceEnv` impl).
2. **Session gate**: boot shows the DSL greeter; sessiond authenticates; handoff to
   the DSL shell per the TASK-0065B contract (authority unchanged, markers align with
   the existing session chain).
3. **Launch e2e**: live QEMU pointer hover/click on a launcher entry →
   `abilitymgr` launch → execd spawns app-host → app surface visible (ADR-0042
   transport) → focus/return behavior deterministic.
4. **queryd boot wiring** (from 0078B): service enters the nexus-init topology
   (RFC-0069 manifest entry, slot/grant discipline per the bootstrap-ordering rules);
   `@persist` restore path live via statefsd.
5. Selftests + postflight `tools/postflight-systemui-bootstrap-shell.sh`.

## Non-Goals

- Quick settings/notifications/media migration (TASK-0119/0120+). Multi-user/lock
  screen surfaces (0109/0110 track). New session semantics. Kernel changes.

## Constraints / invariants (hard requirements)

- **One shell path** — the DSL shell replaces the native shell view in the default
  product; feature flag only as a bounded migration aid (removed in TASK-0120).
- Launch/login success markers require **live routed input** (0080C's defining rule:
  no selftest-only mutation, no fake proof).
- Existing boot marker chain (reveal, session) stays intact; new markers additive.
- Bootstrap-ordering discipline: queryd + app-host follow the pre-grant rules
  (empty-waitset-park lessons); no early-recv hazards.
- No `unwrap/expect`; no godfiles.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — required (user boot-verify ×3)

UART marker chain (order-tolerant within stages):

- `DSL: greeter visible` → sessiond login chain (existing markers) →
  `systemui: dsl shell on` → `systemui: dsl launcher visible`
- hover: `systemui: dsl launcher hover visible`
- click → `abilitymgr: launch (app=<id> …)` → `APPHOST: mounted <id>@<ver>` →
  `WINDOWD: surface presented id=<n>` → `launcher: app frame visible`
- `queryd: ready` in the boot chain; `@persist` restore marker for the demo app
- `SELFTEST: systemui live launcher click ok`,
  `SELFTEST: systemui bootstrap greeter ok`, `SELFTEST: dsl app launch e2e ok`

Visual proof:

- greeter → login → shell → launcher hover state → click → visible app window, all
  with the live host pointer in the QEMU window; 0 faults; boot timing not regressed
  (reveal chain unchanged).

### Docs — required

- `docs/systemui/dsl-migration.md` phase record; `docs/dev/dsl/runtime.md` OS-mount
  section final; `docs/dev/ui/shell/session.md` notes the DSL greeter view.

## Touched paths (allowlist)

- `source/services/systemui/` (mount wiring, registry-derived DeviceEnv), image/build
  wiring for shell `.nxir`
- `source/init/nexus-init/` (queryd topology entry), `source/services/queryd/`
- `source/apps/selftest-client/`, `tools/postflight-systemui-bootstrap-shell.sh` (new)
- `docs/systemui/dsl-migration.md`, `docs/dev/dsl/runtime.md`,
  `docs/dev/ui/shell/session.md`

## Plan (small PRs)

1. shell `.nxir` build wiring + registry resolution + DSL shell mount [boot-verify 1]
2. greeter swap behind the session gate [rides with 1 or own verify]
3. launch e2e via app-host + focus/return [boot-verify 2]
4. queryd topology + @persist + selftests + postflight [boot-verify 3]

## STATUS / PROGRESS LEDGER (2026-07-07)

Partial delivery (autonomous phase-6 batch, uncommitted):

- **Postflight script DONE**: `tools/postflight-systemui-bootstrap-shell.sh`
  — stage ladder over the newest uart.log; base stages green on the current
  boot; live-click stages report PEND (never fake-passed); unlanded wiring
  stages report SKIP with their gating step. Handles interactive verdict
  FOLDING (`OK/WARN <svc>` accepted where raw markers fold).
- **Registry truthing DONE**: desktop `shell.toml` `dsl_root` now points at
  the real 0080B tree (`userspace/systemui/shells/desktop`).
- **OPEN (boot-verify lanes, in plan order)**:
  1. shell `.nxir` build wiring + mount via the 0076B in-compositor path
     [boot-verify 1] — the compiler-side blocker (single-segment abort on
     shell-sized programs) was fixed with 0080B.
  2. greeter swap behind the session gate.
  3. launch e2e selftest markers (`SELFTEST: systemui live launcher click ok`).
  4. queryd topology: BLOCKED on a no_std conversion of nexus-idl-runtime
     (its capnp dep is std-only today; feature unification would poison the
     riscv graph) — do that conversion as its own gated step, THEN the
     os-lite queryd loop (server.rs is alloc-clean except std::collections).
     `@persist` OS wiring rides here (runtime core landed with 0080D).

### 2026-07-07 abends (Closure-Plan P0.1/P1.1/P1.2, uncommitted)

- **P0.1 Layout-Hardening**: kmain-Layout-Assert (`KERNEL: layout ok` mit
  image_end/pool/headroom-WERTEN + LAYOUT:-Fehlerpfade + <64K-Warnung);
  StackPool-Cursor-Korruptions-Diagnose mit Wert; VMO-Pool-Erschöpfung
  jetzt permanent log_error (bootet grün — die 14:3x-StackExhausted-Fails
  waren NICHT diese Zeile; Klasse jetzt mit Tripwires bewaffnet);
  `scripts/contract-image-layout.sh` Perturbations-Gate (Pad braucht
  #[no_mangle] gegen Linker-GC; Landeprüfung image_end≠baseline = offener
  Feinschliff).
- **P1.1 Event-Kanal komplett re-applied** (execd+app-host-Hälften laut
  0080D-Ledger Runde 2, Timeout(30ms)-Übergangsloop bis P0.2): Boot 0 Fails.
  USER-VERIFY: „+"-Klick.
- **P1.2 = 0080C SCHRITT 1 LIVE**: windowd build.rs löst `dsl_root` aus der
  Registry (shells/desktop/shell.toml) und kompiliert das Shell-Projekt via
  compile_project_dir; Fenster öffnet als „Shell" mit Marker
  `systemui: dsl shell on` (hash cc27bc354c380b0f); Postflight-Stufe aktiv.
  Texte zeigen i18n-Keys (IdentityLocale) bis P2.3-Kataloge. Counter-Demo-
  Embed in windowd damit ERSETZT durch die Shell (ein Programm, Registry-
  getrieben).
- OFFEN hier: Greeter-Swap (Schritt 2), Launch-e2e-Selftests (Schritt 3),
  Vollflächen-Shell statt Fenster (mit Fokus-/Layer-Arbeit), P0.3-Recovery
  (meine VNC-Lane zeigt weiterhin host-klassiges Schwarz bei grüner Kette).

### P0.3-Kernstück (gleicher Abend, uncommitted): Display-Wahrheit LIVE

`gpud: scanout sample ok` — one-shot Readback der LIVE-Scanout-RT
(GL_SCANOUT_RES 0xE0) von der Host-GPU nach dem ersten erfolgreichen
Present (gl_scanout.rs `scanout_sample` + service.rs-Report). BEFUND: die
angezeigte Fläche ENTHÄLT Pixel ⇒ das Guest-Rendering ist korrekt; das
seit nachmittags beobachtete Schwarz (User-GTK + VNC) liegt im
HOST-Display-Pfad. Diagnose-Regel ab jetzt: `scanout sample ok` + schwarzer
Schirm = Host-Lane (QEMU/GL), `FAIL scanout black` = Guest-Compose. OFFEN
P0.3: Present-NACK + Damage-Requeue (transiente Guest-Fälle), SELFTEST
`display nonblack ok`.

### P0.3 KOMPLETT (2026-07-07 abends, uncommitted): Present-NACK + Requeue + SELFTEST

Closure-Plan P0.3 a–c geliefert (ADR-0032-Addendum dokumentiert den Kontrakt):

- **a) gpud Present-NACK**: `OP_PRESENT_DAMAGE` snapshottet das ring-weite
  `IRQ_DEADLINE_EXPIRED_COUNT` um den GANZEN Present; Delta > 0 ⇒
  `STATUS_DEVICE_ERROR` + `gpud: FAIL present deadline (cmd=N)` (no-alloc
  Emitter). Das Counter-Delta ist der eine Seam, den ALLE Deadline-Pfade
  teilen — auch `let _ =`-geschluckte Optionaldraws und die abandon/reset-
  Recovery von `alloc_free_slot`/`wait_slot`, die bewusst Erfolg zurückgibt.
- **b) windowd Requeue**: drain_gpud_replies unterscheidet jetzt Present-NACK
  (n≥5, status≠OK) von Protokoll-Garbage: NACK ⇒ note_present_nacked —
  in-flight-Slot frei + seq advance (Watchdog bleibt für echte No-Reply-
  Stalls), VOLLFRAME-Requeue (RT nach abgebrochenem Batch undefiniert),
  bounded 8 + `windowd: present retry n=` / `windowd: FAIL present retries
  exhausted (n=)`; sauberer Ack resettet das Budget (note_present_acked_clean).
  Client-Reset nur noch bei Garbage. Pacer: `frames_in_flight() > 0` hält den
  120Hz-Pacer an, damit ein NACK im Idle binnen eines Ticks gedraint wird.
- **c) SELFTEST-Anschluss**: `SELFTEST: display nonblack ok` direkt nach dem
  GEMESSENEN `gpud: scanout sample ok` (#98: Messung, keine Behauptung);
  Postflight-Stufe „display truth (P0.3 scanout readback)" dreiwertig
  (ok/FAIL/SKIP für 2D-Boots) + Retry-Marker-Auswertung (Retries = Recovery
  arbeitet; FAIL nur bei erschöpftem Budget).

Beweise: windowd Host 138+2+9 grün, gpud Host 9+4+16 grün, riscv-Checks
gpud (virgl+mmio) & windowd 0 Fehler / keine NEUEN Warnungen. Boot-Gate
(Marker-Ladder + Postflight) siehe nächster Ledger-Eintrag; das volle
Plan-Gate (5 Erste-Boots-nach-Build unter Host-Last) = User-Lane.

### P0.3 Boot-Gate (gleicher Abend): 2 Boots grün + VISUELLER Beweis

Zwei frische virgl-Boots (manual--19-33-04, manual--19-34-43): Ladder komplett
grün (`KERNEL: layout ok` mit Werten, `systemui: dsl shell on`, `chain G4
scanout ok` → `gpud: scanout sample ok` → `SELFTEST: display nonblack ok`),
0 FAIL/PANIC/KPGF-Zeilen; KEINE Retry-Marker = gesunder Boot, NACK-Pfad
korrekt still. Postflight: alle Basis-Stufen OK inkl. neuer „display truth"-
Stufe (Klick-Stufen PEND, bekannte Wirings SKIP). VISUELL: visual-postflight
gegen die LIVE-VNC-Lane = **OK, mean luma 119.9** — Frame zeigt den Greeter
(Wallpaper + Avatar + Cursor). Die Host-Schwarz-Episode vom Nachmittag ist
in dieser Lane nicht mehr präsent. OFFEN (User-Lane): Plan-Gate „5 Erste-
Boots-nach-Build unter Host-Last" — der NACK-Requeue-Pfad selbst feuert nur
bei einem echten kalten Deadline-Miss; seine Buchhaltung ist os-only
(compositor kompiliert host-seitig nicht — bewusst KEIN Placebo-Unit-Test).
