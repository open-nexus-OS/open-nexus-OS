---
title: "TASK-0081 App-Plattform-Anatomie: App-Layout (i18n/assets/native), Boot-TOML (Login/Shell), SDK-Kuratierung, Companion-Services, App-Exports, Widget-Libraries"
status: Draft
owner: "@ui @runtime"
created: 2026-07-07
depends-on:
  - tasks/TASK-0080D-dsl-app-runtime-lifecycle-surface-contract.md   # App-Runtime-Prozess + Transport
  - tasks/TASK-0080B-systemui-dsl-bootstrap-shell-launcher-host.md   # Shell/Greeter in DSL
links:
  - Diskussions-/Entscheidungsprotokoll: ~/.claude/plans/sprightly-wobbling-grove.md (2026-07-07)
  - Offene-Punkte-Index: tasks/TRACK-OPEN-POINTS-2026-07.md
  - App-Layout heute: docs/dev/dsl/project-layout.md
  - Manifest-Schema: tools/nexus-idl/schemas/manifest.capnp (v2.1)
  - Service-Surface-SSOT-Muster: tools/nexus-idl/schemas/dsl_services.capnp
  - systemui-Registry (Boot-TOML-Träger): source/services/systemui/manifests/ (ADR-0035)
  - Lifecycle-Authority: docs/rfcs/RFC-0065, docs/adr/0036
  - Design-Contract (Widget-Maßstab): docs/dev/design_handoff_open_nexus_os/
---

## Context (Entscheidungen vom 2026-07-07, User-approved)

Phase 6 hat den App-Runtime-Prozess bewiesen (eigene Surface, Text, Input).
Dieser Task definiert die VOLLE App-Anatomie darauf — festgelegt wurden:

1. **System-SDK-Libraries**: Apps nutzen systemweite Libs DIREKT (statisch
   gelinkt) — eine KURATIERTE SDK-Crate-Menge (SSOT-Liste wie
   dsl_services.capnp): heute nexus-gfx, nexus-text-baked, nexus-layout,
   nexus-virtual-list, nexus-dsl-runtime; Zielbild nexus-audio/nexus-video
   (Codecs/DSP). Geräte-/Zustands-Zugriff (Ausgabe, Kamera, Present, Dateien)
   bleibt Service + Cap. Rohe Syscall-/Kernel-nahe Crates sind NICHT
   SDK-public. Versionierung über das vorhandene `min_sdk`-Feld.
2. **Rust-Interop = Companion-Service mit Qt-Ergonomie**: `native/` ist eine
   normale Rust-Crate im App-Ordner; das Tooling macht daraus AUTOMATISCH
   einen eigenen Prozess (eigenes Manifest-Segment, eigene Caps, eigener
   Spawn). Der Entwickler deklariert die Surface EINMAL; `nx dsl` generiert
   (a) das Rust-Server-Skeleton in `native/`, (b) die `svc.<app>.<method>()`-
   Signaturen für den DSL-Checker, (c) den Manifest-Service-Eintrag.
   Transcript-Tests funktionieren unverändert. AOT-ELF (`payloadKind = elf`)
   bleibt die zweite Schiene (0079) für Extremfälle (Videorendering).
3. **App-Exports: vermittelt, dann direkt**: abilitymgr prüft
   Export-Deklaration + Consumer-Caps (App-eigene Namespaces
   `app.<bundle>.<CAP>`) fail-closed, startet die Ziel-App bei Bedarf, mintet
   den Endpoint-Pair — danach direkte IPC. Kein Broker im Datenpfad.
4. **Widget-Libraries: build-zeitlich + kuratiert**: Library-Bundles
   (`bundleType = library`) werden via `dependencies` beim App-Build
   aufgelöst und ins EINE `.nxir` kompiliert (kein Laufzeit-Loading; das
   Ein-Programm-ein-Hash-Modell und die AOT-Parität bleiben). Volle
   Checker/Lints; nur Komposition der System-Primitives, keine eigenen
   Modifier/Primitives.
