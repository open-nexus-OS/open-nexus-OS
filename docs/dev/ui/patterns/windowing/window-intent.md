<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Window Intent × Windowing Policy

The model for how a DSL app's window is framed, resized, and controlled. It has
**two orthogonal axes**, and the compositor renders their product:

- **Window intent** — *app-owned*, environment-invariant. What the app **is**
  (content structure + preferred style/mode/size). Declared identically on every
  device. An app never asks for a close/minimize button — those are not intent.
- **Windowing policy** — *environment-owned* (the active shell profile /
  product). Which window-management **affordances exist at all** (controls, mode
  switching, multi-window, windowed-vs-fullscreen). A property of the product,
  not the app.

> `chrome = intent ⟂ policy` — the app declares intent, **windowd** composes the
> frame, **systemui** supplies the policy. min/max/close and the mode menu are
> WM affordances gated by policy; they are never in the app and never in intent.

This is why the same app renders as a resizable window with controls on a
desktop and as a chromeless fullscreen surface in a single-app OS, with no
change to the app: the environment **clamps** the intent.

> Planning doc. "Built" = shipped; otherwise the owning slice is named. The
> app-facing intent vocabulary and the policy table are the contract; the wire
> encoding is being cut slice-by-slice (see [Staging](#staging)).

## Axis 1 — Window intent (app-owned)

Declared by the app in its `.nx` as a top-level `Window { … }` scene. Stable
across every environment; the app re-declares nothing per device.

### Frame intent

| Field | Values | Meaning |
|---|---|---|
| `style` | `titlebar` · `hiddenTitlebar` · `plain` | How much frame the app *wants*. `plain` = no title zone at all (the shell). |
| `mode` | `auto` · `freeform` · `fullscreen` | Preferred default window mode. `auto` lets the policy choose. |
| `level` | `normal` · `desktop` · `overlay` | Z-band. `desktop` = the bottom shell layer; `overlay` = above normal windows. |
| `resizable` | `true` · `false` | Whether the app tolerates arbitrary content sizes. `false` + `fullscreen` = a fixed full-screen surface (the shell). |
| `defaultSize` | `w × h` | Preferred freeform size; ignored when the policy is fullscreen-only. |

The app declares *preferences*; the policy decides what is honoured. A locked
kiosk app that declared `titlebar` still gets no title bar — the policy wins.

### Content structure (semantic, not positional)

Inside the `Window`, structure is declared by role so it survives every frame
and mode. windowd (and, when collapsed, the app) places it — never absolute
coordinates.

- **`SplitView { sidebar {…} content {…} inspector {…} }`** — the responsive
  navigation/content/properties layout (the design handoff's `AppWindow`). Side
  panes collapse into glass overlays as the window narrows. `content` is the
  only required pane.
- **`.toolbar { item(placement: …) { … } }`** — items are positioned into the
  frame by **semantic placement**, not coordinates:
  - `navigation` — leading, back/nav affordances.
  - `principal` — centre (title / segmented control).
  - `primaryAction` — trailing actions; the **"more" / overflow** is a `Menu`
    here (right of the frame, left of the WM window controls).
  - `bottomBar` — the `WindowActionBar` (floating action bar).
- **`inspector`** — the properties pane; collapses to an overlay when narrow.

The app paints its **content panes only**. It never paints a title bar, window
controls, or the mode menu.

## Axis 2 — Windowing policy (environment-owned)

A property of the active **shell profile** (the systemui SSOT — see
`docs/dev/ui/foundations/layout/profiles.md` and the product → shell selection).
It answers "which window-management affordances does this product provide?"

| Profile | Window controls | Mode dropdown | Multi-window | Default framing |
|---|---|---|---|---|
| **Desktop** | min / max / close | freeform · tile · fullscreen | yes | full glass Window chrome |
| **Tablet** | close (or gesture) | fullscreen · split | limited | light chrome, mostly fullscreen |
| **TV** | none | none | no | fullscreen, focus-nav only |
| **Kiosk / SingleApp** | **none** | **none** | no | app fills screen, **zero chrome** |

The policy is the **ceiling** on affordances; an app can only ever receive what
the policy provides. `Kiosk` grants nothing, so *any* app — including a plain
app chosen as the single-app launcher — fills the screen with no controls,
regardless of the `style` it declared. This is the single-app / "app as
launcher" case, expressed without a special app type.

## Composition — `chrome = intent ⟂ policy`

windowd is the window manager. Per client surface it computes the frame from the
app's intent **clamped by** the active policy:

1. **Controls** = policy's control set (∅ under Kiosk/TV) — never from intent.
2. **Mode menu** = shown iff policy allows mode switching **and** the app is
   `resizable`. The top-left dropdown (next to the app icon) is windowd's.
3. **Title zone** = drawn iff `style != plain` **and** policy provides chrome.
4. **Level** = intent's z-band, clamped (an app cannot request `desktop` unless
   it is the selected shell — enforced like a privilege, see below).
5. **Panes** = the app's `SplitView`/`toolbar` items placed into the composed
   frame; collapsed to overlays under narrow/fullscreen policy.

Ownership summary:

| Concern | Owner |
|---|---|
| Content + structure + frame *preference* | the app (`Window` intent) |
| The frame, controls, mode menu, mode transitions, geometry | **windowd** |
| Which affordances exist (the policy) | **systemui** (shell profile / product) |

`level: desktop` is capability-gated, not a free intent: only the app the
product selected as `shell` (bundle_type `shell`, see
`docs/dev/app-platform/privileged-roles.md`) is composited at the desktop band.
A normal app declaring `desktop` is clamped to `normal` — same fail-closed
discipline as the routing ceiling.

## Geometry & resize — the WM owns it

Because the WM owns the frame and the mode transitions, it also owns geometry:
the app does **not** query the display size. Instead windowd **pushes the
current content rect** to the app over the event channel it already holds
(alongside input + surface acks). The app re-lays-out to whatever rect it gets.

- Launch / mount → windowd sends the initial content rect for the composed frame
  under the active policy.
- Mode change / resize / rotate → windowd sends a new content rect; the app
  re-lays-out and re-presents.
- The **shell** simply receives "content rect = full screen" — the fullscreen
  case is not special, it is one value of the general resize channel.

This replaces any one-off "query display mode" op: resize is the general
channel, and it is exactly what freeform ⇄ tiled ⇄ fullscreen already need.

## The two anchor cases

- **Desktop shell** = intent `{ style: plain, mode: fullscreen, level: desktop,
  resizable: false }` under **Desktop** policy → chromeless, bottom z-band,
  full-screen content rect. It is the launcher; other app windows compose above
  it with full chrome. No "desktop role" flag — just intent + policy + the
  shell capability.
- **Single-app OS** = **Kiosk** policy → no controls, no mode menu, no
  multi-window; the chosen app fills the screen chromeless whatever it declared.

## Flow

```
app (.nx)            Window { style, mode, level, resizable, SplitView, toolbar }
   │  (compiled into the payload as scene intent)
   ▼
app-host             reads intent from the payload; creates its surface;
   │                 renders content panes only (no chrome)
   │  SURFACE_CREATE (+ intent)                    ▲ content-rect (resize) events
   ▼                                               │
windowd (WM)  ── compose frame = intent ⟂ policy ──┘
   ▲                 draws controls / mode menu / title / panes; owns geometry
   │  policy (shell profile)
systemui             product → shell profile → windowing policy
```

## Staging

1. **Slice 1 (shell)** — introduce the `Window` intent enum
   (`style`/`level`/`resizable`/size) end to end for the minimal corner: the
   shell as `plain / fullscreen / desktop` under Desktop policy, windowd pushes
   the content rect, no title bar, bottom z-band. First boot-verifiable step.
2. **Slice 2** — normal-app frame under Desktop policy: title zone + `WindowControls`
   (min/max/close) + the mode dropdown; freeform/fullscreen transitions via the
   resize channel.
3. **Slice 3** — `SplitView` (sidebar/content/inspector) + semantic `toolbar`
   placement + the overflow `Menu`, with responsive collapse.
4. **Slice 4** — policy clamping per profile (Tablet/TV/Kiosk): the affordance
   table enforced; single-app OS validated.

## Related

- `docs/dev/ui/patterns/windowing/README.md` · `wm.md` · `wm-snap.md` ·
  `wm-resize-move.md` — the WM mechanics these compose onto.
- `docs/dev/ui/foundations/layout/profiles.md` — the shell-profile SSOT the
  policy hangs off.
- `docs/dev/design_handoff_open_nexus_os/README.md` — `Window` / `AppWindow` /
  `WindowControls` / `WindowActionBar` components + the "OS Window Frame" template.
- `docs/dev/app-platform/privileged-roles.md` — `bundle_type = shell` (the
  desktop-band capability) and the ceiling model this level-gating mirrors.
