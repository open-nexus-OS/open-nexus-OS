---
title: TRACK Recipes App (Recime-class): personal cookbook + meal plans + nutrition, share/import from Moments/Video (bounded, offline-first)
status: Draft
owner: @apps @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Content/URIs + picker + grants: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md, tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md, tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Share v2 intents + chooser: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md, tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - NexusMoments (recipe attachment offer): tasks/TRACK-NEXUSSOCIAL.md
  - NexusVideo (recipe attachment offer): tasks/TRACK-NEXUSVIDEO.md
  - NexusFrame (optional recipe cards / graphics): tasks/TRACK-NEXUSFRAME.md
---

## Goal (track-level)

Deliver a first-party **Recipes** app comparable to Recime:

- personal recipe library (offline-first),
- structured recipes:
  - ingredients (grouped: main / side / sauce, etc.),
  - steps/instructions,
  - servings, timings,
  - nutrition summary (calories/macros; v1 can be user-entered),
- shopping list (optional later),
- import/share flows from other apps.

Twist (integration):

- NexusMoments and NexusVideo can attach a **recipe** to a post, and our clients render a
  “Open recipe” button that imports it into Recipes.

## Non-goals (avoid drift)

- Not a diet tracker/medical app in v1.
- No unbounded web scraping/crawling in v1.
- No secret/hidden metadata that surprises users: the attachment must have a clear fallback representation.

## Authority model (must match registry)

Recipes is an app. It consumes:

- `contentd`/`mimed`/`grantsd` for import/export (no raw filesystem paths),
- `policyd` for any sensitive operations (e.g., future cloud sync),
- `logd` for audit/log sink (no secrets; no personal data leaks).

## Data model stance (v1)

- Canonical local model is structured and bounded:
  - title, tags
  - ingredient groups: `[ { name, items[] } ]`
  - steps: `[ { text, timers? } ]`
  - nutrition: `{ calories_kcal, protein_g, carbs_g, fat_g }` (optional)
- Storage is offline-first under `/state` with bounded indices and deterministic ordering.

## “Recipe attachment” integration (portable, Fediverse-safe)

We support two tiers so it works on third-party instances without becoming a proprietary fork:

### Tier 1 (portable): recipe block in caption/description (text fallback)

- When composing a post, the app can include a bounded “Recipe block” in the caption/description.
- Our clients detect it and render a **button** (“Open recipe”), optionally collapsing the block in UI.
- Other clients will still show the text, which is acceptable and non-deceptive.

### Tier 2 (enhanced, optional): structured attachment

- On servers that support it (e.g., Nexus-hosted instances), include a structured attachment
  with an explicit media type (implementation detail to be chosen later).
- Still keep a short human-readable fallback in the caption.

Import mechanism:

- Use Share v2 / Intents dispatch to Recipes with a `content://` URI (or text payload within budgets),
  with scoped grants where needed (`TASK-0126`, `TASK-0127`, `TASK-0084`).

## Phase map

### Phase 0 — Recipes core (host-first)

- recipe model + bounded validation
- deterministic save/load + list ordering
- import of “recipe block” text format (fixtures)

### Phase 1 — OS wiring + share target

- register Recipes as a Share/Intent target (receives recipe block or content:// payload)
- import from Moments/Video into Recipes via chooser/targets with grants enforcement

### Phase 2 — Media attachments (photo/video links) + polish

- attach a photo (or extracted video frame) as recipe cover via content:// grants
- optional “recipe card” export (image/PDF) using NexusFrame/print pipeline (later)

## Candidate subtasks (to be extracted into TASK-XXXX)

- **CAND-RECIPES-000: Recipe model v0 (groups/steps/nutrition) + bounded validation + tests**
- **CAND-RECIPES-010: Portable recipe block format v0 (parse/emit) + fixtures**
- **CAND-RECIPES-020: Recipes share/intent target v0 (import) + grants enforcement**
- **CAND-RECIPES-030: Moments/Video compose integration v0 (attach recipe + button)**
- **CAND-RECIPES-040: Cover media v0 (use photo; extract frame from video if available)**

## Extraction rules

Candidates become real tasks only when they:

- define explicit bounds (max block bytes, max ingredients/steps),
- provide deterministic tests (fixtures),
- do not introduce new URI schemes or parallel authorities,
- keep secrets/personal data out of logs.
