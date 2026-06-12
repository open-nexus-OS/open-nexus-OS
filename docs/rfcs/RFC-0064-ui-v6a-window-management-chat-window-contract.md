# RFC-0064: UI v6a — Window Management v1 (Chat-Window + Drag) Contract

- Status: In Progress
- Owners: @ui
- Created: 2026-06-12
- Last Updated: 2026-06-12 (initial seed: Chat-Window als erste WM-Implementierung)
- Links:
  - Tasks: `tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md` (execution + proof — SSOT for stop conditions)
  - Depends on: `docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md` (scene graph rendering authority)
  - Depends on: `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md` (input routing + hit-test)
  - Related: `docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md` (animation runtime — reused by future transitions)
  - Follow-up: `tasks/TASK-0064B` (scene transitions: crossfade/slide — intentionally deferred)

## Status at a Glance

- **Phase 0 (Chat-Button + Window Model)**: ⬜ — Chat-Button neben Hamburger, `Window`/`WindowManager` structs
- **Phase 1 (Title-Bar + X-Button + Drag)**: ⬜ — Drag über Title-Bar, X schließt Window
- **Phase 2 (Integration + Proof)**: ⬜ — WM in CompositorState, Host-Tests, QEMU-Marker

Definition: "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Stop conditions and proof commands live exclusively in TASK-0064.

- **This RFC owns**:
  - `Window` struct contract (id, title, bounds, visible, z_index, scene_root_id)
  - `WindowManager` API contract (open, close, toggle, focus)
  - Title-Bar contract: Glas-Backdrop + Titel-Label + X-Close-Button
  - Drag contract: Title-Bar als Drag-Handle, Bounds-Clamping auf Display-Grenzen
  - Z-Order contract: Chat-Window > Sidebar > Proof-Panel > Wallpaper
  - Chat-Button contract: Position, Icon, Toggle-Verhalten
  - Window scene-graph subtree structure (Root Group → Title-Bar + Content-Area)

- **This RFC does NOT own**:
  - Multi-Window (nur Chat ist ein Window)
  - Resize (keine Kanten-Griffe)
  - Minimize/Maximize/Fullscreen
  - Scene Transitions (Crossfade/Slide) — TASK-0064B / RFC-0064B
  - `wm.capnp` IPC-Protokoll — kommt wenn Apps Window-Management brauchen
  - Kernel-Änderungen

### Relationship to tasks (single execution truth)

- **TASK-0064** is the SSOT for stop conditions, proof commands, plan ordering, and touched paths.
- This RFC owns the stable contracts and invariants that TASK-0064 implements.

## Context

TASK-0063 established the scene graph as sole rendering authority. Der Chat existiert als
statischer Scene-Graph-Node mit CPU-gerendertem Inhalt — aber ohne Window-Verhalten.

Das nächstliegende Bedürfnis ist kein voller Window-Manager mit IPC und Multi-App-Support,
sondern ein konkretes Window für den Chat: öffnen, schließen, verschieben, in den Vordergrund
holen. Das gibt dem WM-Layer sofort einen sichtbaren Use Case.

## Goals

- Chat wird von statischem Scene-Node zu einem **dragbaren, schließbaren Window**
- Chat-Button (neben Hamburger-Menu) öffnet/schließt das Window
- Title-Bar mit "Chat"-Label und X-Close-Button
- Drag per Title-Bar, Bounds-Clamping auf Display-Grenzen
- Z-Order: Window-Click bringt es nach vorne

## Non-Goals

- Resize, Minimize, Maximize, Fullscreen
- Multi-Window
- Scene-Transitions (Crossfade/Slide)
- IPC-Protokoll für Apps
- Kernel-Änderungen

## Constraints / invariants (hard requirements)

- **Determinism**: Window open/close/drag ist deterministisch — keine Timing-Abhängigkeiten
- **Kein Fake-Success**: Marker nur emittieren wenn das Verhalten tatsächlich stattfand
- **Bounded Resources**: Window-Zählung gecappt (aktuell 1), Drag-State ist Singleton
- **Scene Graph Authority**: Window-Nodes hängen unter `panel_container_id` im Scene Graph
- **Input Routing**: Hit-Test für Title-Bar und X-Button nutzt bestehende Input-Pipeline
- **No `unwrap`/`expect`** in Produktionspfaden
- **No Debug-Logs im Kernel**