5. **Konsolidierung**: `userspace/apps/<name>/` trägt manifest.toml + ui/ +
   i18n/ + assets/ (+ native/); `bundles/` verschwindet.

## Goal

1. **App-Ziel-Layout** (project-layout.md wird normativ erweitert):

   ```
   userspace/apps/<name>/
     manifest.toml            # Identität, caps, payload_kind, exports, dependencies
     ui/pages/**.nx           # bestehendes ui/-Layout (project-layout.md)
     ui/components/**.nx
     ui/composables/**.store.nx
     ui/services/**.service.nx
     ui/platform/<profile>/**
     i18n/<locale>.json       # nx-dsl i18n extract/compile (CLI existiert)
     assets/**                # Bilder/Icons/Sounds → manifest `resources`,
                              # DSL Image/Icon → IR-AssetRef (Wiring neu)
     native/ (optional)       # Companion-Rust-Crate (Entscheidung 2)
   ```

2. **Boot-TOML** (systemui-Registry, ADR-0035): `product.toml` erhält
   `session = "greeter" | "auto"`, `greeter = "<app-id>"` (App implementiert
   den Greeter-Kontrakt: svc.session.*), `shell = "<app-id>"` — jede App kann
   Shell sein (Single-Purpose-OS/Kiosk); Authority unverändert (sessiond
   entscheidet, abilitymgr launcht, windowd mountet).
3. **SDK-SSOT**: `docs/dev/sdk/crates.md` + maschinenlesbare Liste (Quelle
   für Lint/CI: eine App-`native/`-Crate darf nur SDK-Crates + eigene Deps
   ziehen — RFC-0009-Dep-Gate-Muster erweitern).
4. **Companion-Tooling**: `nx dsl add native <name>` scaffolds `native/`
   (Crate + Surface-Deklaration); Build generiert Skeleton + svc-Signaturen +
   Manifest-Segment; execd/abilitymgr spawnen den Companion beim App-Launch
   mit dessen eigenen Caps (fail-closed).
5. **App-Exports** (Manifest v2.2, append-only): `exports = [{ ability,
   permission }]`; abilitymgr-Vermittlung (resolve → Grant-Check beide Seiten
   → ggf. Launch → Endpoint-Pair-Mint → direkte IPC); Consumer-Seite erhält
   generierte `svc.app_<bundle>.<method>()`-Signaturen. Referenz-Case: die
   Chat-App exponiert Send/Receive (Werbung/Support in den Chat, Antworten),
   KI-Aktionen wie „neuer Termin" laufen über denselben Mechanismus.
6. **Widget-Libraries**: `dependencies`-Auflösung im `nx dsl build`
   (Projekt-Merge kennt Library-Quellen), Determinismus-Beweis (byte-gleiche
   .nxir), Lint: Libraries ohne Modifier-/Primitive-Erweiterungen.

## Non-Goals

- Chat/Search-Migration zu DSL-Apps (eigener Track, Chat-Track).
- Dynamisches Linken / In-Prozess-Plugins (abgelehnt: bricht die
  Trust-Boundary; existiert im OS nicht).
- Laufzeit-Component-Loading (abgelehnt: bricht Ein-Programm-ein-Hash + AOT).
- Signierung/Store-Pipeline (RFC-0039).

## Constraints / invariants

- Manifest bleibt die EINZIGE Identitäts-/Caps-Quelle; alles fail-closed.
- Kein wilder Westen: Libraries/SDK nur über die kuratierten SSOT-Listen;
  DSL-Determinismus (kanonisches .nxir, ein Hash) ist unantastbar.
