# Handoff: open nexus OS — Launcher / Shell UI → Rust (custom GUI stack)

This package is everything needed to reimplement the **OS launcher shell** — top bar, dock/taskbar, desktop icons, app launcher, and all drop-down panels (Control Center, Notifications, Calendar, WiFi, Sound, Battery) — in your own Rust component layer → DSL → GUI pipeline.

## What's in here
- **`OpenNexusLauncher.html`** — the whole launcher as **one self-contained HTML file**. Open it in any browser, offline. This is the **living source of truth** — it is fully interactive: switch device presets (top-right Auto/Phone/Tablet/Landscape), toggle dark/light and Desktop/Tablet mode inside the Control Center, open every panel, and drag/swipe the app-launcher pages. Because glass depends on real GPU `backdrop-filter`, **this file — not the screenshots — shows the true look.**
- **`reference/launcher-source.html`** — the same launcher, un-bundled and readable (React/JSX). Every layout rule below is implemented here; read it as the reference implementation.
- **`reference/launcher-modes.card.html`** — the 4-layout responsive diagram (Desktop · Landscape · Tablet Portrait · Phone) with the glass tokens annotated inline.
- **`reference/tokens/`** — all design tokens as CSS custom properties (colors, glass, typography, spacing/radius/shadow, motion). The **literal numeric contract** — port these values verbatim.
- **`screenshots/01-tablet.png`** — the tablet home screen (top bar + desktop icons + dock). Note: HTML-to-image can't render backdrop-blur, so panel overlays screenshot see-through — open the HTML for those.

> These files are HTML/CSS/React **reference material**, not code to port line-by-line. Re-express the same layout logic, tokens, and interaction model in Rust. Treat the tokens as exact numbers, the source as the behavioral spec, and the bundled HTML as the visual ground truth.

---

## The one concept that drives everything: mode vs. width
There are **two independent axes** that decide the whole layout. Get these right first.

**1. `desktopMode` — an explicit boolean the user toggles** (Control Center → the "Desktop / Tablet" tile, `--accent` teal for desktop, violet for tablet). This is **not** width-derived. When `true`, the shell uses the **desktop taskbar + windowed launcher + individual status panels**. When `false`, it's a touch tablet/phone shell.

**2. Viewport width `vw`** — used only when `desktopMode` is `false`, to pick between three touch layouts:

| Layout | Condition | 
|---|---|
| **Phone** | `vw < 640` |
| **Tablet Portrait** | `640 ≤ vw < 1024` |
| **Landscape** | `vw ≥ 1024` |

(The live HTML also has a dev-only device-preset picker top-right that overrides `vw` — drop that in your build.)

So the four real targets are: **Desktop** (mode flag) and **Phone / Tablet Portrait / Landscape** (width, touch).

---

## Top Bar (height 36px, always on top, `z-index:50`)
Always three zones: **left · Dynamic Island (absolute center) · right**.

- **Desktop mode**
  - Left: time+date pill (opens Calendar) · notifications pill (bell + red dot, mail-count "4", messages + calendar glyphs → opens Notifications).
  - Right: **four separate pills** — WiFi · Sound · Battery ("78%") · Control-Center — each opens **its own** drop-down panel.
- **Tablet Portrait / Landscape** (touch)
  - Left: time pill (→ Calendar) · bell pill (→ Notifications).
  - Right: **one** combined status pill (wifi + volume + battery "78%") → opens the full **Control Center** (which contains WiFi/sound/battery inline). Touch collapses the four desktop panels into one.
- **Phone** (`vw < 640`, "merged mode")
  - Left: time **and** bell combined into a single pill → opens Notifications.
  - Right: combined status pill → Control Center.

**Dynamic Island** — pinned dead-center in every mode. Collapsed `118×28`, expands on hover to `288×64` (now-playing: art + title/artist + transport). Transition `cubic-bezier(0.34,1.4,0.5,1)` ~0.5s (the `--motion-spring` curve). Pills use hover-fill only (`rgba(255,255,255,0.18)` dark / `rgba(0,0,0,0.10)` light), no persistent background.