## Proposed design

### Window struct (normative)

```rust
struct Window {
    id: WindowId,                  // unique identifier
    title: &'static str,           // "Chat"
    bounds: Rect,                  // (x, y, w, h) in display pixels
    default_bounds: Rect,          // reset position on re-open
    visible: bool,                 // scene subtree visibility
    z_index: u32,                  // 0 = wallpaper, 1 = proof, 2 = sidebar, 3 = chat
    scene_root_id: SceneNodeId,    // Group node with BoxShadow
    title_bar_id: SceneNodeId,     // BackdropFilter + title label
    close_btn_id: SceneNodeId,     // X-icon (2 Rect nodes forming an X)
    content_area_id: SceneNodeId,  // Chat content mount point
}
```

### WindowManager API (normative)

```rust
impl WindowManager {
    /// Open a window at its default_bounds. If already open, brings to front.
    fn open(&mut self, graph: &mut SceneGraph, id: WindowId) -> Result<()>;

    /// Close a window (sets visible=false on subtree).
    fn close(&mut self, graph: &mut SceneGraph, id: WindowId) -> Result<()>;

    /// Toggle: open if closed, close if open.
    fn toggle(&mut self, graph: &mut SceneGraph, id: WindowId) -> Result<()>;

    /// Bring window to top of z-order.
    fn focus(&mut self, id: WindowId);

    /// Hit-test: which window's title bar is at (x, y)? Returns None if no match.
    fn hit_test_title_bar(&self, x: i32, y: i32) -> Option<WindowId>;

    /// Hit-test: which window's close button is at (x, y)?
    fn hit_test_close(&self, x: i32, y: i32) -> Option<WindowId>;
}
```

### Drag contract (normative)

```
on_pointer_down(x, y):
  match wm.hit_test_title_bar(x, y):
    Some(win_id) → drag_state = { win_id, grab_offset: (x - win.bounds.x, y - win.bounds.y) }
    None → pass through to existing input routing

on_pointer_move(x, y):
  if drag_state:
    new_x = clamp(x - drag_state.grab_offset.x, 0, display_width - win.bounds.w)
    new_y = clamp(y - drag_state.grab_offset.y, 0, display_height - win.bounds.h)
    win.bounds.x = new_x
    win.bounds.y = new_y
    graph.set_node_position(win.scene_root_id, new_x, new_y)

on_pointer_up:
  drag_state = None
  wm.focus(dragged_window_id)
```

### Chat-Button contract (normative)

- Position: `x = display_width - GLASS_BUTTON_W - GLASS_BUTTON_RIGHT - GLASS_BUTTON_W - 8`
- `y = GLASS_BUTTON_TOP` (gleiche Höhe wie Hamburger-Button)
- Größe: 48×48 (wie Glass-Button)
- Aufbau: BackdropFilter → Rect (Hintergrund) → Icon (2 überlappende RoundedRects = Sprechblase)
- Hit-Test: Rechteck (x, y, 48, 48)
- Click: `wm.toggle(CHAT_WINDOW_ID)`

### Title-Bar contract (normative)

- Höhe: 40px (innerhalb des Window-Bounds)
- Hintergrund: BackdropFilter (Glas-Effekt, gleiche Parameter wie Sidebar)
- Label: "Chat" als Bitmap-Text (5×7 Font), zentriert links (x + 12, y + center)
- X-Button: 24×24, rechts (x + w - 32, y + 8), zwei Rect-Nodes als X
- Hit-Test Title-Bar: Rechteck (x, y, w - 48, 40) — exklusive X-Button-Zone
- Hit-Test X-Button: Rechteck (x + w - 48, y, 48, 40)

### Scene graph subtree structure (normative)

```
Chat-Window Root (Group + BoxShadow, z=3)
├── Title-Bar Backdrop (BackdropFilter)
├── Title-Bar BG (Rect, dark glass fill)
├── "Chat" Label (Text nodes via bitmap font — 5 chars × 7 rows)
├── X-Close Icon (2 Rect nodes: diagonal bars)
└── Content Area (Group, placeholder — chat_panel content mounts here)
```

