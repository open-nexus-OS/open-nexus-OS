<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The `Window` declaration

`Window { … }` states what the app's window **is** — its frame, its mode, its z-band, whether
it may be resized and the size it wants. It is the app-owned half of the window model: the app
declares the intent, the window manager (windowd) composes it under the active windowing policy
and owns the geometry. The model: [window-intent.md](../ui/patterns/windowing/window-intent.md);
the size fields: RFC-0117
([`docs/rfcs/RFC-0117-the-window-intent-carries-the-apps-size-a-default-and-a-minimum.md`](../../rfcs/RFC-0117-the-window-intent-carries-the-apps-size-a-default-and-a-minimum.md)).

```nx
Window {
    style: plain,
    mode: auto,
    resizable: true,
    width: 340,
    height: 600,
    minWidth: 340,
    minHeight: 600,
}

Page CalculatorPage {
    Stack {
        Text("0").textStyle(display)
    }
}
```

## Fields

Every field is optional and the order is free; fields are separated by `,` or `;`. A program has
at most one `Window`; without one, every field takes its default.

| Field | Values | Default | Meaning |
|---|---|---|---|
| `style` | `titlebar` · `hiddenTitlebar` · `plain` | `titlebar` | what the app does with the top of its frame. The window kit draws the bar and the window controls inside the surface for every style; windowd adds no title bar of its own. |
| `mode` | `auto` · `freeform` · `fullscreen` | `auto` | the preferred mode. `auto` lets the window manager choose (a small utility frame when the app declares no size); `fullscreen` covers the work area. |
| `level` | `normal` · `desktop` · `overlay` | `normal` | the z-band: `desktop` is the shell's bottom band, `overlay` floats above normal windows (the on-screen keyboard docks at the bottom edge). |
| `resizable` | `true` · `false` | `false` | whether the app tolerates arbitrary sizes. A window is user-resizable only when it is freeform — not fullscreen, not the desktop, not an overlay — under a policy that allows resizing; a window that is not resizable cannot be tiled. |
| `width`, `height` | px, `1..=8192` | absent | the default content size of a freeform window, at desktop scale |
| `minWidth`, `minHeight` | px, `1..=8192` | absent | the smallest content size the app tolerates |

## What the window manager does with the size

- A freeform window opens at the app's `width` × `height`, clamped to its minimum and to the work
  area. Without a size it opens at the window manager's default — three quarters of the display
  wide, five sixths of the work area tall — or, for `mode: auto`, at its small utility frame.
- The window's minimum is the larger of the app's minimum and the window manager's own, but never
  more than the work area.
- A resize, a tile and a restore never make the window smaller than its minimum; a tile zone
  smaller than the minimum gives the window its minimum on that axis.
- Fullscreen and maximized windows take the work area.

The rule is `source/services/windowd/src/window_size.rs` (host-tested in
`source/services/windowd/tests/window_size.rs`).

## Errors

| Code | When |
|---|---|
| NX0101 | a field value of the wrong kind (`resizable: yes`, `width: "340"`) |
| NX0303 | a field the declaration does not have (`minSize: 300`) |
| NX0305 | a `style`, `mode` or `level` outside its values (`mode: tiny`) |
| NX0301 | a size outside `1..=8192`, or a default below its minimum on the same axis (`width: 300, minWidth: 340`) |

`nx-dsl explain NX0301` prints the long form of any code.

## Examples

The system shell — chromeless, full screen, on the desktop band, never resized:

```nx
Window {
    style: plain,
    level: desktop,
    resizable: false,
}

Page ShellPage {
    Stack { }
}
```

A document window that opens at 900 × 600 and never gets narrower than a phone:

```nx
Window {
    style: hiddenTitlebar,
    mode: freeform,
    resizable: true,
    width: 900,
    height: 600,
    minWidth: 360,
    minHeight: 480,
}

Page DocumentPage {
    Stack { }
}
```

## See also

[`syntax.md`](syntax.md) (the program's declarations) · [`widgets.md`](widgets.md) ·
[`profiles.md`](profiles.md) (the device the app runs on) ·
[window-intent.md](../ui/patterns/windowing/window-intent.md) (intent ⟂ policy).
