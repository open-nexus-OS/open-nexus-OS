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
| `theme.rs` | `ui/theme-tokens` (SSOT existiert) | windowd konsumiert gepushte Theme-Werte, besitzt keine Token-Tabellen |
| `app_menu.rs` | DSL-Shell-App | Apps-Menü = Shell-UI |
| `dock.rs` (+ Dock-Teile in `wm.rs`) | DSL-Shell-App | Dock = Shell-UI |
| `assets.rs` (Icons) | Shell-/Widget-Assets | UI-Assets gehören der UI |
| `systemui_shell.rs` | DSL-Shell-App | Shell-Szene = Shell |
| `compositor/{primitives,cache}.rs` (CPU-Raster/Blur-Reste) | `nexus-gfx` / `gpud` | GPU-Arbeit auf die GPU (RFC-0067 P5-Fortsetzung) |
| `compositor/runtime/anim.rs` | Shell-App / Widget | Chrome-Animationen = UI |
| `proof_panel_spec.rs`, Proof-UI in `smoke.rs` | selftest | Test-UI ist kein Compositor |

## DELETE — Legacy (ersetzt durch deklaratives Multi-Window + DSL-Apps)

**Gate: erst löschbar, wenn die DSL-Shell/der Greeter als app-host läuft (Umbau #17 2c/2d
inkl. Input-Routing zur Desktop-Surface) — sonst schwarzer Desktop.**

| Datei | Ersetzt durch |
|---|---|
| `compositor/shell_window.rs` | Fenster-Chrome = `ui/widgets/window` + Scene-Graph; Compositing = nexus-gfx-Layer |
| `compositor/desktop_layer.rs` | Topbar/Dropdown/Suche = DSL-Shell-App |
| `compositor/chat.rs`, `runtime/chat_window.rs` | Chat als DSL-App (Chat-Track) |
| `runtime/search.rs` | Search als DSL-App |
| `runtime/settings_window.rs` | Settings als DSL-App (bundle_type=settings existiert) |
| `runtime/dsl_mount.rs`, `runtime/dsl_effects.rs` | Shell als app-host (in-process-Mount retire) |
| `runtime/greeter.rs` (Avatar-Greeter) | Greeter als app-host (`userspace/apps/greeter`) |
| `runtime/shell.rs` (UI-Teile) | Shell-App; Policy-Teil → `surface_presentation` |
| `runtime/app_window.rs` (Chrome-/Sonderfall-Teile) | schrumpft auf generisches Client-Surface-Handling (VMO-Blit, Damage) |
| `windowd/build.rs` DSL-Kompilierpfad (`dsl_root`) | bundlemgrd-Payload ist der EINE Kompilierpfad |

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
