# windowd Cleanup-Karte — Single Present Authority (SSOT)

**Status:** Active — governing map for the windowd restructuring (Umbau #17 / RFC-0067 R3+R4).
**Owner-Direktive (2026-07-10):** windowd = **Single Present Authority** — Compositor,
Layer-Tree, Vsync/Present. Nichts anderes. Widgets/Chrome → `ui/widgets/*`; Raster → `nexus-gfx`;
GPU-Arbeit (Blur/Compositing) → `gpud`; Theme → `ui/theme-tokens` (windowd konsumiert gepushte
Werte); Shell-UI (Topbar/Dock/App-Menü/Launcher) → die **DSL-Shell-App** (`userspace/apps/desktop-shell`);
Greeter-UI → die **DSL-Greeter-App**. Alles Deklarative (Fenster-Rolle/Chrome/Resize) kommt aus
`OP_SURFACE_INTENT` ⟂ Policy über `surface_presentation.rs` — **nie** hardcodiert pro Fenster-Typ.

> **Für jeden Agenten/Committer:** Bevor du in windowd etwas anfasst: Datei unten nachschlagen.
> In eine MOVE/DELETE-Datei wird **nicht** hineingebaut — neue Fähigkeiten entstehen am Zielort.

## KEEP — der Compositor-Service (bleibt in windowd)

| Datei | Rolle |
|---|---|
| `server.rs` | Wire-Protokoll (Surface create/present/destroy/intent/layers/theme) |
| `client_surface.rs` | Client-Surface-Tabelle (N Slots, by-id, seq) |
| `surface_presentation.rs` | **Deklarativer Resolver** intent ⟂ policy → {role, chrome, fullscreen, resizable} — die EINE Policy-Stelle |
| `window_scene.rs` | Z-Order/Show-SSOT (Z-Bänder Desktop < Window < Fullscreen) |
| `scene_graph.rs` | Retained Layer-Tree → nexus-gfx CommandBuffer |
| `atlas.rs`, `app_surface.rs`, `resource_pool.rs`, `buffer.rs` | Compositor-Ressourcen |
| `compositor/runtime/{present,framebuffer,gpud,cursor,marker_emit}.rs` | Present-Pacing/Vsync, gpud-Handoff, HW-Cursor |
| `compositor/runtime/input.rs` | Input-**Routing** (Hit-Test → Client). Chrome-*Verhalten* gehört dem Widget |
| `session_client.rs`, `compositor/runtime/session.rs` | Session-Gate-Konsument (wer ist Desktop: greeter/shell) |

## MOVE — falsch platziert (Zielort)

| Datei | Ziel | Warum |
|---|---|---|
| `theme.rs` | (KORRIGIERT 2026-07-10) bleibt als dünner Konsument — die WERTE sind schon build-generiert aus `resources/themes/*.nxtheme.toml` (Value-SSOT). Die echte Dopplung: `ui/theme-tokens` hartcodiert seine Werte → dort aus denselben `.nxtheme.toml` generieren | eine Value-Quelle für Compositor UND Apps |
| `app_menu.rs` | DSL-Shell-App | Apps-Menü = Shell-UI |
| `dock.rs` (+ Dock-Teile in `wm.rs`) | DSL-Shell-App | Dock = Shell-UI |
| `assets.rs` (Icons) | Shell-/Widget-Assets | UI-Assets gehören der UI |
| `systemui_shell.rs` | DSL-Shell-App | Shell-Szene = Shell |
| `compositor/{primitives,cache}.rs` (CPU-Raster/Blur-Reste) | `nexus-gfx` / `gpud` | GPU-Arbeit auf die GPU (RFC-0067 P5-Fortsetzung) |
| `compositor/runtime/anim.rs` | Shell-App / Widget | Chrome-Animationen = UI |
| `proof_panel_spec.rs`, Proof-UI in `smoke.rs` | selftest | Test-UI ist kein Compositor |

## DELETE — Legacy (ersetzt durch deklaratives Multi-Window + DSL-Apps)

**Status 2026-07-10: AUSGEFÜHRT** (bis auf `shell_window.rs`, s.u.). Gelöscht:
`desktop_layer.rs`, `chat.rs` + `runtime/chat_window.rs`, `runtime/search.rs`,
`runtime/settings_window.rs`, `runtime/greeter.rs` (Avatar), `app_menu.rs`,
`registry_client.rs`, `runtime/dsl_mount.rs` + `dsl_effects.rs` (früher),
`build.rs`-DSL-Pfad (früher), `shell.rs`-UI-Teile (topbar/sidepanel/dropdown-
Renderer + ensure_app_menu). `input.rs`/`scene.rs`/`wm.rs` auf AppClient+Desktop
reduziert; Login-Phase = `greeter_login_watch` (session.rs). Boot-Reserven weg —
der Atlas ist ein On-Demand-Pool (Desktop-Band + Floating + Dock).

| Datei | Ersetzt durch | Status |
|---|---|---|
| `compositor/shell_window.rs` | Fenster-Chrome = `ui/widgets/window` + Scene-Graph; Compositing = nexus-gfx-Layer | **BLEIBT vorerst** — trägt das Floating-App-Fenster (Frame/Titel/Resize); Retirement = Widget-Promotion (#23 + windows-as-widgets) |
| `runtime/app_window.rs` (Chrome-/Sonderfall-Teile) | generisches Client-Surface-Handling | Teilweise (schrumpft weiter mit shell_window-Retirement) |
| `window_scene.rs` Chat/Search/Settings-Varianten | Surface-Ids + Rollen (voll deklarativ) | Varianten ungenutzt; Enum-Retirement mit Multi-Window-Generalisierung |

Offen notiert: Wheel-FORWARDING an Client-Surfaces (`OP_SURFACE_INPUT` kind=wheel)
— das legacy Chat/Search-Scrolling entfiel; App-Scroll ist App-Sache.

## Reihenfolge (Stufen, je boot-verifiziert)

1. **JETZT:** Diese Karte + LEGACY-Banner in jede MOVE/DELETE-Datei (Kontaminationsschutz).
2. **2c:** Shell-app-host als Desktop-Surface inkl. Input-Routing; Greeter-app-host hinter dem
   Session-Gate. (Launch bei STATE_ACTIVE ist verdrahtet.)
3. **2d:** DELETE-Spalte ausführen (dsl_mount, desktop_layer, greeter.rs, doppelter Kompilierpfad).
4. **Fenster-DSL-Migration:** chat/search/settings als DSL-Apps → ihre Legacy-Dateien löschen;
   `WindowId`-Enum fällt (voll deklarativ, nur noch Surface-Ids + Rollen).
5. **MOVE-Spalte:** theme/app_menu/dock/assets/anim in Shell-App bzw. theme-tokens; Raster-Reste
   → nexus-gfx/gpud.

Endzustand: windowd ≈ 8–9k LOC reiner Compositor-Service; alles UI deklarativ aus Apps.

## Input-Grenze (Owner-Direktive 2026-07-10)

| Schicht | Ort | Inhalt |
|---|---|---|
| Treiber | `drivers/input/virtio-input`, `hidrawd` | Raw-HID, Device-IRQs, Queues |
| Normalisierung | `inputd` (+ `userspace/{keymaps,key-repeat,pointer-accel}`) | Scancode→Key, Accel, Repeat, Display-Space-Pointer — **nie** in windowd |
| Compositor | `windowd` | NUR Hit-Test (Z-Order-SSOT) + **Routing** zur Ziel-Surface (`OP_SURFACE_INPUT`) |
| Verhalten | Widget / App (DSL) | Was ein Klick/Scroll TUT — nie in windowd |

Das per-Fenster-Input-Verhalten in `compositor/runtime/input.rs` (Chat-Filter, Search-Text,
Scroll-Sonderfälle) ist Teil der Legacy-Fenster und fällt mit der DELETE-Spalte. Endzustand:
input.rs = Hit-Test → deklaratives per-Surface-Forwarding, sonst nichts.
