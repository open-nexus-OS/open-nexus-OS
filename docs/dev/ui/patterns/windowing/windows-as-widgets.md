<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Windows are widgets, through one scene graph

Concrete plan for RFC-0067 **P3 + P4**: retire windowd's hand-rolled window
frames (`ShellWindow`) in favour of the **window widget** + the **retained scene
graph**, so a window is content-sized UI (not a fixed-size frame hardcoded in the
compositor) rendered through one model (not two parallel ones).

> Read after `window-intent.md` (the app-vs-WM ownership) and RFC-0067 (the
> compositor boundary). This is the *mechanism* those describe.

## What we have today (the two problems)

**1. Every window is a hardcoded, fixed-size `ShellWindow`.** windowd constructs
`search`/`settings`/`dsl_win`/`app_win`/`chat` each with a fixed size
(`SEARCH_W`, `SETTINGS_W`, `APP_WIN_MAX_W×H`, …) and hand-composites their atlas
bands in `runtime/scene.rs::build_scene_cb_into`. The app-client window is sized
to its **max** and clips content into it — which is why raising `MAX_SURFACE`
drew the frame + shadow at `1280×832`. This is not a sizing-math bug; it is *a
window frame living in the compositor at a fixed size.*

**2. There are two scene models, and the good one is unused.** `scene_graph.rs`
(~1700 LOC) is a **complete** retained graph — stable node ids, intrusive
siblings (= z-order), `InvalidationClass` + subtree hashing for **O(dirty)**
invalidation, zero per-frame heap, a `RenderPrimitive` vocabulary, **and a
`render_order` walk that emits straight into the `nexus-gfx`
`RenderCommandEncoder`.** Its doc says *"all UI frontends target this single
retained scene graph."* But nothing in the live per-frame path calls it — the
compositor runs `build_scene_cb_into`, which hand-composites `ShellWindow` bands.
So the retained graph is a parallel model (RFC-0067 P4's "collapse").

**3. The window widget already exists and is unadopted.**
`userspace/ui/widgets/window` = `Window` (title bar + close + body → a
`LayoutNode`, composed from `Panel` + `Button`) + `frame::Frame` (the host-tested
hit-test / drag / resize / damage SSOT) + `chrome` (`WindowControls`,
`WindowButton`, `WindowPane`). Built for RFC-0067 P3, never wired into windowd.

## Target pipeline (one model)

```
DSL / widgets ──▶ LayoutNode        (nexus-layout: content-sized)
                    │
                    ▼
              retained SceneNode graph   (scene_graph.rs: O(dirty), z-order, hashing)
                    │
                    ▼
              nexus-gfx CommandBuffer     (Layer / glass SSOT + rasterizer)
                    │
                    ▼
                  gpud                    (virgl GPU · CPU fallback)
```

- **A window is a `window::Window` `LayoutNode`**, laid out to its **content
  size** — no `APP_WIN_MAX`, no fixed frame. Chrome (title/controls) is added by
  the WM per `intent ⟂ policy` (chromeless for a `plain`/desktop shell).
- **The retained scene graph is the live SSOT.** It consumes the laid-out nodes,
  tracks dirty subtrees, and emits the CommandBuffer. `build_scene_cb_into`'s
  hand-compositing retires into it.
- **Glass + rasterization stay in `nexus-gfx`** (already the SSOT — `Layer` /
  `composite_layer_full`, `raster/`). The scene graph emits those commands; it
  does not re-implement them.

### Where each `ShellWindow` responsibility goes

| `ShellWindow` does today | New home |
|---|---|
| Frame geometry (hit-test / drag / resize / damage) | `window::frame::Frame` (SSOT, exists) |
| Chrome (title bar, controls, close) | `window::Window` / `chrome` → `LayoutNode` |
| Glass composite (blur/SDF/shadow) | `nexus-gfx` layer SSOT (exists) |
| Fixed size | **gone** — content size from layout |
| Atlas band / surface upload | windowd surface lifecycle (keeps) |
| Z-order / focus | `window_scene::WindowStack` (keeps) → scene-graph insertion order |

The WM keeps exactly the *service* concerns (surfaces, z-order, damage, present);
everything *visual* is a widget → scene node → nexus-gfx.

## The app-client surface + the DSL shell, on this model

- A normal **app** renders its content into its surface VMO. windowd wraps that
  surface in a **`Window` widget frame** (title + controls per policy), laid out
  to the content rect, as scene nodes. The frame is content-sized — the bug class
  disappears.
- The **desktop shell** declares `Window { style: plain, level: desktop }`
  (window-intent), so the WM adds **no** chrome: the shell's surface is the
  bottom scene layer, full-screen, and its panels are R1 material-glass layers.
- **Window controls + the mode dropdown** are WM-composed scene nodes gated by
  the windowing **policy** (zero under Kiosk/single-app) — never in the app.

## Scene-model decision (picked)

**Make `scene_graph.rs` the live path.** It is already the complete, better
renderer (O(dirty) beats the per-frame hand-composite); the docs already declare
it the SSOT. We wire it in and delete the parallel hand-compositing — we do **not**
keep two models or grow a third. `nexus-layout`'s `LayoutNode` is the *input*
(what widgets/DSL produce); the retained `SceneNode` graph is the *compositor's*
persistent structure; `nexus-gfx` is the *output* command vocabulary.

## Phases (strangler-fig, each host-tested + owner boot-verified)

Gate = host + windowd tests green · riscv build green · owner boots
`GPU_MODE=virgl just start` and confirms the UI is **identical or better**, never
worse · commit per phase. New home → switch one consumer → prove identical →
migrate the rest → delete the old → commit.

- **P3.1 — content-sized frame via `window::frame`.** Route `ShellWindow`'s
  geometry (size, hit-test, drag, damage) through `window::frame::Frame`, sized to
  the surface/content — not a fixed max. Kills the app-client max-size bug
  immediately, with the compositing path unchanged. Smallest, safest first step.
- **P3.2 — window chrome as the `Window` widget.** The title bar + controls
  become `window::Window`/`chrome` `LayoutNode`s; windowd stops drawing title
  rows by hand. Still composited by `build_scene_cb_into` for now.
  **Status 2026-07-13: DONE** — `runtime/chrome_widget.rs` renders the bar from
  `WindowControls` (+ a `gap()` builder so the 30 px buttons center in the
  `Frame`'s 40 px hit zones) + a text title through `nexus-layout` +
  `nexus-scene-raster` into ONE shared `ChromeCache` (keyed w/hover/theme/
  radius; band blits memcpy rows). The hand rasterizer `draw_title_bar_row`
  is deleted (`round_top_corners` survives as the cache's corner mask).
  Boot-proven with 4 windows + hover + fullscreen round-trips.
- **P4.0 — the `LayoutNode` → `SceneNode` bridge (the enabler).** Today
  `scene_graph` is populated **by hand** (`systemui_shell` calls `insert_node` +
  `RenderPrimitive` per element); there is **no** bridge from a laid-out widget
  tree to the scene graph. Build `layout_to_scene`: walk a `LayoutResult`
  (rect + `VisualStyle` per box + text runs) and emit `SceneNode`s
  (`Rect` for a fill, `Group`+`BackdropFilter` for a `material: glass` box,
  text nodes, `Surface` for a client-VMO body). This is what makes "a window is
  a widget" real — every widget/DSL renders through the same path. Host-tested
  (pure `LayoutResult` → node list).
- **P4.1 — scene graph goes live for one window.** Build the app-client window
  as a `window::Window` `LayoutNode`, run it through `layout_to_scene`, and
  render via `scene_graph`'s encoder walk; leave the others hand-composited.
  Proves the pipeline end-to-end and retires the app-client `ShellWindow`.
- **P4.2 — migrate the remaining windows + chrome** onto the scene graph;
  `build_scene_cb_into`'s hand-composite shrinks to nothing.
- **P4.3 — delete `ShellWindow` + the hand-composite path.** One scene model,
  content-sized window widgets, `nexus-gfx` output. RFC-0067 P3+P4 closed.

The DSL-shell / R1 material-layer / intent×policy work all ride on this: once
windows are content-sized widgets through the scene graph, the shell surface is
just the desktop-level, chromeless window, and its panels are scene-graph glass
layers — no `ShellWindow`, no `APP_WIN_MAX`, no fixed frames.

## Related

- `window-intent.md` — app-owned intent × environment policy; the material layer seam.
- `docs/rfcs/RFC-0067-…` — the compositor-service boundary + Revival (R1–R4).
- `userspace/ui/widgets/window/` — `Window` + `frame` + `chrome` (the widget).
- `source/services/windowd/src/scene_graph.rs` — the retained graph to make live.
- `source/services/windowd/src/compositor/shell_window.rs` — `ShellWindow`, to retire.
