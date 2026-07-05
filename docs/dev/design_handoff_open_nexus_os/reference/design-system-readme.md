# open nexus OS Design System

**Source repository:** [AIVA-Computers/Oslauncherdesign](https://github.com/AIVA-Computers/Oslauncherdesign)  
**Original Figma:** https://www.figma.com/design/dYF51i6v5zRuN9bEanFhbl/OS-Launcher-Design  
**Attributions:** shadcn/ui (MIT), Unsplash photos

---

## Product Overview

**open nexus OS** (by AIVA Computers) is a hybrid tablet/desktop operating system launcher — part iOS home screen, part macOS top bar, part Android taskbar. It runs on tablets, desktop, and narrow phone viewports, with a responsive layout that adapts between "tablet mode" (portrait, floating dock) and "desktop mode" (landscape, full taskbar).

The defining visual language is **liquid glass**: every UI surface is a frosted, translucent panel with backdrop-blur, white borders, and soft shadows sitting over a full-bleed wallpaper. This gives the OS its signature "you can see the world through the interface" quality.

The UI language is **German** (Einstellungen, Nachrichten, Dateien, etc.), pointing to a German-market product.

---

## Content Fundamentals

**Language:** German UI strings throughout. All system labels, settings, notifications, and button text are in German. When building designs, use German copy to match the authentic feel: Einstellungen (Settings), Nachrichten (Messages), Dateien (Files), Mitteilungen (Notifications), Alle löschen (Clear all).

**Tone:** Clean, functional, minimal. No marketing copy in the OS UI — only action-oriented labels. Copy is direct and imperative where needed (e.g. "Alle Schließen" not "Would you like to close all?").

**Casing:** Sentence case for UI labels. German nouns are always capitalised (standard German grammar).

**Numbers:** Percentages shown for battery (78%), no decimal places in system UI.

**Emoji:** Used sparingly and intentionally. Only in media player album art placeholder (🎵) and no other persistent UI. Not used as icons.

**Character style:** Terse. Single-word or two-word labels wherever possible. Time in `de-DE` locale (24-hour HH:MM format).

---

## Visual Foundations

### Colors
- **Primary brand:** Near-black navy `#030213` — used for solid button fills, icon text on light mode.
- **Destructive:** Red `#d4183d` — for delete/danger actions.
- **Light mode surfaces:** Pure white `#ffffff` background; `#ececf0` muted; `#e9ebef` accent.
- **Dark mode surfaces:** Near-black `oklch(0.145 0 0)` ≈ `#1c1c1e` (iOS dark background). Secondary/muted at `oklch(0.269 0 0)` ≈ `#383838`.
- **Blue accent (active toggles):** `rgba(59,130,246,0.85)` — WiFi on, Bluetooth on, active states.
- **Orange (airplane mode):** `rgba(249,115,22,0.85)`.
- **Teal / Violet (mode toggles):** `rgba(20,184,166,0.85)` desktop / `rgba(139,92,246,0.85)` tablet.
- **Notification dot:** `#ef4444` — always red, always top-right, 6px diameter.

### Typography
Primary font: **Inter**, with **Noto Sans** as fallback (both loaded from Google Fonts), then the native system stack (`-apple-system`, Segoe UI, `system-ui`). Inter gives the OS a consistent, neutral UI face across every platform; Noto Sans covers the broadest glyph range when Inter is unavailable.
- H1: `--text-2xl` / 500 weight
- Body: `--text-base` (14px) / 400 weight  
- Labels/buttons: `--text-base` (14px) / 500 weight
- Status bar / captions: `--text-xs` (11px) / 600 weight
- Icon labels: 11–13px / 600 weight, with text-shadow for wallpaper legibility

### Liquid Glass Surfaces
The entire UI is built on three glass levels:
1. **Panel** (`blur(40–64px)`, `rgba(255,255,255,0.10)` dark / `0.50` light) — dock, overlays, control center
2. **Card** (`blur(20px)`, `rgba(255,255,255,0.08)` dark / `0.60` light) — cards inside panels  
3. **Subtle** (`blur(12px)`, `rgba(255,255,255,0.06)` dark / `0.70` light) — settings rows, list items

All glass surfaces have a **top-shine overlay**: `linear-gradient(to bottom, rgba(255,255,255,0.10-0.15), transparent)` pinned to the inner top edge.

### Backgrounds & Wallpapers
Full-bleed photography from Unsplash — dark mode: tech/circuit dark photo; light mode: mountain landscape. A tint overlay (`rgba(0,0,0,0.35)` dark / `rgba(255,255,255,0.20)` light) reduces contrast without washing out the image.

### Border Radius
The UI uses large radii — panels are `border-radius: 24px (--radius-3xl)`, cards `16px (--radius-2xl)`, app icon backings `12–20px` depending on size. Pill shapes (`border-radius: 9999px`) are used for dock pills, status bar buttons, and the Dynamic Island. The base token is `--radius-base: 10px` (used for inputs and small controls).

### Shadows
- Panels: `box-shadow: 0 25px 50px rgba(0,0,0,0.40-0.60)` — deep drop shadow, stronger in dark mode.
- App icons: `0 4px 12px rgba(0,0,0,0.20)`, grows to `0 8px 20px rgba(0,0,0,0.30)` on hover.
- Active toggles: colour-matched glow (`box-shadow: 0 4px 12px rgba(59,130,246,0.30)`).

### Animations & Motion
All curves and durations are tokens in `tokens/motion.css` — components reference them (`var(--motion-spring)` etc.), never hard-code new curves.
- **Spring expand** (`--motion-spring`, panel/overlay entry): `cubic-bezier(0.34, 1.4, 0.5, 1)`, 0.5s — elastic overshoot.
- **Soft spring** (`--motion-spring-soft`, thumbs/press-release): `cubic-bezier(0.34, 1.2, 0.5, 1)`, 0.28s.
- **Icon spring** (`--motion-spring-icon`): `cubic-bezier(0.34, 1.56, 0.64, 1)` — strongest overshoot, hover pop.
- **Smooth collapse** (`--motion-smooth`): `cubic-bezier(0.4, 0, 0.2, 1)`, 0.4s — exits are quick and quiet, never bouncy.
- **Page swipe** (`--motion-glide`): `cubic-bezier(0.22, 1, 0.36, 1)`, 0.45s.
- **Press**: instant down (`scale(0.9–0.95)`, 0.1s), springy release. Toggles stretch the thumb along the travel axis while pressed.
- Shared keyframes (`nx-overlay-in`, `nx-menu-in`, `nx-sheet-in`, `nx-toast-rise`, `nx-scrim-in`) live in `motion.css`.
- `prefers-reduced-motion` collapses all durations to ~0 via the tokens.
- **No decorative looping animations** on desktop content.

### Hover & Active States
- Ghost/glass buttons: `hover:rgba(255,255,255,0.15)` (dark) / `rgba(0,0,0,0.08)` (light)
- Active pill (pressed): `rgba(255,255,255,0.20)` dark / `rgba(0,0,0,0.12)` light
- Icons: `scale(1.08)` hover, `scale(0.92)` press, elastic spring

### Borders
All glass surfaces: 1px solid `rgba(255,255,255,0.18–0.20)` in dark mode, `rgba(255,255,255,0.75–0.80)` in light mode. No borders on subtle surfaces.

### Blur
- `backdrop-filter: blur(8px)` — toggles/switches
- `backdrop-filter: blur(20px)` — inner elements, icon backings
- `backdrop-filter: blur(40px)` — control center, panels
- `backdrop-filter: blur(64px)` — dock, app launcher backdrop

### Transparency & Layering
The UI is designed for perpetual layering — wallpaper → tint overlay → glass panels → glass cards → controls. Every layer must be translucent to let lower layers bleed through the blur.

### Corner Imagery
No illustrations, gradients as decoration, or hand-drawn elements. Only photography wallpapers (Unsplash) and functional UI surfaces. The design is austere and functional.

---

## Iconography

**Custom app icons:** 60+ SVG icons in `assets/icons/`. These are rounded, colourful, multi-layer SVG icons similar to iOS/macOS app icons. `music.svg` is the Musik *folder* icon; `music-app.svg` is the Music *app* icon. Each has a transparent background — the glass backing square is rendered by the UI, not embedded in the SVG. Icons are always rendered inside a glass-backed rounded rectangle.

**System/UI icons:** Lucide React (`lucide-react@0.487.0`) is used throughout for all system icons (WiFi, Battery, Bell, Search, X, ChevronDown, etc.). Stroke weight 2, size 14–16px in status bar, 14–18px in panels. No fill icons except for a few specific cases (audio bars).

**No emoji used as icons.** Unicode characters not used as icons either — all iconography is SVG.

**CDN usage:** Load Lucide from CDN in HTML prototypes:
```html
<script src="https://unpkg.com/lucide@latest/dist/umd/lucide.min.js"></script>
```

**App icon usage:** Always wrap in a glass backing `<div>` with:
```css
background: rgba(255,255,255,0.10–0.12);
border: 1px solid rgba(255,255,255,0.15–0.18);
backdrop-filter: blur(20px);
border-radius: 12–20px; /* size-dependent */
```
Plus the top-shine pseudo-element gradient.

---

## Notification & Feedback System

The OS has five distinct surfaces that show notifications, status, and feedback. To keep them from overlapping or "shouting twice", every message is routed by three questions:

1. **Who sends it?** — *System* vs. *App* vs. *a running activity with live state*
2. **How long does it live?** — *transient* (an event, auto-dismisses) vs. *persistent* (running, has a state)
3. **Must the user act?** — *passive* (info) vs. *active* (answer a call, control media)

This yields exactly **one rule per surface** — no overlap.

### The five surfaces

| Surface | What belongs here | Source | Lifespan | Position |
|---|---|---|---|---|
| **Activity Runner** *(the live pill)* | Running activities *with live state* + transient high-priority | App **or** System | persistent (while the activity runs) | top centre |
| **Mitteilungen** (Notification Center) | App notifications, passive, stackable | App | persistent until dismissed | top/right, expandable |
| **Control Center** | **Only** media-playback control | System (audio routing) | persistent (mirrors state) | Control Center panel |
| **System-Toasts** | Transient system events with no action | **System only** | transient (3–5 s, auto-dismiss) | left edge, slide-in |
| **Background Jobs** *(in the taskbar)* | Local background processes + media mini | System / local app | persistent (while job runs) | bottom bar |

### Routing rules (the actual rulebook)

**Activity Runner** (the black top-centre live pill — our name for the "dynamic island" concept, deliberately not Apple-branded) = "something is running *right now* and I want to glance at it / control it fast."
- Incoming call (transient, high prio → auto-expands), flashlight on, music *when the app is backgrounded/closed*, navigation *when the app is closed*, timer/recording running.
- **Rule: live state + needs glance/quick-action = Activity Runner.** Compact by default; expands on tap/hover or priority escalation.
- When the source app is in the **foreground**, the Runner shows nothing for it — it is only for what is *absent from view*.

**Mitteilungen** = "an app wants to tell me something; I can react later."
- Messages, mail, app pushes, reminders.
- **Rule: comes from an app, is passive, has no live state = Notification.** Stacked, persistent, swipeable.

**Control Center** = "the permanent switch for whatever is *currently* playing."
- Only the now-playing media card (play/pause/skip/scrubber/output).
- **Rule: Control Center only mirrors playback state, never generates events.** No toasts, no app pushes here.

**System-Toasts** (left) = "the system briefly confirms something; you don't have to do anything."
- "Update fertig", "Screenshot gespeichert", "Bluetooth verbunden", volume, "Nicht stören an".
- **Rule: system only, never app. Transient, auto-dismiss, no action.** If it needs an action → it becomes a Mitteilung. If it keeps running → it becomes an Activity Runner.

**Background Jobs** (in the desktop taskbar, bottom — our name for the local-process tray) = "local jobs that take time."
- Compiling, rendering, export, download, file copy + the media mini-player.
- **Rule: persistent local process with progress = Background Jobs.** Shows progress; **max. 3 visible at once**, the rest collapse into a "+N" overflow chip.

> **Naming:** we use **Activity Runner** (not "Dynamic Island") and **Background Jobs** (not "taskbar tray") throughout the product to keep the vocabulary our own and Apple-neutral.

### Priority hierarchy (who wins when it gets crowded)

```
1  Critical    — call, alarm, critical battery     → Activity Runner expands, interrupts
2  Active-live — navigation, timer, recording        → Activity Runner compact
3  System ack  — update/screenshot/connect            → Toast left (transient)
4  App notice  — push, mail, chat                      → Mitteilungen (stacked)
5  Background  — render/compile/download               → Background Jobs (with progress)
   Media (no event, always a state)                    → Activity Runner (app closed) + Control Center + Background-Jobs mini
```

### Migration rules (prevents duplicates)

- **Media** exists as *one* state, mirrored in up to three places (Activity Runner / Control Center / Background Jobs) — never as a Notification.
- **Toast → Mitteilung:** if a system event needs an action or should persist, it moves out into the Mitteilungen instead of disappearing.
- **One per type:** never let the same fact "shout" in two surfaces at once (e.g. a call lives in the Activity Runner only, never additionally as a Toast).

### Design lineage
This mirrors **Apple's** clean 3-way split — Live Activities/Dynamic Island (running state) vs. Notification Center (app pushes) vs. Control Center (persistent switches incl. now-playing) — which is why the system never feels doubled. From **Xiaomi/HyperOS** we take the explicit **priority tiers** (important vs. silent) and the **max-N + overflow** treatment for background-task visibility in the taskbar.

---

## File Index

```
styles.css                          Global CSS entry point (@imports only)
tokens/
  colors.css                        All color custom properties (light + .dark)
  typography.css                    Font family, weight, size tokens
  spacing.css                       Spacing, radius, shadow, z-index tokens
  glass.css                         Liquid glass effect tokens
  motion.css                        Motion tokens: easing curves, durations, shared keyframes
  fonts.css                         Font imports (Inter + Noto Sans from Google Fonts)
assets/
  icons/                            29 SVG app icons (archive, books, calculator…)
components/core/                    Foundational surfaces & primitives
  GlassButton.jsx + .d.ts           Primary interactive button (6 variants, 4 sizes)
  Badge.jsx + .d.ts                 Status chip (8 variants)
  GlassCard.jsx + .d.ts             Glass container panel (5 surface levels)
  AppIcon.jsx + .d.ts               App icon tile with glass backing (5 sizes)
  GlassToggle.jsx + .d.ts           iOS-style on/off toggle
  core.card.html                    Showcase card
components/controls/                Form controls
  Segment.jsx                       Segmented control with sliding thumb
  Slider.jsx                        Range slider (drag + tap)
  GlassCheckbox.jsx                 Rounded-square checkbox
  GlassRadioGroup.jsx               Single-select radio list
  Stepper.jsx                       −/+ numeric stepper
  Select.jsx                        Glass dropdown (frosted panel, not native)
  Rating.jsx                        Star rating (interactive / read-only)
  WheelPicker.jsx                   iOS snap-scroll wheel column
  DatePicker.jsx                    Day·month·year glass date wheel
components/inputs/                  Text entry
  TextField.jsx                     Text input (label, icon, error)
  SearchBar.jsx                     iOS search pill with clear button
  TextArea.jsx                      Multiline input (counter, auto-grow)
components/feedback/                Status & progress
  Spinner.jsx                       Activity indicator (12 spokes)
  ProgressBar.jsx                   Determinate / indeterminate bar
  Toast.jsx                         Transient floating notification
  Skeleton.jsx                      Shimmer loading placeholder (+ SkeletonText)
  Banner.jsx                        Inline status strip (4 variants)
  Refresher.jsx                     Pull-to-refresh wrapper
components/overlays/                Dialogs & floating surfaces
  Modal.jsx                         Centered glass dialog
  ActionSheet.jsx                   Bottom action list (iOS-style)
  Alert.jsx                         Compact confirmation dialog
  Popover.jsx                       Anchored menu panel (+ PopoverItem)
  Menu.jsx                          Dropdown + context menu (+ ContextMenu)
  Tooltip.jsx                       Hover/focus glass label
  FAB.jsx                           Floating action button (expandable)
components/navigation/              Chrome, data & layout
  Toolbar.jsx                       Top title/navigation bar
  TabBar.jsx                        Bottom tab navigation
  ListItem.jsx                      Settings/list row (+ List wrapper)
  SubHeader.jsx                     Section header row
  Accordion.jsx                     Collapsible disclosure group
  Sidebar.jsx                       Nav rail / drawer (+ SplitView)
  TreeView.jsx                      Collapsible file/folder tree
  Breadcrumbs.jsx                   Path navigation trail
  Pagination.jsx                    Page navigation (arrows + pills)
  Avatar.jsx                        User avatar (image / initials + status)
  Chip.jsx                          Selectable / removable token
  *.card.html                       Showcase cards per folder
components/window/                  Window shell — the chrome every app/settings window is built from
  Window.jsx + .d.ts                Glass window shell: surface + 3-zone title chrome + body slot
  WindowPane.jsx + .d.ts            Inner content card (optional header + scrolling body)
  WindowControls.jsx + .d.ts        Minimise / maximise / close cluster
  WindowButton.jsx + .d.ts          Chrome icon button (the former per-template `.ibtn`)
  Icon.jsx + .d.ts                  Lucide-style SVG renderer (replaces per-template `_ic()`)
  window.card.html                  Showcase card

Overlay material: Modal · Alert · ActionSheet · Popover · Menu · Tooltip · Select
dropdown · Toast · FAB sit on a dedicated dense reading material
(--glass-overlay-bg, ~0.80–0.82 opacity + saturate) so text stays crisp —
distinct from the lighter control-center --glass-panel-bg. Scrims use
--glass-scrim. All overlay/reading surfaces are token-driven and switch
dark/light automatically.
guidelines/
  colors-brand.card.html            Brand + semantic colors
  colors-surface.card.html          Light mode surface colors
  colors-dark.card.html             Dark mode surface colors
  colors-charts.card.html           Chart color palette
  glass-dark.card.html              Glass levels on dark wallpaper
  glass-light.card.html             Glass levels on light wallpaper
  type-scale.card.html              Font size scale
  type-weights.card.html            Font weights + stack
  radius.card.html                  Border radius scale
  spacing.card.html                 Spacing scale
  shadows.card.html                 Shadow scale
  animation.card.html               Easing curves + interaction patterns
  app-icons.card.html               All 29 app icons
  activity-runner.card.html         Activity Runner (live pill): music, call, nav, timer, flashlight
  background-jobs.card.html         Background Jobs in the taskbar: progress, max-3 + overflow, media mini
  system-toast.card.html            System Toast glass cards (left edge)
ui_kits/os_launcher/
  index.html                        Full interactive OS Launcher prototype
readme.md                           This file
SKILL.md                            Agent skill definition
```

---

## Using This Design System

### In HTML prototypes
```html
<link rel="stylesheet" href="path/to/styles.css">
<script src="path/to/_ds_bundle.js"></script>
<script>
  const {
    // core
    GlassButton, Badge, GlassCard, AppIcon, GlassToggle,
    // window shell
    Window, WindowPane, WindowControls, WindowButton, Icon,
    // controls
    Segment, Slider, GlassCheckbox, GlassRadioGroup, Stepper, Select, Rating, WheelPicker, DatePicker,
    // inputs
    TextField, SearchBar, TextArea,
    // feedback
    Spinner, ProgressBar, Toast, Skeleton, SkeletonText, Banner, Refresher,
    // overlays
    Modal, ActionSheet, Alert, Popover, PopoverItem, Menu, ContextMenu, Tooltip, FAB,
    // navigation, data & layout
    Toolbar, TabBar, List, ListItem, SubHeader, Accordion, Sidebar, SplitView, TreeView, Breadcrumbs, Pagination, Avatar, Chip,
  } = window.AIVAOSDesignSystem_afec52;
</script>
```

All components are styled in the same liquid-glass language as the core set and the OS window / Control Center: they read `--glass-*` and `--color-*` tokens and adapt to `.dark` automatically. Every interactive control supports both controlled (`value` + `onChange`) and uncontrolled (`defaultValue`/`defaultChecked`) usage.

### Reproducing the glass aesthetic
1. Put a `background-image` wallpaper behind everything
2. Add a tint overlay (`rgba(0,0,0,0.35)` for dark)
3. Use `GlassCard variant="panel"` or `variant="dock"` for all UI surfaces
4. Use `AppIcon` for all app icon tiles
5. Use `GlassButton variant="glass"` for most buttons; `variant="active"` for blue toggle state
6. Use `GlassToggle` for binary settings
7. Text on glass: `rgba(255,255,255,0.90)` primary, `rgba(255,255,255,0.45)` secondary

### Dark / Light mode
Add `.dark` class to `<html>` or any ancestor element to activate dark mode tokens. All components read from CSS custom properties and adapt automatically.

### Windows: one base, many apps
Every app / settings window is composed from the **`AppWindow`** component — never hand-rolled. `AppWindow` wraps the `Window` shell (dense surface + three-zone title chrome) with a sidebar · content-pane · properties-pane body, an optional floating `WindowActionBar`, and a responsive layout that lifts the side panes into glass overlays as the window narrows. Provide identity chrome via the `leading` / `toolbar` / `trailing` slots (built from `WindowButton` + `Icon` + `Menu`); the sidebar- and properties-toggle buttons are added automatically.

The window templates demonstrate the pattern: **OS Window Frame** is the full reference (window-mode + app menus, properties, action bar); **OS App Window** (file manager) and **OS Settings Window** are thinner compositions of the same `AppWindow`. The dense window look lives in the `--glass-window-*` tokens — one source of truth. Launcher panels (Control Center, notifications) are *not* windows; they use `--glass-panel-*` instead.
