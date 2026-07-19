---
title: TASK-0080C SystemUI DSL shell + greeter (OS/QEMU): boot→greeter→login→shell→launcher-click→app end-to-end
status: Done
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

- `docs/dev/ui/dsl-migration.md` phase record; `docs/dev/dsl/runtime.md` OS-mount
  section final; `docs/dev/ui/shell/session.md` notes the DSL greeter view.

## Touched paths (allowlist)

- `source/services/systemui/` (mount wiring, registry-derived DeviceEnv), image/build
  wiring for shell `.nxir`
- `source/init/nexus-init/` (queryd topology entry), `source/services/queryd/`
- `source/apps/selftest-client/`, `tools/postflight-systemui-bootstrap-shell.sh` (new)
- `docs/dev/ui/dsl-migration.md`, `docs/dev/dsl/runtime.md`,
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

### P0.1 Nachtrag (2026-07-07 spätabends, uncommitted): Perturbations-Gate ENTPLACEBOT

Der erste volle Gate-Lauf schlug EHRLICH fehl: `FAIL (pad4096) — pad did not
land (image_end unchanged)` — die Landeprüfung fing exakt die Placebo-Klasse,
für die sie gebaut wurde. Ursache (ELF-verifiziert, Map + compile_error-Probe):
das an diag/log.rs angehängte `#[used] #[no_mangle]`-Static KOMPILIERT zwar
in die neuron-lib, wird aber vom Linker (gc-sections) VERWORFEN — `#[used]`
garantiert nur die Objekt-Emission, nicht das Überleben des Links. Fix
(production-grade, keine Datei-Mutation mehr): kmain assert_memory_layout
trägt jetzt einen REFERENZIERTEN rodata-Pad-Anker — Größe compile-zeitlich
aus `NEURON_LAYOUT_PAD` (const-Parser über option_env!, 0 = default = null
Kosten), volatile-gelesen + Inhalts-Check (`LAYOUT: pad probe mismatch`),
Banner erweitert um `pad=N`. contract-image-layout.sh baut pro Lauf nur noch
mit gesetztem Env (cargo env-dep-tracking rebuildet den Kernel), prüft
Banner-`pad=N` UND image_end-Verschiebung. ELF-Beweis: .rodata +0x1070,
LAYOUT_PAD in der Map, .bss verschoben. Gate-Lauf-Ergebnis: siehe nächste
Zeile im Ledger.

### P0.1 Gate-Ergebnis (gleicher Abend): ALL GREEN

`scripts/contract-image-layout.sh` (env-Pad-Mechanismus): baseline
image_end=0x80ba2280/375K → pad4096 0x80ba3280/371K → pad8192
0x80ba4280/367K → pad65536 0x80bb2280/311K — image_end wandert exakt um
die Pad-Größe, Banner-`pad=N` bestätigt, Marker-Ladder + visible chain in
allen 4 Boots grün, KEINE Tripwires (StackExhausted/STACK-POOL/VMO-POOL/
LAYOUT:). Das P0.1-Perturbations-Gate ist damit GESCHLOSSEN; `just
contract-image-layout` = CI-Rezept. Die log_error-Zeile in VmoPool bleibt
dauerhaft drin (Plan-Forderung; VMO-Erschöpfung ist jetzt LAUT).

### 2026-07-08: `on Mount` view-lifecycle trigger GELIEFERT (uncommitted) — Effect-Host-Fundament