### Z-Order contract (normative)

| Layer | z_index | Node |
|---|---|---|
| Chat-Window | 3 | Chat-Window Root |
| Sidebar | 2 | sidebar_id |
| Proof-Panel | 1 | proof_panel_id |
| Wallpaper | 0 | wallpaper_id |

Z-Order wird über die Reihenfolge im Scene Graph abgebildet (spätere Kinder = vorne).
`focus()` verschiebt den Window-Node ans Ende der `panel_container`-Kinderliste.

## Security considerations

- **Threat model**: Keine neuen Angriffsvektoren — Window-Drag operiert innerhalb `windowd`,
  kein externer Input außer Pointer-Events (bereits durch Input-Pipeline validiert).
- **Mitigations**:
  - Bounds-Clamping verhindert Window-Position außerhalb des Displays
  - Drag-State ist Singleton: nur ein Window gleichzeitig draggable
  - Z-Order ist bounded (max 4 Layer)
- **Open risks**: Keine.

## Failure model (normative)

- **open() wenn Window bereits offen**: Focus statt Duplikat
- **close() wenn Window bereits zu**: No-Op (kein Fehler)
- **Drag außerhalb Display**: Bounds-Clamping greift, Window stoppt an Kante
- **Drag wenn Window unsichtbar**: No-Op (Hit-Test findet keine Title-Bar)
- **X-Button außerhalb Window**: No-Op (Hit-Test schlägt fehl)
- **Kein Silent Fallback**: Jeder Fehlerpfad emittiert einen Marker oder returned Err

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p windowd -- wm
cd /home/jenning/open-nexus-OS && cargo test -p ui_v6a_host
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers

- `windowd: wm on` — WindowManager initialisiert
- `windowd: chat window open` — Chat-Window sichtbar nach open/toggle
- `windowd: chat window close` — Chat-Window unsichtbar nach close/toggle
- `windowd: chat window drag ok` — Drag abgeschlossen, bounds korrekt
- `windowd: chat button click ok` — Button-Click erkannt und verarbeitet
- `SELFTEST: ui v6 wm ok` — Selftest-Client bestätigt alle WM-Gates

## Alternatives considered

- **Abstrakter WM mit IPC**: Abgelehnt. Over-Engineering ohne sichtbaren Use Case. Chat-Window
  als Konkretisierung liefert sofort testbare Ergebnisse und zwingt den WM-Layer, echte
  Probleme zu lösen (Drag, Clamping, Z-Order) statt abstrakte Stacks zu modellieren.
- **Fenster-Titel als Scene-Node-Label**: Abgelehnt. Title-Bar ist Glas mit Bitmap-Text —
  das ist visuell konsistent mit dem bestehenden Sidebar/Button-Design.
- **Drag über beliebigen Window-Bereich**: Abgelehnt. Nur Title-Bar als Handle — das ist
  das Standard-OS-Pattern (Windows, macOS, GNOME, KDE) und verhindert Konflikte mit
  Content-Interaktion (Scroll im Chat).

## Open questions

- **Fenster-Titel zentriert oder linksbündig?** → Linksbündig (wie macOS), X rechts
- **Default-Position nach Close/Reopen?** → Letzte Position (nicht zurücksetzen auf default_bounds)
- **Transitions beim Open/Close?** → Nein, in v1 kein Animation-Fade. Kommt in TASK-0064B.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0** (Chat-Button + Window Model): `Window`/`WindowManager` structs + Chat-Button in SystemUiShell — proof: `cargo test -p windowd -- wm`
- [ ] **Phase 1** (Title-Bar + X + Drag): Title-Bar mit Label und X-Button, Drag-Mechanik — proof: `cargo test -p ui_v6a_host`
- [ ] **Phase 2** (Integration + Proof): WM in CompositorState, QEMU-Marker, Visual-Proof — proof: `just test-os`
- [ ] Task TASK-0064 linked and its stop conditions cover all phases above.
- [ ] QEMU markers from §Deterministic markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Anti-marker test: `windowd: chat window open` darf NICHT erscheinen bevor Button geklickt wurde.