---

## Dock / Taskbar (bottom, `z-index:50`)
This is where the four layouts differ most.

- **Desktop mode → Taskbar.** Full-width bar, height **56px**, **panel glass**, `blur(64px)`, top border only (`1px rgba(255,255,255,0.12)` dark). Three zones: **left** launcher glyph (bare icon, hover-brighten, no chrome) · **center** app icons placed *directly* in the bar (size `xs`, active app underlined) + divider + inline mini media player · **right** back / home / overview nav (bare icons).
- **Landscape (≥1024) → three floating elements**, all panel glass, sitting on the wallpaper: round **launcher button** (left) · **dock pill** = app icons (`md`) + divider + mini player (center) · **nav pill** = back/home/overview (right).
- **Tablet Portrait (640–1024).** Round launcher button + dock pill **side by side, centered**; **bare** nav icons in a row **below** them.
- **Phone (<640).** Full-width **dock row** of 5 apps (`sm`, evenly spread); **bare** nav row below: back · launcher glyph · home · overview.

Dock apps (default): Locus, Stash (Files), Tuner (Settings), Relay, Iris. Mini player = album gradient + track + play; mirrors the Dynamic Island's now-playing state.

---

## Desktop / home icons (the area between bar and dock, `z-index:10`)
- **Desktop mode**: a **vertical column** that wraps into more columns (flex-column, `flex-wrap:wrap`, `align-content:flex-start`), gap `22px × 34px`, padding 20 — classic desktop icon field, top-left origin.
- **Touch (tablet)**: a centered **6-column grid**, gap `24px × 16px`, top-aligned, padding 28.

Icons use the `AppIcon` component (`md`, labeled). Icon variants: **native** (asset is already a shaped tile — default), **freestanding** (folders/special: Files/Pictures/Videos/Documents/Downloads — transparent, no tile), **wrapped** (shapeless/sideloaded — render on a glass panel).

---

## App Launcher
Opened from the launcher glyph / home.

- **Touch (Tablet/Phone) → fullscreen overlay.** Backdrop `blur(24px)` over a dark/light scrim, greeting + date header, search field, then a **paged app grid** that supports touch-swipe, mouse-drag, and wheel, with page-dot indicators. Grid columns/rows are measured to fit the viewport (`ResizeObserver`). Page transition `cubic-bezier(0.22,1,0.36,1)` ~0.42s (`--motion-glide`).
- **Desktop → two sub-modes** (toggle via the expand button in the launcher):
  - **Windowed** (default): `720×520` floating panel, **no** backdrop blur, 4-column scrolling grid, and a **footer** with user avatar/name + power/settings actions.
  - **Fullscreen**: same as touch fullscreen.
- **Phone**: fullscreen launcher only.

Entry animation: `slideUp` (opacity + translateY + slight scale), `cubic-bezier(0.16,1,0.3,1)` 0.3s.

---

## Drop-down Panels (all anchor at `top:44`, `z-index:40`, panel glass `blur(72px) saturate(180%)`)
Each panel renders a full-screen invisible backdrop behind it (`z-index:39`) that closes it on outside click. Only one panel open at a time (opening any closes the others).

