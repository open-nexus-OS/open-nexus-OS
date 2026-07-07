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
- [ ] **Launch-Pfad** (Plan quell-verifiziert im 0080D-Ledger):
      windowd→abilitymgr-Route (init-Wiring), abilitymgr OP_LAUNCH→execd
      (Route + Frame-Format wie `execd_spawn_image`), Marker
      `abilitymgr: launch (app=…, inst=…)`, execd-R1-Autolaunch löschen.
      Vorarbeit erledigt: `bundles/counter` registriert (Registry n=3,
      erscheint im Apps-Dropdown).
- [ ] **GET_PAYLOAD** os-lite in bundlemgrd (Payload→VMO→Cap-Move an
      Kind-Slot 7); ersetzt das embedded `.nxir` im app-host.
- [ ] **Lifecycle-Rest**: `@persist` via statefsd (suspend/restore),
      stop/crash residency (ADR-0037-Marker), Reaper-Restart-Policy.
- [ ] **R4**: payloadKind-Dispatch-Verifikation gegen eine AOT-ELF (mit 0079).
- [ ] **R3-v1-Limitationen** (im Code dokumentiert): Taps während Ack-Wait
      übersprungen; Full-Surface-Blit (Blit-by-Rect = recorded optimization,
      ADR-0042); geteilter Ack-Kanal (per-App-Kanäle bei Multi-App); kein
      Fokus-Modell/Keys/Motion-Input; keine Resize-Negotiation.
- [ ] **0080B**: Shell + Greeter in DSL (`userspace/systemui/`, Launcher aus
      der Registry, Greeter-Kontrakt, profilbewusst, Host-Snapshot-Matrix).
- [ ] **0080C**: OS-Wiring e2e (Produkt→Profil→Shell-Mount, Greeter→Login→
      Shell-Gate, Launcher-Klick→Launch→Surface), queryd-Boot-Wiring
      (RFC-0069), Postflight-Script.

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
- [ ] DSL-Demo-Fenster: kein Scroll-Body (v0.1); Reopen-Trigger nach dem
      Schließen fehlt (0076B-Ledger).

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
