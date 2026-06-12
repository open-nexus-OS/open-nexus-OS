---
title: TASK-0064 UI v6a: window management v1 — Chat-Window mit Drag, Title-Bar, Z-Order
status: In Progress
owner: @ui
created: 2025-12-23
updated: 2026-06-12 (rescope: Chat-Window als erste WM-Implementierung, kein abstrakter WM-Layer)
depends-on: [TASK-0063]
follow-up-tasks: [TASK-0064B]
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC (design contract): docs/rfcs/RFC-0064-ui-v6a-window-management-chat-window-contract.md
  - UI v5b baseline (scene graph): tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - UI v5a baseline (animation): tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
  - UI v2a baseline (present/input): tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Der aktuelle Chat ist ein statischer Scene-Graph-Node (640×640, feste Position 320,80) mit
CPU-gerendertem Inhalt. Es gibt keine Window-Verwaltung: kein Drag, kein Close, kein Focus.

TASK-0064 baut die minimale Window-Management-Schicht — aber **nicht abstrakt**, sondern
**konkret am Chat-Window**. Der Chat wird das erste echte Window mit Title-Bar, Drag,
X-Close-Button und Z-Order. Das gibt dem WM-Layer sofort einen sichtbaren, testbaren
Use Case und verhindert Over-Engineering.

Scene-Transitions (Crossfade/Slide) werden auf TASK-0064B verschoben.

## Goal

1. **Chat-Button**: Neuer Button (Sprechblasen-Icon) links neben dem Hamburger-Menu-Button.
   Klick toggled das Chat-Window open/close.
2. **Window-Modell**: `Window` struct (id, title, bounds, visible, z_index, scene_root_id)
   + `WindowManager` (open/close/toggle, focus-on-click, z-order).
3. **Title-Bar + X-Button**: Chat-Window bekommt eine Title-Bar (Glas-Hintergrund, "Chat"-Label,
   X-Close-Button rechts).
4. **Drag**: Pointer-Down auf Title-Bar startet Drag, Pointer-Move verschiebt Window,
   Pointer-Up beendet. Bounds-Clamping: Window bleibt im Display.
5. **Z-Order**: Window-Click bringt es nach vorne. Z-Stack: Chat-Window > Sidebar > Proof-Panel.
6. **Host-Tests + OS-Marker**: `tests/ui_v6a_host/` + UART-Marker + QEMU-Visual-Proof.

## Non-Goals

- Kein Resize (keine Kanten-Griffe)
- Kein Minimize/Maximize/Fullscreen
- Kein Multi-Window (nur Chat ist ein Window; Sidebar und Proof-Panel bleiben statisch)
- Keine Scene-Transitions (Crossfade/Slide) — kommt in TASK-0064B
- Kein `wm.capnp` IPC (noch kein App-WM-Protokoll nötig)
- Keine Kernel-Änderungen

## Button-Layout (top-right Ecke)

```text
┌──────────────────────────────────────────────┐
│                                      ┌──┐┌──┐│
│                                      │💬││☰││  ← Chat-Button + Hamburger
│                                      └──┘└──┘│
│     ┌─────────────────────┐                  │
│     │  Chat               │  ✕               │  ← Chat-Window (title bar + X)
│     │─────────────────────│                  │
│     │  Nachrichten...      │                  │
│     └─────────────────────┘                  │
└──────────────────────────────────────────────┘
```

- Hamburger-Button: x = display_width - 48 - 20 (bestehend)
- Chat-Button: x = display_width - 48 - 20 - 48 - 8 (links vom Hamburger, 8px Gap)
- Chat-Window Default: x=280, y=80, w=680, h=560 (zentriert im 1280×800 Display)

## Window-Modell

```rust
struct Window {
    id: WindowId,
    title: &'static str,
    bounds: Rect,          // aktuelle Position + Größe
    default_bounds: Rect,  // für Reset/Open
    visible: bool,
    z_index: u32,
    scene_root_id: SceneNodeId,      // Group-Node (Shadow)
    title_bar_id: SceneNodeId,       // Glas-Backdrop + Titel-Label
    close_btn_id: SceneNodeId,       // X-Icon
    content_area_id: SceneNodeId,    // Chat-Content
}

struct WindowManager {
    windows: Vec<Window>,
    drag_state: Option<DragState>,
    focus_id: Option<WindowId>,
}
```

## Drag-Mechanik

- `on_pointer_down(x, y)` → Hit-Test gegen alle sichtbaren Title-Bars (top-down nach Z-Order)
- Match → `drag_state = DragState { window_id, grab_offset: (x - win.x, y - win.y) }`
- `on_pointer_move(x, y)` → `win.bounds` aktualisieren, clamped to display
- `on_pointer_up` → Drag-State clearen, Window fokussieren
- Drag aktualisiert Scene-Graph-Node-Positionen

## Plan (kleine PRs)

1. **Chat-Button**: Neuer Button im SystemUiShell (Backdrop + BG + Sprechblasen-Icon).
   Host-Test: Button existiert, Hit-Test funktioniert.
2. **Window-Modell + WM**: `Window` struct, `WindowManager` mit open/close/toggle/focus.
   Host-Test: open → visible, close → !visible, focus-on-click.
3. **Title-Bar + X-Button**: Chat-Window mit Title-Bar (Glas + "Chat" + X).
   Host-Test: X-Button schließt Window.
4. **Drag**: Pointer-Events auf Title-Bar → Window verschieben.
   Host-Test: Drag um (100, 50) → bounds korrekt.
5. **Integration**: WM in `CompositorState`, Chat-Button toggled, X schließt.
   OS-Marker + QEMU-Test.
6. **Bounds-Clamping + Edge-Cases**: Window bleibt im Display, Doppel-Klick toggled.

## Stop Conditions

### Proof (Host) — `tests/ui_v6a_host/`

- Chat-Button toggled Window open/close (visible ↔ invisible)
- X-Button schließt Window
- Drag: Window um (dx, dy) → bounds korrekt
- Bounds-Clamping: Window bleibt im Display (x ≥ 0, y ≥ 0, x+w ≤ 1280, y+h ≤ 800)
- Z-Order: Window-Click bringt es nach vorne
- Doppel-Öffnen toggled korrekt (kein Duplikat)

### Proof (OS/QEMU) — UART-Marker

- `windowd: wm on`
- `windowd: chat window open`
- `windowd: chat window close`
- `windowd: chat window drag ok`
- `windowd: chat button click ok`
- `SELFTEST: ui v6 wm ok`

### Visual proof (QEMU)

- Chat-Button sichtbar neben Hamburger-Menu
- Klick → Chat-Window erscheint mit Title-Bar + X
- Drag an Title-Bar → Window folgt Maus
- X-Button → Window verschwindet
- Erneuter Klick → Window erscheint wieder an letzter Position

## Touched paths (allowlist)

- `source/services/windowd/src/systemui_shell.rs` — Chat-Button, Window-Title-Bar, X-Button
- `source/services/windowd/src/wm.rs` — NEU: WindowManager
- `source/services/windowd/src/compositor/runtime.rs` — WM-Integration, Drag-Events
- `source/services/windowd/src/interaction.rs` — Hit-Test für Title-Bar + Chat-Button
- `source/services/windowd/src/scene_graph.rs` — Node-Position-Update für Drag
- `tests/ui_v6a_host/` — NEU: Host-Tests
- `source/apps/selftest-client/` — Marker
- `tools/postflight-ui-v6a.sh` — Delegates
