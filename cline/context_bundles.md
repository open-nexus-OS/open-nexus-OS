Diese Datei beschreibt, welche Dateien pro Phase in Cline als `@`-Kontext bereitgestellt werden sollen.

Cline hat **kein** automatisches Einblenden von Dateien basierend auf Pfad-Patterns (anders als Cursor's `.cursor/rules/*.mdc`). Stattdessen wird `.clinerules` **immer** gelesen und **alle anderen Dateien müssen manuell als `@`-Bundles bereitgestellt werden**.

**Wichtig**: `.cursor/` bleibt Single Source of Truth für State und Handoff. Diese Datei beschreibt nur, wie man sie in Cline einliest.

---

## Bundle 1: Task-Start (jeder neue Chat)

``` text
@.clinerules
@.cursorrules
@.cursor/current_state.md
@.cursor/handoff/current.md
@tasks/TASK-XXXX-*.md
@docs/rfcs/RFC-00XX-*.md  (falls in Task verlinkt)
@docs/adr/ADR-00XX-*.md   (falls in Task verlinkt)
```

**Reihenfolge**: `.clinerules` zuerst (aktiviert Task-Start-Modus), dann `.cursorrules` (Security/Build-Invarianten), dann State/Handoff, dann Task+RFCs.

## Bundle 2: Implementation (während der Task)

``` text
@.clinerules
@tasks/TASK-XXXX-*.md
@source/services/XYZ/src/lib.rs  (nur die 1-3 relevanten Dateien)
@source/services/XYZ/src/main.rs
@source/services/XYZ/Cargo.toml
```

**Nur die Dateien, die wirklich gebraucht werden.** Kein `@workspace` oder `@codebase`.

## Bundle 3: Testing/Debugging

``` text
@.clinerules
@.cursor/current_state.md
@docs/testing/index.md
@source/services/XYZ/tests/integration.rs  (nur die relevanten Test-Dateien)
```

Bei langen Debug-Ketten:
1. `/summarize` ausführen
2. `.cursor/handoff/current.md` mit Rolling-Summary updaten
3. **Neuen Chat starten** mit Bundle 1 (ohne alte History)

## Bundle 4: Wrap-up (Task-Ende)

``` text
@.clinerules
@tasks/TASK-XXXX-*.md
@.cursor/current_state.md
@.cursor/handoff/current.md
@.cursor/next_task_prep.md
@docs/rfcs/RFC-00XX-*.md  (falls erstellt/geändert)
@docs/adr/ADR-00XX-*.md   (falls erstellt/geändert)
@CHANGELOG.md
@tasks/IMPLEMENTATION-ORDER.md
@tasks/STATUS-BOARD.md
```

---

## Anti-Patterns (verboten)

- ❌ `@workspace` oder `@codebase` zu Chat-Start (Token-Explosion)
- ❌ 20+ Dateien in einem Bundle (Context-Bloat)
- ❌ Task/RFC vergessen und direkt implementieren (Contract-Drift)
- ❌ `.cursor/` Dateien duplizieren oder in `cline/` kopieren (`.cursor/` ist SSOT)
- ❌ Diffs von `git diff` automatisch einblenden lassen (Cline's `autoIncludeGitDiff` auf `false`)

## Empfohlene Cline-Settings

- `autoIncludeGitDiff`: `false` (manuell entscheiden)
- `autoIncludeOpenFiles`: `false` (manuell entscheiden)
- Bei Token-Näherung an 80%: `/summarize` + neuer Chat mit Bundle 1
