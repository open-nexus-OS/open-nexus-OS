# Open Nexus OS — App Icon Set

48 app icons across 4 categories, each in 3 variants. SVG-only, Apple-like / visionOS-inspired design language.

## File Structure

```
icons/
├── productivity/   19 apps
├── media/           9 apps
├── games/           3 apps
└── system/         17 apps
```

Each app has three files:

- `{name}.svg` — **Main icon** (1024×1024 viewBox). Full color with squircle background, gradients, and detailed symbol.
- `{name}.micro.svg` — **Micro variant** (24×24 viewBox). Simplified geometry for display at ≤24px.
- `{name}.symbolic.svg` — **Symbolic variant** (24×24 viewBox). Monochrome, uses `currentColor`, no background — for theming and small-size monochrome rendering.

## Design System

- **Squircle base**: all main icons use `rect rx="230"` on a 1024 canvas (~22% corner radius, Apple-like)
- **Vertical linear gradients**: light top → dark bottom
- **No filters or blur** — depth achieved with stacked translucent shapes, gradients, and inner strokes (per `Icon Design Guidelines`)
- **Inner stroke**: 3px white at 18% opacity for subtle bevel
- **Symbol size**: ~50–65% of canvas, centered
- **Layer structure**: `data-layer="background"` and `data-layer="symbol"` for system theming

## Special: Stash (folder template)

Stash is the **only icon without a squircle background** — it's the freestanding folder template that other folders inherit. Its SVG declares two recolorable layers:

```xml
<g data-layer="background" data-customizable="background">…</g>
<g data-layer="symbol" data-customizable="folder-color">…</g>
```

The renderer can swap `folder-color` per folder and `background` per user preference (e.g. tinted, transparent, glass).

---

## Productivity (19)

| Name | Beschreibung |
|---|---|
| **Locus** | Web browser — find your place on the internet |
| **Synapz Docs** | Word processor for documents and long-form writing |
| **Synapz Sheets** | Spreadsheet with formulas, charts, and tables |
| **Synapz Slides** | Presentations with themes and animations |
| **Glyph** | Quick notes, offline-first, share-targets |
| **Relay** | Email client (IMAP/SMTP), offline-first |
| **Cadence** | Calendar and contacts in one PIM app |
| **Pulse** | RSS/Atom feed reader with offline read-later |
| **Recipes** | Cookbook with meal plans and nutrition info |
| **Staff** | PDF sheet-music reader with setlists |
| **Cue** | Smooth-scrolling teleprompter with mirror mode |
| **Cipher** | Password manager and generator, keystore-backed |
| **Weather** | Location-gated forecasts, cache-first |
| **Atlas** | Online and offline maps with routing |
| **Stash** | File manager — the folder template itself |
| **Nib** | Plain text editor |
| **PDF Viewer** | View, annotate, and share PDFs |
| **Markup** | Markdown viewer and renderer |
| **Image Viewer** | View images and basic adjustments |

## Media (9)

| Name | Beschreibung |
|---|---|
| **Iris** | Photo library with albums and editing |
| **Music** | Music player and library |
| **Reel** | Video player and library |
| **Inkwell** | 2D paint and illustration (Procreate-class) |
| **Vertex** | 3D modeling and sculpting (SketchUp/Shapr3D-class) |
| **Resonance** | Digital Audio Workstation, multitrack + MIDI + plugins |
| **Cinescope** | Non-linear video editor with timeline |
| **Beacon** | Live capture, scene compose, and stream (OBS-class) |
| **Echo** | Podcast player with offline downloads and queue |

## Games (3)

| Name | Beschreibung |
|---|---|
| **Gaming Hub** | Bundled arcade games — Breakout, Asteroids, Snake |
| **Flipper** | Physics-driven pinball reference game |
| **Tessera** | Touch-first, accessibility-forward puzzle game |

## Dev / System (17)

| Name | Beschreibung |
|---|---|
| **NeX Studio** | Integrated development environment with build + debug |
| **Console** | Terminal with tabs, PTY, and safe clipboard |
| **Plaza** | App store for discovery, install, and publishing |
| **Tuner** | System settings and preferences |
| **Dial** | Phone dialer and call history |
| **Shutter** | Camera with photo and video capture |
| **Calculator** | Basic and scientific calculator |
| **Chronos** | Clock, alarms, timers, world time, stopwatch |
| **Whisper** | Voice memos and audio recording |
| **Lingua** | Text and voice translation |
| **Cabinet** | E-book reader and library |
| **Needle** | Compass with directional and elevation info |
| **Purse** | Wallet for cards, passes, and payment |
| **Trace** | Find My Device — locate trusted devices |
| **Cora** | Voice assistant — system-wide AI helper |
| **Aiva** | Health and fitness tracking |
| **Family** | Family Mode — household, guardians, approvals |

---

## Frameworks / SDKs (not apps, no icons)

| Name | Beschreibung |
|---|---|
| **NexusGfx** | Metal-like graphics SDK |
| **NexusMedia** | Audio/video/image processing SDK |
| **NexusNet** | Cloud and distributed software bus SDK |
| **NexusInfer** | On-device machine learning SDK |
| **NexusGame** | Games SDK |
| **QuerySpec** | Database query language |
| **NeX DSL** | Domain-specific language for the OS |

## Nexus-Familie Apps (not yet iconized)

The seven Nexus-prefixed apps below were not iconized in this batch:

| Name | Beschreibung |
|---|---|
| **NexusFrame** | Photo/design editor (Pixelmator-class) |
| **NexusVideo** | Federated video platform (PeerTube-based) |
| **NexusAccount** | Optional cloud account provider |
| **NexusSocial** | Fediverse microblog |
| **NexusMoments** | Fediverse photo sharing |
| **NexusChat** | Federated messaging |
| **NexusForum** | Federated forums |

---

## Trademark Notes

Some names have potential conflicts in adjacent product categories — verify with a trademark search (EUIPO TMview, USPTO TESS) before public launch. Particularly flagged:

- **Synapse / Synapz** — multiple SaaS uses (Razer Synapse, Matrix Synapse, Synapse Audio)
- **Locus** — Locus Map (Android maps app, different category)
- **Echo** — Amazon Echo (different category but famous mark)
- **Whisper** — OpenAI Whisper (same functional space: voice/speech)
- **Cora** — Cora.ai (AI meeting assistant, adjacent category)
- **Vertex** — Vertex Pharmaceuticals + Google Vertex AI
- **Cadence** — Cadence Design Systems
- **Reel** — Instagram Reels (descriptive use likely OK)
- **Aiva** — AIVA.ai (AI music composition, different category)

The remaining names are either descriptive (Weather, Calculator, Music, etc.), historic words (Atlas, Compass, Wallet), or generic enough to be low-risk — but final verification before launch is recommended.

## License

Icons released as part of Open Nexus OS. See repository LICENSE.
