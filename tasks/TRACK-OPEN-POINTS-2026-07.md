---
title: "TRACK: Offene Punkte (Stand 2026-07-07) — Sammel-Ledger vor Phase 7"
status: Active
owner: "@ui @runtime"
created: 2026-07-07
links:
  - Masterplan-Track: tasks/TRACK-DSL-V1-DEVX.md
  - App-Plattform-Anatomie (Diskussionsergebnis): tasks/TASK-0081-app-platform-anatomy.md
---

# Offene Punkte — vollständiger Stand vor Phase 7

Jede Zeile verweist auf den Task/Ledger, der die Details führt. Diese Datei
ist der Sammel-Index, damit nichts verloren geht; abgearbeitete Punkte werden
hier abgehakt UND im jeweiligen Ledger geschlossen.

## Phase 6 / TASK-0080D (Ledger: tasks/TASK-0080D-…md)

- [ ] **User-Verify ausstehend**: R3-Klick („+" → Zahl steigt sichtbar),
      Text-Frame im App-Fenster, Drag-Fix (App- + DSL-Fenster verschieben).
- [x] **Launch-Pfad** DONE (user-verifiziert 2026-07-07; beide Routen-Fixes im 0080D-Ledger):
      windowd→abilitymgr-Route (init-Wiring), abilitymgr OP_LAUNCH→execd
      (Route + Frame-Format wie `execd_spawn_image`), Marker
      `abilitymgr: launch (app=…, inst=…)`, execd-R1-Autolaunch löschen.
      Vorarbeit erledigt: `bundles/counter` registriert (Registry n=3,
      erscheint im Apps-Dropdown).
- [x] **GET_PAYLOAD** GEBAUT 2026-07-07 (invertiertes Cap-Move: execd erstellt
      die VMO, bundlemgrd füllt + Header-last; Kind pollt Slot 7; embedded
      `.nxir` = markierter Fallback; `image_for_app` jetzt manifest-generiert).
      Runtime-Beweis = User-Klick (Postflight "payload chain").
- [ ] **Lifecycle-Rest**: `@persist`-RUNTIME-KERN DONE 2026-07-07
      (nexus-dsl-runtime::persist, NXPS v1 name-keyed, konformanz-getestet);
      OS-Wiring bewusst offen (keine App deklariert STATE/@persist — Grant
      wäre Cap-Leak; landet mit 0080C Schritt 4). Stop/crash residency +
      Reaper weiterhin offen (async Exit-Protokoll, boot-iteriert).
- [ ] **R4**: payloadKind-Dispatch-Verifikation gegen eine AOT-ELF (mit 0079).
- [ ] **R3-v1-Limitationen** (im Code dokumentiert): Taps während Ack-Wait
      übersprungen; Full-Surface-Blit (Blit-by-Rect = recorded optimization,
      ADR-0042); geteilter Ack-Kanal (per-App-Kanäle bei Multi-App); kein
      Fokus-Modell/Keys/Motion-Input; keine Resize-Negotiation.
- [x] **0080B** HOST-DoD DONE 2026-07-07 (userspace/systemui/ Shell+Launcher+
      Greeter, 7 Host-Proofs, Docs; Icons-Pass deferred → 0080C-Mount;
      Details im 0080B-Ledger). Dabei Compiler-Fix: set_root_canonical
      Single-Segment-Assert bei großen Programmen.
- [ ] **0080C**: OS-Wiring e2e — TEILE DONE 2026-07-07: Postflight-Script
      `tools/postflight-systemui-bootstrap-shell.sh` (Basis grün, Klick-Lane
      PEND, Wiring-Stufen SKIP), shell.toml `dsl_root` → userspace/systemui/
      shells/desktop. OFFEN: Shell/Greeter-Mount-Swap (Boot-Verify 1/2),
      Launch-e2e-Selftest, queryd-Boot (blockiert auf nexus-idl-runtime
      no_std-Konvertierung — Feature-Unification-Risiko, eigener Zug).

## Phase-5-Reste (Ledger: tasks/TASK-0078-…md / TASK-0078B-…md)

- [ ] `add service|test`-Generatoren; `session inspect|clear|export`-Verben.
- [ ] i18n-Coverage-Lint; Transcript-Staleness-Warnung; NX0407/NX0409 →
      Errors (Async-Recipe-Posture-Abschluss); `run --record`-Flag
      (Recorder existiert in `nexus-dsl-runtime::svc`).

## Querschnitts-Befunde

- [ ] **netstackd erreicht sein Entry nie** (vorbestehend, Baseline-bewiesen;
      headless-Proof-Ladder dadurch für ALLE rot: ota-Phase
      `netstackd: ready` fehlt). Kein Output, nicht mal der Panic-Handler;
      Stack 8→32 Pages ohne Wirkung; init meldet up (exec_v2+resume ok).
      Diagnose braucht Kernel-Build mit `exec`-Debug-Log.
      (Details: TASK-0080D-Ledger „SEPARATE PRE-EXISTING FINDING".)
- [ ] **NEU (#102-Familie): Blocking-recv-Wake für exec'd-Kinder tot** —
      ein Kind in `Wait::Blocking` recv wird vom Sender nie geweckt (Beweis:
      Boot 2026-07-07T12-12, zugestellte OP_SURFACE_INPUT-Frames, Empfänger
      still; app-host-Workaround = Wait::Timeout(30ms)-Loop). Kernel-Fix:
      Sender-Wake-Pfad für U-Task-Kinder prüfen (park/wake in sys_ipc_recv).
- [ ] **#102-Folge**: den stillgelegten Selftest-Exec/exit0/Minidump-Chain
      auf dem Resume-Fix wiederherstellen (Root Cause: execd resumte seine
      suspended-gespawnten Kinder nie — behoben in 0080D R1).
- [ ] nxb-pack `rewrite_manifest_with_digests` droppt die v2.0-LISTEN-Felder
      (dependencies/providedServices/resources) — kopieren, sobald das erste
      Bundle sie nutzt (Kommentar an Ort und Stelle).
- [ ] `nx-dsl fmt --check` ist auf kommentierten Dateien immer dirty (der
      Formatter strippt Kommentare) — Formatter-Entscheidung nötig
      (Kommentare erhalten vs. fmt-check-Ausnahme).
- [ ] `Model.i18n_keys` = totes, nie befülltes Feld im Checker (i18n extract
      liest stattdessen die gelowerte IR) — befüllen oder entfernen.
- [ ] hidrawd yield-spin (idle_yields ~12k/Fenster) — dieselbe
      Starvations-Klasse, die netstackds Idle-QoS trifft; auf blocking/
      timed wait umstellen.
- [ ] **Design-System-Reste** (TASK-0073/0074, PAUSED-Track): Feedback-Gruppe,
      W4-Overlays (0074), boot-gated Palette/Glass/W6. Der Design-Contract
      liegt fertig in `docs/dev/design_handoff_open_nexus_os/` (54
      Components / 67 Interfaces) — Maßstab für das System-Widget-Set.
- [x] DSL-Demo-Fenster RETIRED 2026-07-07 (User: nur EIN Counter) — Mount
      bleibt als Headless-Beweis (`DSL: program loaded hash=` +
      `DSL: demo window retired (mount-only)`); Fenster-Pfad wartet auf den
      0080C-Shell-Mount.

## App-Plattform (Diskussionsergebnis 2026-07-07 → TASK-0081)

- [ ] App-Anatomie-Ziel-Layout (manifest.toml + ui/ + i18n/ + assets/ +
      native/ in `userspace/apps/<name>/`; bundles/-Konsolidierung).
- [ ] Boot-TOML: `session = greeter|auto` + `greeter = <app-id>` +
      `shell = <app-id>` in der systemui-Registry (jede App kann Shell sein).
- [ ] SDK-Crate-Kuratierung (SSOT-Liste; Audio/Video-Libs als Zielbild).
- [ ] Companion-Service-Tooling mit Qt-Ergonomie (`nx dsl add native`,
      Surface-Codegen → Rust-Skeleton + svc.*-Signaturen + Manifest-Segment).
- [ ] App-Exports / App-eigene Permission-Namespaces (Manifest v2.2
      `exports`; vermittelt-dann-direkt über abilitymgr).
- [ ] Widget-Library-Auflösung im Build (bundleType=library + dependencies).
- [ ] Chat/Search-Migration von Rust-Crates zu DSL-Apps (Chat-Track).

## Display-Wahrheit / Fake-Proof-Härtung (User-Auftrag 2026-07-07)

Anlass: intermittierender SCHWARZER Scanout nach Boot-Logo (User-Report +
1× selbst reproduziert, VNC-Frame komplett schwarz INKLUSIVE HW-Cursor)
bei KOMPLETT GRÜNER Marker-Kette — `windowd: full-window color visible`
ist die Behauptung des Compositors, niemand prüft das Display. Bisektion
über den Phase-6-Batch (Baseline / DSL-Hälfte / wiring / exec-Seite /
Registry-Seite / voller Batch, je Boot + VNC-Grab): JEDE Konfiguration
zeigt den Greeter korrekt; derselbe volle Batch war um 10:52 schwarz und
ab 11:20 mehrfach gut ⇒ NICHT batch-deterministisch, sondern die bekannte
intermittente virgl-Scanout/Present-Klasse (vgl. Memory
virgl-blur-g3-exec-flaky-hang: "Remaining intermittent single-drain
stall"). Konsequenz = bessere, display-seitige Tests:

- [x] **tools/visual-postflight.py** (NEU): RFB-Grab (ohne Fremd-Deps) vom
      QEMU-VNC + Nicht-Schwarz-Urteil (mean-luma) + PNG-Beweisbild; klare
      Fehlermeldung, die die Klasse benennt (Marker grün ≠ Display an).
      Exit 1 = schwarz, 2 = kein Display.
- [x] **Postflight `--visual`-Stufe** (POSTFLIGHT_VISUAL=1): schlägt hart
      fehl, wenn die Marker grün sind, aber das Display schwarz ist.
- [x] **gpud Scanout-Readback-Selbstcheck** (GELIEFERT, boot-bewiesen
      2026-07-07): one-shot nach erstem G4 `scanout_sample` (gl_scanout.rs,
      TRANSFER_FROM_HOST auf GL_SCANOUT_RES) → `gpud: scanout sample ok` /
      `gpud: FAIL scanout black` / `sample unavailable`. Boot-Befund: sample
      ok bei schwarzem Host-Fenster ⇒ Schwarz-Klasse war HOST-Display-Pfad.
- [x] **Selftest-Ladder-Anschluss** (GELIEFERT 2026-07-07 abends):
      `SELFTEST: display nonblack ok` direkt nach dem GEMESSENEN
      `scanout sample ok` (#98-Disziplin: Messung, keine Behauptung) +
      Postflight-Stufe „display truth (P0.3 scanout readback)" (ok/FAIL/
      SKIP-dreiwertig; 2D/mmio-Boots = SKIP, nicht FAIL).
- [ ] Bestehende Hollow-Marker-Audit fortführen ([[fake-proof-marker-audit]]):
      `windowd: full-window color visible` in die Liste (Behauptung ohne
      Scanout-Beweis).

## ARCHITEKTUR-AUFTRAG (User 2026-07-07): production-grade statt Workarounds

Direktive: Apple-Qualität, Hardening an der Wurzel, keine Brücken. Die zwei
Wurzeln, in dieser Reihenfolge:

1. **Display-Pipeline-Recovery (das wiederkehrende Schwarz)** — Diagnose:
   alle Schwarz-Boots waren ERSTE Boots nach frischem Build (kalter Cache /
   Host-IO); Warmstart-Serien (12×) immer gut. Klasse: erster großer
   virgl-Compose frisst das 500ms-Deadline-Budget → gpud bricht den Frame
   ab → REAKTIVER Compositor ohne neues Damage recomposed NIE → RT bleibt
   schwarz, Marker bleiben grün. Production-grade Fix (kein Retry-Hack):
   a) [x] GELIEFERT (2026-07-07 abends): gpud Present-Ausgang EHRLICH —
      Delta des ring-weiten `IRQ_DEADLINE_EXPIRED_COUNT` um den ganzen
      Present (service.rs OP_PRESENT_DAMAGE) fängt JEDEN Deadline-Miss,
      auch in `let _ =`-geschluckten Optionaldraws ⇒ STATUS_DEVICE_ERROR
      (NACK) + `gpud: FAIL present deadline (cmd=N)` (no-alloc Emitter);
   b) [x] GELIEFERT (gleicher Abend): windowd NACK-Pfad in
      drain_gpud_replies — Frame nicht als präsentiert gebucht, VOLLFRAME-
      Damage-Requeue (RT-Zustand nach abgebrochenem Batch ist undefiniert),
      bounded (8) + `windowd: present retry n=` / `windowd: FAIL present
      retries exhausted (n=)`; Route-Reset nur noch für echte Protokoll-
      Garbage. Pacer bleibt bei frames_in_flight>0 aktiv, damit ein NACK
      im Idle gedraint wird (nicht erst beim nächsten Input);
   c) [x] GELIEFERT: Scanout-Readback + `SELFTEST: display nonblack ok`
      + Postflight-Stufe (siehe Display-Wahrheit-Sektion oben).
   Gate OFFEN: 5 Erste-Boots-nach-Build unter Host-Last, visual-postflight
   grün (User-Lane; NACK-Pfad braucht einen echten kalten Erst-Boot).
2. **Kernel: Sender-Wake für exec'd-Kinder in blocking recv (#102-Familie)**
   — STATUS 2026-07-07 abends: **Regressionsgate GEBAUT und GRÜN.**
   a) [x] Deterministischer Reproducer/Gate: `recv-wake-probe` (neues
      no_std-Kind), von execd EINMAL nach ready gespawnt (init mintet 2
      Pairs → execd-Slots 13–16, Kind-Slots 5/6, grants-before-resume,
      armed→park→ping→woke-Handshake mit 30ms-Park-Fenster, alle Hops
      fail-loud). Boot-Verdict: `SELFTEST: exec child blocking recv wake ok`
      — der Sender-Wake eines in blocking recv GEPARKTEN exec'd-Kindes
      FUNKTIONIERT mit dem aktuellen Kernel. Die 12-12-27-These reproduziert
      sich deterministisch NICHT mehr (seither: Kernel-Hardening-Batch
      committet; oder damals andere Ursache in der Kanal-Umbauphase).
      Postflight-Stufe „recv-wake regression gate" wacht ab jetzt jeden Boot.
   b) [x] Kernel-Wake-Ausgänge LAUT: `observe_wake_outcome` druckt one-shot
      `KERNEL: FAIL ipc wake (task-not-found|enqueue-rejected)` — ein still
      verlorener Wake (Waiter gepoppt, nie enqueued) kann nicht mehr stumm
      passieren. Boot: 0 solcher Zeilen.
   c) [x] app-host Event-Loop: Timeout(30ms)-Übergang RAUS →
      `Wait::Blocking` (reaktiv, null Polls), abgesichert durchs Gate.
   d) NEUE FALLE dokumentiert (#123-Klasse): execd resumed BEVOR inits
      Wiring-Arm transferiert (Slots 4..24 leer bei 0.124s, `init:
      settingsd slots` später) — Probe wartet bounded (5s cap_clone-Poll
      auf Slot 16). Gilt für JEDEN execd-Post-ready-Cap-Zugriff.
   OFFEN: User-Klick-Test („+" im Counter → Zahl steigt, jetzt über
   blocking recv); retired Selftest-Exec-Chain-Reaktivierung (hello/exit0/
   minidump) als Folgezug.
3. Danach erst wieder Feature-Arbeit (0080C Shell-Mount auf der dann
   verlässlichen Pipeline).
