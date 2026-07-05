# Handoff: open nexus OS — Design System → Rust (custom GUI stack)

## Overview
**open nexus OS** (AIVA Computers) is a hybrid tablet/desktop OS launcher — part iOS home screen, part macOS top bar, part Android taskbar. Its signature visual language is **liquid glass**: every UI surface is a frosted, translucent panel over a full-bleed wallpaper. UI copy is German.

This package hands off the **entire design system** — tokens, ~50 components, and 7 full-window/screen templates — so it can be reimplemented in your own Rust component layer → DSL → GUI pipeline.

## About the files in this bundle
Everything under `reference/` and the source design system is **HTML/CSS/React (JSX)** — built as design references and a living style guide, not code to port line-by-line. Your job is to **re-express the same visual system and interaction model** in Rust, using your own component primitives and renderer. Treat:
- `reference/tokens/*.css` and `reference/styles.css` — as **the literal numeric contract** (colors, spacing, radii, shadows, blur, motion curves/durations). These values should carry over exactly, just re-declared in whatever token format your Rust layer uses (const table, TOML, build-time codegen, etc).
- `reference/component-api.d.ts` — as **the prop/variant contract** for each component (which variants, sizes, and states exist, and what callbacks/props they take). Re-implement the same variant surface as native Rust components — not the JSX/DOM internals.
- `screenshots/*.png` — as **visual ground truth** for layout, spacing, and the glass effect appearance, since backdrop-blur/translucency compositing will need a custom renderer implementation (most Rust GUI stacks don't have blur built in).

## Fidelity
**High-fidelity.** All colors, spacing, radii, shadows, and motion curves are final tokens (see `reference/tokens/`), not placeholders. Recreate pixel-accurately where your renderer allows; the one part that requires real engineering work (rather than 1:1 port) is the **backdrop-blur / glass compositing**, since that's a GPU/compositor feature specific to each rendering stack.

## The core visual system: Liquid Glass
Three "glass levels," each with dark + light values as CSS custom properties in `reference/tokens/glass.css`:

1. **Panel** — `blur(40–64px)`, bg `rgba(255,255,255,0.10)` dark / `0.50` light. Dock, control center, app launcher.
2. **Card** — `blur(20px)`, bg `rgba(255,255,255,0.08)` dark / `0.60` light. Cards nested inside panels.
3. **Subtle** — `blur(12px)`, bg `rgba(255,255,255,0.06)` dark / `0.70` light. Settings rows, list items.

Plus a fourth, denser **Window** material (`--glass-window-*` tokens) used only for app/settings window chrome — solider and less translucent than the launcher panels, so document content stays legible.

Every glass surface also has:
- A **1px border**: `rgba(255,255,255,0.18–0.20)` dark / `rgba(255,255,255,0.75–0.80)` light.
- A **top-shine overlay**: `linear-gradient(to bottom, rgba(255,255,255,0.10–0.15), transparent)` pinned to the inner top edge — a soft highlight, not a full gradient fill.
- A **drop shadow**: panels `0 25px 50px rgba(0,0,0,0.40–0.60)`; see `--shadow-*` tokens for the full scale.

To reproduce: wallpaper (full-bleed photo) → tint overlay (`rgba(0,0,0,0.35)` dark / `rgba(255,255,255,0.20)` light) → backdrop-blur layer → semi-opaque fill → 1px border → top-shine → content. All four layers must stay translucent enough for the wallpaper to read through the blur.

Reference screenshots: `13-glass-levels.png` (all 3 levels side by side, light + dark), template screenshots (`01`–`07`) for glass in real context.

## Design Tokens
Full literal values live in `reference/tokens/` — copy these directly into your Rust token layer:

- **colors.css** — brand (`#030213` primary navy), destructive (`#d4183d`), light/dark surface pairs, sidebar, chart palette, semantic status colors. Dark mode is a `.dark` scope override of the same variable names.
- **glass.css** — the liquid-glass system described above, plus toggle-on/off states, notification dot color (`#ef4444`), text-on-glass opacities.
- **typography.css** — font stack (`Inter` → `Noto Sans` → system), weights (400/500/600/700), size scale 11px–36px, line-heights, letter-spacing.
- **spacing.css** — 4px-based spacing scale (0–96px), border-radius scale (6px–pill), shadow scale (5 steps + icon/dock specials), z-index layers.
- **motion.css** — 5 easing curves and 5 durations (see below), plus shared keyframes for overlay/menu/sheet/toast entrances.

### Motion — one physics vocabulary for the whole OS
| Token | Curve | Duration | Use |
|---|---|---|---|
| `--motion-spring` | `cubic-bezier(0.34,1.4,0.5,1)` | 0.50s | panel/window entry, elastic overshoot |
| `--motion-spring-soft` | `cubic-bezier(0.34,1.2,0.5,1)` | 0.28s | thumbs, chips, subtle overshoot |
| `--motion-spring-icon` | `cubic-bezier(0.34,1.56,0.64,1)` | — | icon hover pop, strongest overshoot |
| `--motion-smooth` | `cubic-bezier(0.4,0,0.2,1)` | 0.40s | collapse/exit — quick, never bouncy |
| `--motion-glide` | `cubic-bezier(0.22,1,0.36,1)` | 0.45s | page swipes, large moves |

Rule of thumb: **entrances spring, exits are smooth and faster, presses are instant** (`scale(0.9–0.95)` in 0.1s then springy release). Respect a reduced-motion equivalent — collapse all durations to ~0.

## Components (reference/component-api.d.ts)
~50 components across 7 groups, each documented with full prop/variant/size/state signatures in the `.d.ts` file. Groups, and what they cover:

- **core** — `GlassButton` (6 variants incl. glass/active/destructive, 4 sizes), `Badge` (8 variants), `GlassCard` (5 surface levels — panel/card/subtle/window/window-pane), `AppIcon` (5 sizes, glass backing), `GlassToggle` (iOS-style switch).
- **controls** — `Segment` (sliding-thumb segmented control), `Slider`, `GlassCheckbox`, `GlassRadioGroup`, `Stepper`, `Select` (frosted dropdown, not native), `Rating`, `WheelPicker` (iOS snap-scroll), `DatePicker` (day/month/year wheel).
- **inputs** — `TextField`, `SearchBar` (iOS pill), `TextArea` (auto-grow + counter).
- **feedback** — `Spinner`, `ProgressBar`, `Toast`, `Skeleton`/`SkeletonText`, `Banner` (4 variants), `Refresher` (pull-to-refresh).
- **overlays** — `Modal`, `ActionSheet`, `Alert`, `Popover`/`PopoverItem`, `Menu`/`ContextMenu`, `Tooltip`, `FAB` (expandable). These sit on a denser "overlay/reading" material (`--glass-overlay-*`, ~0.80–0.82 opacity) distinct from panel glass, so text stays crisp.
- **navigation** — `Toolbar`, `TabBar`, `List`/`ListItem`, `SubHeader`, `Accordion`, `Sidebar`/`SplitView`, `TreeView`, `Breadcrumbs`, `Pagination`, `Avatar`, `Chip`.
- **window** — the shell every app/settings window is built from: `Window` (surface + 3-zone title chrome), `WindowPane` (inner content card), `WindowControls` (minimize/maximize/close), `WindowButton`, `Icon`, `WindowActionBar`, and `AppWindow` (the composed pattern: sidebar + content pane + properties pane, responsive — side panes collapse into glass overlays as the window narrows).

Screenshots `08`–`11` show representative components from core/controls/overlays/navigation groups rendered together.

## Templates (full screens/windows)
Seven reference screens, screenshotted in `screenshots/`:

1. **OS Window Frame** (`01`) — the full reference: window chrome, app menus, sidebar navigation, content list, properties pane, floating action bar.
2. **OS App Window** (`02`) — file-manager composition of `AppWindow`.
3. **OS Settings Window** (`03`) — settings composition of `AppWindow`.
4. **OS Control Center** (`04`) — quick toggles (WiFi/Bluetooth/Airplane/mode), now-playing media card, brightness/volume sliders.
5. **OS Notifications** (`05`) — Mitteilungen: stacked, swipeable, persistent-until-dismissed app notifications.
6. **OS Calendar Panel** (`06`).
7. **OS Login** (`07`).

## Notification & Feedback System — 5 surfaces, one rule each
This is a behavioral spec, not just visual — implement the routing logic, not only the look:

| Surface | What belongs here | Source | Lifespan | Position |
|---|---|---|---|---|
| **Activity Runner** (our name for the live/"dynamic island" pill) | Running activity with live state + transient high-priority (call, timer, nav, recording) | App or System | persistent while running | top center |
| **Mitteilungen** (Notification Center) | App pushes, passive, stackable | App | persistent until dismissed | top/right, expandable |
| **Control Center** | Only now-playing media control | System | persistent, mirrors state | Control Center panel |
| **System-Toasts** | Transient system confirmations, no action needed | System only | 3–5s auto-dismiss | left edge |
| **Background Jobs** (taskbar) | Local background processes + media mini-player | System/app | persistent while job runs, max 3 + overflow chip | bottom bar |

Routing test for any new event: **(1) who sends it** (system/app/live activity), **(2) how long does it live** (transient/persistent), **(3) must the user act** (passive/active). One surface, no duplicates — e.g. a call lives in the Activity Runner only, never also as a Toast; media is one state mirrored in up to three places but never a Notification.

Priority when crowded: Critical (call/alarm) → Active-live (nav/timer/recording) → System ack (toast) → App notice (Mitteilungen) → Background (progress). See screenshots `14`–`16`.

## Iconography
- **System icons**: Lucide (stroke weight 2, 14–18px). Rust equivalent: pick one consistent stroke-icon set (Lucide has a Rust-friendly SVG export, or use `lucide` icon data directly) — don't mix icon families.
- **App icons**: 60+ custom multi-layer SVGs (transparent bg), always rendered inside a glass-backed rounded-rect tile (see `AppIcon` component, 5 sizes). Screenshot `12`.
- No emoji as icons; no hand-drawn/decorative SVG illustration anywhere in the system.

## Language & Copy
UI strings are German throughout (Einstellungen, Nachrichten, Dateien…), sentence case, terse/imperative, no marketing tone. `de-DE` 24-hour time, no-decimal percentages. Keep this if the product ships to the same market — swap only if you're localizing.

## Assets
- `reference/tokens/` — all CSS custom properties (source of truth for every numeric value).
- `reference/styles.css` — the import manifest tying tokens together.
- `reference/component-api.d.ts` — every component's prop/variant contract, concatenated.
- `reference/design-system-readme.md` — the full original design-system readme (product context, full color/type/shadow docs, complete file index).
- `screenshots/` — 16 reference renders: 7 full templates, 4 component-group showcases, 5 OS-shell/glass/icon guideline cards.

## Suggested implementation order
1. Port the token tables (colors, type scale, spacing/radius/shadow, motion curves) into your Rust token layer verbatim.
2. Build the glass-compositing primitive (blur + tint + border + shine + shadow) as a single reusable draw call/widget — everything else is built on top of it.
3. Build `core` components (button, card, icon, toggle) — these compose into everything else.
4. Build `window`/`AppWindow` shell, then the two window templates.
5. Build the remaining control/input/feedback/overlay/navigation components as needed per screen.
6. Implement the 5-surface notification routing logic last — it's behavioral, not visual.