- Companion-Prozesse durchlaufen dieselbe Spawn-Disziplin wie Apps
  (grants-before-resume, #102-Lektion).
- Kein `unwrap/expect`; keine Godfiles; keine Firmen-/Produktnamen.

## Plan (Phasenschnitt, jeder host-first)

1. Docs-Normierung: project-layout.md (Ziel-Layout), sdk/crates.md,
   services.md-Abschnitte, manifest v2.2-Entwurf (exports) in nxb.md.
2. Konsolidierung: bundles/* → userspace/apps/*/manifest.toml; Generatoren
   (abilitymgr/bundlemgrd build.rs, nxb-pack, build.sh) auf die neue Quelle.
3. assets/ + i18n/-Wiring: AssetRef-Pipeline (Manifest resources ↔ IR),
   Katalog-Einbindung in den App-Build.
4. Companion-Tooling (add native + Codegen + Spawn-Wiring).
5. Exports/Permission-Namespaces (Manifest v2.2 + abilitymgr-Vermittlung).
6. Widget-Library-Auflösung im Build.

### Proof (Host) — je Phase

- Layout/Generatoren: `nx dsl init`-Scaffold baut; Konsolidierungs-Roundtrip
  (Registry-Tabellen identisch vor/nach Quelle-Umzug).
- Companion: Surface-Deklaration → generiertes Skeleton kompiliert; DSL-App
  ruft die generierte svc-Signatur (Transcript-Test).
- Exports: Vermittlungs-Statemachine host-getestet (Grant-Matrix fail-closed).
- Libraries: byte-deterministisches .nxir mit/ohne Library-Dep-Auflösung.

## STATUS / PROGRESS LEDGER (2026-07-07, uncommitted)

**Phase 1+2 (Docs+Konsolidierung) GELIEFERT, Boot-TOML GELIEFERT:**

- **C4 Konsolidierung DONE (host-grün)**: `bundles/` GELÖSCHT; Manifeste
  leben in `userspace/apps/<name>/manifest.toml` (chat/search neben ihren
  Crates; counter mit ui/pages/CounterPage.nx + i18n/ + assets/-Gerüst).
  NEUER SSOT-Compile-Pfad `nexus_dsl_core::compile_project_dir(root)`
  (project.rs, std-gated): ui/-Walk → merge → check → lower — von ALLEN
  vier Generatoren benutzt (bundlemgrd build.rs APP_PAYLOADS, abilitymgr
  build.rs Caps/UI-Programs, windowd dsl_demo, app-host payload) — ein
  Payload kann nie mehr vom CLI-Projektmodus abweichen.
  `examples/dsl/counter` bleibt als Lehr-Beispiel (dsl_goldens nutzt es).
  HINWEIS: programHash des counter ändert sich (sourceDigest = Projektbaum
  statt Einzeldatei) — Parität windowd↔app-host bleibt (gleicher Baum,
  gleicher Helper). Tests: bundlemgrd 27+13, abilitymgr, systemui grün;
  riscv os-lite 0 Fehler.
- **B2 Boot-TOML DONE (host-grün)**: `ProductManifest.session`
  (greeter|auto, Default greeter) + `.greeter` (App-Id, leer = eingebaute
  Greeter-View); Validierung fail-closed (auto+greeter-App = Widerspruch =
  Parse-Fehler; unbekannter Mode = Fehler). default-Produkt: explizit
  `session = "greeter"`; kiosk: `session = "auto"` (Single-Purpose bootet
  direkt in die Shell-App). 3 neue Unit-Tests; 19 systemui-Tests grün.
  PARSER-FALLE: leere String-Werte (`greeter = ""`) lehnt parse_entries ab
  — Default = Zeile weglassen. OS-Konsum (sessiond/windowd lesen
  session/greeter) = 0080C-Gate.

**OFFEN (in Plan-Reihenfolge):** assets/+i18n-Wiring (AssetRef-Pipeline),
Companion-Tooling (`nx dsl add native` + Codegen), Manifest v2.2 `exports`
(+abilitymgr-Vermittlung; nxb-pack ist zeilenbasiert — Export-Syntax
liste-tauglich designen), Widget-Library-Auflösung, SDK-SSOT-Liste
(docs/dev/sdk/crates.md + Dep-Gate).