Diagnose beim Start des Effect-Host-Keystones (P1.3): der DSL-Greeter
(`@effect on Load` → svc.session.*) und der DSL-Launcher (`@effect on Refresh`
→ svc.bundlemgr.enumerate) haben KEINEN Auslöser — ein `@effect` läuft nur bei
Event-Dispatch, und nichts dispatcht den initialen Load. Die Sprache hatte
keinen Lifecycle-Trigger. docs/dev/dsl/patterns.md skizzierte bereits
`on Mount -> emit(...)` als Zielbild → als echtes Feature umgesetzt (statt
Event-Namen in windowd hartzucodieren; „Architektur statt Workaround").

- **registry.rs**: `Mount` in `TRIGGERS` (Checker akzeptiert `on Mount`).
- **runtime `View::fire_mount(host)`**: feuert alle `Mount`-Handler der aktiven
  Seite GENAU EINMAL pro Route (Guard `mounted_page` = NavEntry.page); ein
  reiner Re-Render (der `handlers` neu aufbaut) feuert NICHT erneut; Navigation
  zu einer neuen Seite feuert deren Mount-Effekt. Dispatches werden VOR dem
  Feuern gesnapshottet (dispatch baut `handlers` um). Nur `Dispatch`-Aktionen
  sind lifecycle-gültig. Host ruft `fire_mount` nach mount + nach jeder
  Navigation (idempotent).
- **Test** (dsl_goldens/scenes `on_mount_fires_the_load_effect_exactly_once`):
  `on Mount -> dispatch(Load)` + `@effect on Load { svc.library.list }` +
  CountingHost → Service EINMAL getroffen, State aktualisiert, zweiter
  fire_mount = kein Re-Fire (Damage::None). syntax.md „View lifecycle".
- Beweise: dsl_goldens 13, runtime 20, core, conformance 6+3 grün; runtime
  riscv no_std 0 Fehler.

OFFEN (nächste Schritte des Keystones): windowd `DslEffectHost` (svc.session.*
→ session_client, svc.bundlemgr.enumerate → registry_client, svc.ability.launch
→ launch_app); `.nx` von Greeter/Launcher um `on Mount -> dispatch(Load/Refresh)`
ergänzen; `fire_mount` in den windowd-Mount-Loop; List<Str>/List<Record>-
Rendering; dann Boot-Verify Greeter-Users + Launcher-Apps.

### 2026-07-08: windowd DslEffectHost — svc.* calls REAL, boot-verified (uncommitted)

Zweiter Schritt des Keystones (nach [[on Mount]]): der In-Compositor-DSL-Mount
läuft nicht mehr auf `NoIo`, sondern auf `DslEffectHost` (neue Datei
compositor/runtime/dsl_effects.rs), der den Service-Seam an windowds VORHANDENE
Routen adaptiert:
- `svc.bundlemgr.enumerate(q)` → `registry_client::fetch_app_menu()` →
  `List<Record{id,label}>` (field-sorted; id/label-Syms aus der Symboltabelle).
- `svc.ability.launch(id)` → Intent gemerkt, NACH dispatch von `drain_dsl_launches`
  über `launch_app(id)` ausgeführt (launch_app braucht `&mut self`, das der
  gemountete `View` während des Effekts borgt — daher Queue statt Direktaufruf).
- `svc.session.users/login` → `session_client` (für den Greeter-Swap bereit).
Jeder Call druckt einen EHRLICHEN Marker mit Ergebnis-Count.

Wiring (dsl_mount.rs): `dsl_pointer_body` = pointer(host) → `fire_mount(host)`
(feuert den on-Mount-Load der neu-navigierten Seite) → drain launches → damage;
`boot_fire_dsl_mount` für die Boot-Route (no-op bei ShellPage, bereit für den
Greeter). LauncherPage.nx: `on Mount -> dispatch(Refresh)`.

BOOT-BEWEIS (manual--2026-07-08T09-02-59): Login → DSL-Shell → „Apps"-Klick →
navigate(/launcher) → `windowd: dsl svc bundlemgr.enumerate ok (n=3)` → Launcher
rendert die ECHTEN Registry-Apps (Card „Chat" sichtbar aus bundlemgrd). „Chat"-
Card-Klick → `windowd: dsl svc ability.launch(chat)` → `windowd: launch request
app=chat` → `abilitymgr: launch (app=chat, inst=1)` — die komplette RFC-0065-
Kette aus einer DSL-App. Beweise: windowd 138+2+9, dsl_goldens 13, systemui 7
grün; riscv windowd 0 Fehler.

OFFEN (Folge-Increments): Greeter-Swap (session.users/login sind bereit — Greeter
mounten + session-gaten + on Mount->Load); i18n-Kataloge (Text zeigt noch
launcher.*/shell.*-Keys, P2.3); TextField-Eingabe (Suche/Secret); Card-Label-
Rendering für alle Container (collect_texts deckt Stack/Grid; Card rendert schon).

### 2026-07-08 (KORREKTUR): `on Mount` war React-Denken — durch Wurzel-Effekte ersetzt

User-Einwand: `on Mount` ist ein Lifecycle-Hook (React `useEffect`/
componentDidMount) — genau der „zweite Effekt-Auslöser", den principles.md §5
verbietet („one effect model, no alternates"; „writing the obvious program
produces a well-architected app … makes violating them impossible"). Komplett
zurückgebaut (registry TRIGGERS, View::fire_mount, LauncherPage-Hook,
syntax.md, Test — alles net-zero).

RICHTIG (deklarativ, aus dem Datenfluss abgeleitet, keine Syntax): ein Event
mit `@effect`, das von NICHTS dispatcht wird (kein Handler, kein Reducer, kein
anderer Effekt), ist eine WURZEL — es kann nur beim Mount laufen, also führt
die Runtime es EINMAL beim Mount aus. `@effect on Load` ohne Dispatcher lädt
einfach; kein Lifecycle-Code im `.nx`. Ein handler-dispatchtes Event (z.B.
Greeter-`Submit` vom Login-Button) ist KEINE Wurzel → feuert nicht beim Mount
(kein Auto-Login).

Umsetzung: neues `nexus-dsl-runtime::initial` — statische IR-Analyse beim
Mount (Effekt-Trigger minus alle Dispatch-Ziele in Effekt-Steps + Handlern
aller Komponenten); `View::run_initial_effects(host)` feuert die Wurzeln
einmal; Host ruft es nach dem Mount. windowd `run_dsl_initial_effects` ersetzt
`boot_fire_dsl_mount`; `dsl_pointer_body` ohne fire_mount.

BOOT-BEWEIS (manual--2026-07-08T09-35-33, OHNE Klick): `systemui: dsl shell on`
→ `windowd: dsl svc bundlemgr.enumerate ok (n=3)` — die Launcher-`@effect on
Refresh` (Wurzel) lud die echten Registry-Apps beim Mount automatisch. 0
FAIL-Zeilen, alle P0-Gates grün. Test: dsl_goldens
`root_effect_loads_at_mount_without_a_lifecycle_hook` (Load feuert, Submit
nicht). Suiten: dsl_goldens 13, runtime 20, core, conformance grün; riscv
runtime+windowd 0 Fehler.

### 2026-07-08: Shell/Greeter nach userspace/apps/ verschoben (Architektur-Korrektur)

User-Einwand: systemui ist der SELEKTOR (Manifest bestimmt WELCHE UI), enthält
aber keine UI. Shell und Greeter sind APPS → `userspace/apps/` (wie counter/
chat/search), nicht `userspace/systemui/`. windowd = Compositor, nicht Shell-
Host (der In-Compositor-Mount bleibt vorerst Bootstrap).

Umzug (git mv, quell-erhaltend — Shell-programHash unverändert cc27bc35):
- `userspace/systemui/shells/desktop/` → `userspace/apps/desktop-shell/` (+ manifest.toml, caps=[WINDOW,LAUNCH,ENUMERATE]).
- `userspace/systemui/greeter/` → `userspace/apps/greeter/` (+ manifest.toml, caps=[WINDOW,SESSION]).
- `userspace/systemui/` gelöscht; Cargo.toml-exclude-Eintrag entfernt.
- shell.toml `dsl_root = "userspace/apps/desktop-shell"`; product.toml `greeter = "greeter"` (App-ID; systemui-Service modelliert shell/greeter schon als Registry-IDs).
- Host-Test-Basis + Docs (patterns/session/dsl-migration) auf neue Pfade.

Der systemui-SERVICE war schon korrekt (product → profile+shell/greeter als
IDs, resolve über registry); nur die Projektbäume lagen physisch falsch.

BOOT-BEWEIS (manual--2026-07-08T09-48-38): `DSL: program loaded hash=cc27bc35`
(identisch) → `systemui: dsl shell on` → `dsl svc bundlemgr.enumerate ok (n=5)`
— Shell mountet aus der neuen Location; die 2 neuen App-Manifeste tauchen jetzt
in der Registry auf (n 3→5). 0 FAILs. Host: windowd 138, systemui 19,
shell-host 7, dsl-core 28 grün; riscv windowd baut die Shell aus neuem dsl_root.

OFFEN (nächste Schritte des Umbaus): DslEffectHost → app-host (per-Cap-Routing);
Shell/Greeter als echte Bundles via RFC-0065 launchen; windowd-dsl_mount
zurückbauen; Shell/Greeter aus der Launcher-Enumerate ausblenden (system/hidden
flag — sie sollen nicht user-launchbar in der App-Liste erscheinen).

### 2026-07-08: bundle_type = shell/greeter als Privilegien-Decke (3 Schritte)

User: der eigene bundle_type soll (a) Shell/Greeter aus der App-Liste
eindeutig raushalten UND (b) ihnen Zugriff auf privilegierte Caps geben, den
normale Apps NICHT haben — bei erhaltener Flexibilität (Autologin ohne User,
beliebige App als Single-App-Shell). Kern-Trennung: **bundle_type = Privilegien-
DECKE (was das Bundle anfordern darf), product = Rollen-ZUWEISUNG (welche App
die Rolle spielt).**

- **Schritt 1 (DONE, boot-verifiziert n 5→3):** BundleType-Enum um `shell @5` /
  `greeter @6` erweitert (manifest.capnp); desktop-shell/greeter-Manifeste auf
  die Typen; bundlemgrd build.rs führt bundle_type in APP_REGISTRY, os_lite
  `build_list_apps_response` filtert auf `bundle_type == "app"` — Shell/Greeter
  registriert + servierbar, aber NICHT in der Launcher-Liste.
- **Schritt 2 (DONE, host-getestet):** Cap-Decke fail-closed bei PACK-Zeit
  (nxb-pack): `SESSION` nur greeter, `LAUNCH`/`ENUMERATE` nur shell — eine
  `app`, die diese deklariert, wird abgelehnt. SESSION/LAUNCH/ENUMERATE zur
  abilitymgr-Known-Permissions-Liste. Test bundle_type_gates_system_role_permissions.
- **Schritt 3 (teilweise, Rest reitet auf Greeter-Swap):** systemui-Product-
  Konsistenz `auto ⟺ kein Greeter` war schon fail-closed. Der Typ-CROSS-CHECK
  (product.greeter zeigt auf greeter-type-Bundle) braucht die App-bundle_types
  aus bundlemgrd → landet mit dem Greeter-Swap (systemui resolved+launcht die
  Rollen-App). Sicherheitsboden hält trotzdem: die Pack-Decke lässt SESSION nur
  bei greeter-Bundles zu, ein fehlzeigendes `greeter =` kann NIE Login treiben.

Autologin/Single-App bleiben möglich: `session = auto` = gar kein Greeter;
`product.shell = <plain app>` (bleibt bundle_type=app, braucht keine erhöhten
Caps) für Single-App-OS. Beweise: bundlemgrd 13, nxb-pack 4, abilitymgr 31,
windowd 138, systemui 19 grün; riscv bundlemgrd/abilitymgr sauber; Boot n=3.

### 2026-07-08: bundle_type `settings` + privilegierte-Rollen-Roadmap

- **`settings`-Typ (DONE, host-getestet):** manifest.capnp `settings @7`; Decke
  `SETTINGS` nur für settings (nxb-pack); abilitymgr KNOWN_PERMISSIONS +SETTINGS;
  Launcher-Filter generalisiert auf `is_launchable = app | settings` (Settings
  IST user-startbar — anders als shell/greeter). Trennt die zwei Achsen
  Sichtbarkeit vs. Privileg sauber. Tests: nxb-pack 4, bundlemgrd 13, abilitymgr 31.
- **Roadmap-Doku NEU:** `docs/dev/app-platform/privileged-roles.md` — Survey der
  privilegierten App-Rollen (Android/iOS/OHOS) auf unser Modell gemappt: A
  System-Surfaces (shell/greeter/ime/systemui), B privilegierte startbare Apps
  (settings✓, filemanager/phone/sms/contacts/camera/browser/…), C privilegierte
  Hintergrund-Services (autofill/a11y/vpn/print/…), D Zugriffs-PORTALE
  (document/photo/contact-picker, share, scoped-grants = mediated-then-direct).
  Design-Leitplanke: eigener bundle_type nur für echte System-Apps mit Identität;
  sonst Portal (Picker) ODER gated Service-Cap. Verweist auf TASK-0083/0084/0106/
  0113/0118/0131.

### 2026-07-08: Deklaratives App-Kind-Service-Routing — Stufe 1 (SSOT-Map) DONE

User bestätigt: production-grade = deklarativ (Manifest-Caps treiben das Routing
ab, nicht Pro-Dienst-Verdrahten), fail-closed, schnell (Routen beim Launch
provisioniert → direkte IPC), dynamisch erweiterbar (SDK-Map-Zeile pro Dienst;
Laufzeit-Grants über den bestehenden mediated-then-direct-Broker). Zwei Ebenen:
statisch-deklariert (Launch-provisioniert) + Broker-für-dynamisch.

- **Stufe 1 DONE:** `source/libs/nexus-sdk-routes` (no_std, keine Runtime-capnp) —
  SSOT `ServiceRoute { svc, route, permission, child_slot }`: bundlemgr→bundlemgrd
  /ENUMERATE/11, ability→abilitymgr/LAUNCH/12, session→sessiond/SESSION/13; +
  reply-inbox slots 9/10; Lookups route_for_svc/route_for_permission; Self-
  Consistency-Test. abilitymgr dep + Guard `every_sdk_route_permission_is_known`
  (32 Tests grün, riscv sauber).

STUFE-2-ENTSCHEIDUNG (offen, boot-kritisch): Provisioning braucht eine Minting-
Autorität pro Launch (das Kind kann keinen eigenen Endpoint minten — Factory nur
für Kernel/init-Kinder; Control-Channel-Reply-Inbox teilen = Race). Optionen:
(a) init mintet die Kind-Reply-Inbox im execd-Arm (R1 single-app, wie der
    bestehende Event-Kanal), execd resolved die Service-Sends über seinen
    Control-Channel + grantet sie per SDK-Map-Slot; Multi-App = R1→R2-Folge.
(b) neuer Control-Channel-Op `PROVISION_APP_ROUTES(pid, caps)`: init mintet
    per-Launch (Multi-App sofort), execd/abilitymgr triggert.
(c) execd bekommt die Factory (Spawner mintet selbst) — mächtiger, weniger
    Kapselung.
Empfehlung: (a) zuerst (kleinster boot-sicherer Schritt, matcht Event-Kanal-R1),
(b) als Multi-App-Ausbau. execd braucht die App-Caps (build.rs-Tabelle wie
bundlemgrd/abilitymgr) um nur deklarierte Routen zu provisionieren.