- **Control Center** — `328px`, top-right. The touch shell's single hub. Contents: connectivity grid (WLAN, Bluetooth, Flugmodus, and the **Desktop/Tablet** view-mode toggle), Brightness slider + appearance (dark/light) button, Sound slider + mute, Battery meter. Entry `slideUp` 0.3s. Toggle-tile active state = accent fill + white text (accents: wifi/bt blue `59,130,246`, air orange `249,115,22`, desktop teal `20,184,166`, tablet violet `139,92,246`).
- **Notifications** — `330px`, top-left. Title "Mitteilungen" + "Alle löschen". A **stacked** mail card (count badge, second card peeking behind at `translateY(7px)`) then individual notification cards (app icon, title, body, relative time). Entry `slideDown` 0.35s spring.
- **Calendar** — `288px`, top-left. Month grid (Mon-first, today = blue filled circle, event days = red dot under the date), month nav + "today" dot button, and an "Anstehend" upcoming-events list with colored spine + relative labels ("Heute"/"Morgen"/weekday).
- **WiFi / Sound / Battery** — `300px`, top-right, **desktop-mode only** (touch folds these into Control Center). WiFi: on/off toggle, "Nur Kabel", network radio-list with signal bars + lock. Sound: output/input device radio-lists + volume slider. Battery: %, power source, energy-profile radio-list, "Kein Standby".

Shared panel bits: `RadioRow` (selectable list row, active = blue tint + check), section labels at 11px `font-weight:600`, cards nested inside panels use **card glass** (lighter/less blur than the panel).

---

## Glass system (the signature look)
Three levels, plus a denser window material — full dark/light values in `reference/tokens/glass.css`:
1. **Panel** — `blur(40–72px)`, bg `rgba(255,255,255,0.10)` dark / `0.50` light. Dock, taskbar, all drop-down panels, launcher.
2. **Card** — `blur(20–40px)`, bg `0.08` dark / `0.60` light. Cards nested inside panels, mini player.
3. **Subtle** — `blur(12px)`, bg `0.06` dark / `0.70` light. List rows.

Every glass surface = **backdrop-blur layer → semi-opaque fill → 1px border** (`rgba(255,255,255,0.18)` dark / `0.75` light) **→ top-shine** (`linear-gradient(to bottom, rgba(255,255,255,0.12), transparent)`, inner top edge) **→ drop shadow** (`0 25px 50px rgba(0,0,0,0.45)`). The wallpaper must read through all of it. **This backdrop-blur compositing is the one part that needs real renderer engineering in Rust** — most GUI stacks don't have it built in. Build it once as a reusable primitive and everything else layers on top.

Background stack: full-bleed wallpaper → tint overlay (`rgba(0,0,0,0.35)` dark / `rgba(255,255,255,0.20)` light) → shell.

## Motion (one physics vocabulary — `reference/tokens/motion.css`)
Entrances spring (`cubic-bezier(0.34,1.4,0.5,1)`, panels/island), page moves glide (`cubic-bezier(0.22,1,0.36,1)`), exits are smooth & faster (`cubic-bezier(0.4,0,0.2,1)`), icon hover pops (`cubic-bezier(0.34,1.56,0.64,1)`, `scale(1.09)`), presses are instant (`scale(0.9)` in ~0.1s). Honor reduced-motion (collapse durations).

## Hit targets & sizing
Touch hit targets ≥ 44px. Top bar 36px, taskbar 56px. AppIcon named sizes map to px: `xs`≤42 · `sm`≤52 · `md`≤64 · `lg`≤80 · `xl`>80. Panel radii 20–30px; dock pills fully rounded (`--radius-full`).

## Language
All UI copy is German (`de-DE`), 24-hour time, sentence case. Keep if shipping to the same market.

## Suggested build order
1. Port token tables (colors, glass, type, spacing/radius/shadow, motion) into your Rust token layer verbatim.
2. Build the glass-compositing primitive (blur + tint + fill + border + shine + shadow) as one reusable widget.
3. Build the wallpaper + top bar + Dynamic Island.
4. Build the two dock families: touch (phone/tablet/landscape) and desktop taskbar, switched by the mode/width logic above.
5. Build the desktop-icon field (both column and grid variants).
6. Build the app launcher (fullscreen + windowed).
7. Build the drop-down panels: Control Center first (it's the touch hub and owns the mode toggle), then Notifications, Calendar, then the desktop-only WiFi/Sound/Battery panels.
