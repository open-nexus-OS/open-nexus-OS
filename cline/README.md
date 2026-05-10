# Cline Workflow (Open Nexus OS)

Cline-spezifische Ergänzung zum `.cursor/` Session-System.

## Was wird automatisch gelesen?

Cline liest **zwei Rule-Dateien** automatisch zu Chat-Start:
- `.cursorrules` — Security, Build, Policy Invarianten (projektweit)
- `.clinerules` — Arbeitsablauf-Steuerung (Task-Start, Testing, Debug, Proofs, Context Budget)

## Was muss manuell bereitgestellt werden?

Zu Beginn jedes Chats/Sessions:
- `.cursor/current_state.md` — komprimierter Systemzustand
- `.cursor/handoff/current.md` (falls vorhanden) — Live-Handoff
- Die Task-Datei (`tasks/TASK-*.md`) und verlinkte RFC/ADR Contracts

**Empfohlen**: Diese Dateien als `@`-Context-Bundles bereitstellen (siehe `context_bundles.md`).

## Wie Context Budget funktioniert

Cline hat ein Token-Limit für den Chat-Kontext. Um Explosion zu vermeiden:

- **Kein `@codebase`** oder `@workspace` ohne spezifischen Grund.
- **Nur 1-3 relevante Dateien** manuell als Kontext hinzufügen.
- Bei langen Debug-Ketten: **summarisieren** (UI-Button oder `/summarize`), dann `.cursor/handoff/current.md` updaten und **neuen Chat starten**.
- Cline's `autoIncludeGitDiff`/`autoIncludeOpenFiles` sind standardmäßig aus (kein impliziter Context-Bloat).

## Wichtige Build-Befehle

```bash
# Host Development
just diag-host               # Check: host builds kompilieren
cargo test --workspace       # Alle Host-Tests

# OS Development
just diag-os                 # Check: OS Services kompilieren (RISC-V)
just dep-gate                # CRITICAL: Forbidden crates check
just test-os                 # QEMU Smoke Tests

# Vor OS-Commits
just dep-gate && just diag-os

# Legacy make commands
make build                   # Full build (Podman)
make run                     # QEMU run
make test                    # Host tests
```

## Arbeitsablauf

### 1) Task-Start
1. `.clinerules` §1 "Task-Start" lesen lassen (automatisch).
2. `.cursor/current_state.md` + `.cursor/handoff/current.md` manuell bereitstellen.
3. Task-Datei + verlinkte RFCs/ADRs manuell bereitstellen.
4. Cline arbeiten lassen — es liest automatisch `.clinerules` und `.cursorrules`.

### 2) Plan Mode
- Cline startet im Plan Mode mit dem gelesenen Kontext.
- Erst planen, dann implementieren (Contract-first, drift-free).

### 3) Implementation
- Cline hält sich an `.clinerules` §3 "Execution discipline".
- Kein Scope-Creep, kein Fake-Success, kein Kernel-Debug.

### 4) Task-Ende
- Cline führt Wrap-up gemäß `.clinerules` §4 durch.
- `current_state.md` und `handoff/current.md` werden aktualisiert.

## Was tun bei Token-Explosion?

1. UI-Button `/summarize` (oder Cline-internes Summarize).
2. `.cursor/handoff/current.md` mit aktueller Rolling-Summary updaten.
3. **Neuen Chat starten** und nur `.cursor/handoff/current.md` + Task-Datei bereitstellen.
4. Weiterarbeiten — Cline hat durch `.clinerules` + `.cursorrules` alle Regeln.

## Unterschiede zu Cursor

| Aspekt | Cursor | Cline |
|--------|--------|-------|
| Rule-Dateien | `.cursorrules` + `rules/*.mdc` (Pfad-getriggert) | `.cursorrules` + `.clinerules` (immer aktiv) |
| State/Handoff | `.cursor/` Ordner (SSOT) | `.cursor/` Ordner (SSOT, shared) |
| Context-Bundles | `.cursor/context_bundles.md` | `cline/context_bundles.md` |
| Plan Mode | Via Cursor Agent UI | Via Cline Plan/Act Toggle |
| Token-Budget | Cursor-spezifische Settings | Cline-spezifische Limits |

**`.cursor/` bleibt Single Source of Truth** für State, Handoff, und Pre-Flight-Checks. Kein Duplikat in `cline/`.
