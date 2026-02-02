---
title: TRACK Fediverse Apps (NexusSocial + NexusMoments + NexusChat + NexusForum): complete Fediverse suite with first-party UX (no inofficial bridges)
status: Draft
owner: @apps @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation / System Surfaces (avoid per-app chat/compose reimplementation): tasks/TRACK-SYSTEM-DELEGATION.md
  - NexusNet SDK (network/OAuth surfaces): tasks/TRACK-NEXUSNET-SDK.md
  - Zero-Copy App Platform (UI primitives/caching patterns): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Video Editor app (shared editor offer): tasks/TRACK-VIDEO-EDITOR-APP.md
  - NexusVideo (PeerTube client; long-form + creator economy): tasks/TRACK-NEXUSVIDEO.md
  - Recipes app (recipe attachment import target): tasks/TRACK-RECIPES-APP.md
  - Intents/share (targets + chooser): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md, tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - Scoped URI grants (content://): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Document picker + Open With: tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Accounts/Identity v1.2 (multi-user + sessiond + keystore binding): tasks/TASK-0223-accounts-identity-v1_2a-host-multiuser-sessiond.md
  - Accounts/Identity v1.2 (OS wiring + secure home): tasks/TASK-0224-accounts-identity-v1_2b-os-securefs-home-greeter-keystore.md
  - Backup/Restore v1 (NBK + backupd): tasks/TASK-0161-backup-restore-v1a-host-nbk-format-pack-verify-restore.md
  - Backup/Restore v1 (OS/QEMU): tasks/TASK-0162-backup-restore-v1b-os-backupd-settings-cli-selftests-docs.md
  - TRACK NexusAccount (online account provider + cloud storage/sync): tasks/TRACK-NEXUSACCOUNT.md
---

## Goal (track-level)

Deliver a **complete Fediverse app suite** for Open Nexus OS that:

- feels familiar and fast (recognizable UX patterns from mainstream apps),
- is **Fediverse-only** (open protocols, no scraping, no proprietary "compat modes"),
- is capability/policy-gated and auditable (no ambient network authority),
- is deterministic and testable (host-first proofs; OS/QEMU markers only after real behavior),
- and is legally safe (own brand, own UI identity; no confusing ties to US proprietary products).

### Apps in this track

1. **NexusSocial** (Mastodon/ActivityPub) — Microblogging (Twitter/X alternative)
2. **NexusMoments** (Pixelfed/ActivityPub) — Photo+video sharing (Instagram alternative)
3. **NexusChat** (Matrix) — Messaging (WhatsApp/Telegram alternative)
4. **NexusForum** (Lemmy/ActivityPub) — Communities (Reddit alternative)

## Scope boundaries (anti-drift)

- These are **clients**, not new server/federation implementations.
- **No inofficial bridges** to proprietary platforms (Meta/X/YouTube/etc.). (Hard rule.)
- No "algorithmic For You" / engagement dark patterns.
- No kernel changes as part of Fediverse app tasks (split into prerequisites if ever needed).
- Video (PeerTube) is tracked separately in `TRACK-NEXUSVIDEO.md` due to creator economy complexity.

## Relationship to NexusAccount (online) vs OS identity (local)

All Fediverse apps must work with **local OS users** (identityd/sessiond) and may optionally integrate with
**NexusAccount** as an *online account provider* for cross-device features. Key stance:

- **Local login is local**: the OS user/session model is owned by `identityd`/`sessiond` (see v18 + v1.2 tasks).
- **NexusAccount is optional**: it should never be required to use any Fediverse app with a third-party instance.
- **No global tokens**: online account credentials must follow the NexusNet account model (per-app grants; tokens are secrets).

## Product stance (what “feels like the American apps” means here)

We target *familiar interaction patterns* while keeping the product clearly distinct:

- **Fast path**: open → home timeline rendered quickly (cache + skeleton UI).
- **Compose**: one-tap posting, drafts, clear primary action, predictable error UX.
- **Threads**: context + replies with collapse/expand, “jump to parent”.
- **Notifications**: grouped, scannable, actionable.
- **Polish**: haptics/animations only when deterministic and testable; accessibility baseline.

We explicitly do **not** copy brand identifiers or trade dress.

## Legal + brand guardrails (non-negotiable)

- **No trademark confusion**:
  - do not use third-party logos, names, or "official for a platform" phrasing,
  - do not use "Twitter mode / Instagram mode" wording.
- **No trade dress copying**:
  - do not replicate exact icon sets, color palettes, or layout proportions 1:1.
- **Protocol is fine**:
  - ActivityPub and Mastodon-compatible HTTP APIs are open/standardized and safe to implement.

## Shared architecture principles (all apps)

### Authority model (who owns what)

- Each app is a **standalone app** (UI + app domain).
- Network operations are capability-gated and should prefer shared NexusNet surfaces (where available).
- Secrets live in **`keystored`** (never logs, never plain storage).
- Policy decisions live in **`policyd`** (deny-by-default; auditable grants).

### Account model stance (per-app grants; no token strings in apps)

Where any Fediverse app uses OAuth (Fediverse instance OAuth or NexusAccount OAuth), it must follow the NexusNet
"account provider" model:

- Accounts are added/managed in Accounts UI (system surface) when possible.
- Apps request **scoped per-app grants** (`account.use`-style), revocable and auditable.
- Tokens are stored in `keystored` and must never appear in logs or crash reports.

### Recommended layering (implementation guidance)

- **Protocol client**: strict bounds, stable error model, deterministic pagination ordering.
- **Domain layer**: timeline/threads/notifications models, cache policy, merge rules.
- **UI layer**: rendering + interaction; no direct HTTP/OAuth calls from widgets.

---

## App 1: NexusSocial (Mastodon/ActivityPub — Microblogging)

### Product positioning (NexusSocial)
"Like Twitter/X, but you own your data and choose your community"

### MVP feature set (v1)

#### Account + onboarding
- Instance selection:
  - "Recommended instances" list (configurable) + "enter custom instance" (advanced).
  - Strict URL validation and allowlist/denylist hooks (policy-driven).
- OAuth sign-in (where supported by the instance) with token storage in `keystored`.
- Account switching (at least 2 accounts) with explicit per-account grants.

**Optional fast-path (product)**:
- Offer "Create a Nexus account" as a recommended onboarding path **only** as a convenience.
  - It must be clearly presented as "a recommended Fediverse instance account" (not a special protocol).
  - Users must be able to choose another instance (advanced).

#### Core social flows
- Home timeline: load/refresh/paginate, deterministic ordering.
- Post renderer: text, links, CW/spoiler, basic content warnings UI.
- Compose: post, reply, CW, visibility controls (public/unlisted/followers/direct if supported).
- Thread view: context + replies + reply action.
- Profiles: view profile + posts, follow/unfollow (policy-gated if desired).
- Notifications: mentions/replies/boosts/follows; deterministic grouping rules.

#### Non-goals (v1 - NexusSocial)
- DMs (use NexusChat instead).
- Full moderation tooling / admin panels.
- Advanced media editing, full offline queueing, push notifications (can come later).

---

## App 2: NexusMoments (Pixelfed/ActivityPub — Photo+Video Sharing)

### Product positioning (NexusMoments)
"Like Instagram, but no algorithms and you own your photos"

### MVP feature set (v1 - NexusMoments)

#### Account + onboarding (NexusMoments)

- Pixelfed instance selection (similar to NexusSocial).
- OAuth sign-in with `keystored` token storage.
- Optional: "Create a Nexus account" fast-path (same model as NexusSocial).

#### Core photo flows
- **Feed**: chronological photo feed from followed accounts.
- **Grid view**: user profiles show photo grid (Instagram-style).
- **Post**: upload photo (single or carousel), caption, hashtags, location (optional).
- **Stories** (optional v1.1): 24-hour ephemeral posts (if Pixelfed instance supports).
- **Likes + Comments**: interact with posts.
- **Explore**: discover via hashtags (bounded, policy-gated).

#### Media handling
- Image upload: JPEG/PNG/HEIC, max 10MB per image (configurable).
- Basic filters (optional v1.1): brightness/contrast/saturation.
- Strict bounds: max 10 images per post, max resolution 4K.

#### Recipe attachment (optional v1.1)

NexusMoments can attach a **recipe** to a photo/video post:

- the post contains a bounded, portable “recipe block” fallback (text) so federation remains honest,
- NexusMoments renders it as an “Open recipe” button in our client UI and dispatches to the Recipes app
  via Share v2 / Intents (`TASK-0126`, `TASK-0127`) with scoped grants where applicable (`TASK-0084`),
- the Recipes app imports it into the user’s cookbook (`tasks/TRACK-RECIPES-APP.md`).

This feature is strictly bounded (max bytes, max steps/ingredients) and must not rely on proprietary bridges.

#### Video posts (v1 - NexusMoments)

NexusMoments supports **short-form video posts** (Instagram “Reels”-class *interaction*, not trade dress):

- single video post, or video in a carousel (instance/protocol permitting),
- playback is integrated with system media session surfaces when available (pause/seek/volume),
- upload is strictly bounded:
  - max duration (e.g. 90s default; configurable by policy),
  - max bytes per upload,
  - max resolution (e.g. 1080p default; configurable by policy),
  - max frame rate (e.g. 30fps default),
  - strict container/codec allowlist (v1: MP4/H.264 + AAC; expand later under NexusMedia).

**Editing stance (v1)**:

- NexusMoments itself stays lightweight:
  - trim/crop/rotate is allowed,
  - advanced editing is offered via the shared **Video Editor** app (`tasks/TRACK-VIDEO-EDITOR-APP.md`)
    using Open-With / Intents + scoped grants (`TASK-0083`, `TASK-0126`, `TASK-0127`, `TASK-0084`).

**When a video is out-of-bounds**:

- Offer “Post on NexusVideo instead” (PeerTube) and attach/share the resulting link/preview into NexusMoments.
  This keeps Gallery fun while respecting boundedness and avoiding an unbounded transcoding pipeline inside the app.

#### Non-goals (v1 - NexusMoments)
- Advanced editing (crop/rotate only; no AR filters).
- Shopping/commerce features.

---

## App 3: NexusChat (Matrix — Messaging)

### Product positioning (NexusChat)
"Like WhatsApp/Telegram, but end-to-end encrypted by default and decentralized"

### MVP feature set (v1 - NexusChat)

#### Account + onboarding (NexusChat)

- Matrix homeserver selection (similar to Fediverse instance selection).
- Matrix account creation or sign-in.
- Optional: "Create a Nexus account" fast-path (Matrix homeserver hosted by project).

#### Core messaging flows
- **Direct messages**: 1-on-1 chats with E2EE (Olm/Megolm).
- **Group chats**: create/join rooms, invite members.
- **Message types**: text, emoji reactions, read receipts.
- **Media sharing**: images, videos, files (bounded sizes).
- **Notifications**: new message alerts (policy-gated).

#### System Delegation stance (avoid per-app chat stacks)

- NexusChat is the **default system chat surface**.
- Other apps should delegate messaging UX to NexusChat (compose/send/share targets), instead of embedding their own chat.
- Future: SMS/MMS (and similar external transports) should deliver into the NexusChat inbox via an ingress adapter,
  not via separate user-facing “SMS apps”.

Transport extensibility note:
- NexusChat should be architected as a **multi-transport inbox**:
  - Matrix is the first supported protocol.
  - Additional transports (SMS/MMS, future open standards) can be added via policy-gated adapter services,
    selected by recipient address type (e.g., phone-number vs Matrix ID), without changing the core “chat surface” UX.

Data portability / anti-lock-in note:
- NexusChat (and any app that wants to be the default chat surface) must support **chat export + import/transfer**
  so users can switch defaults without losing their history (see `tasks/TRACK-SYSTEM-DELEGATION.md` and `TASK-0126C`).

#### Inline action cards (MVP comfort without mini-app runtime)

To keep “WeChat-like convenience” while preserving OS security:
- NexusChat should support **inline action cards** for low-risk flows like `track.inline`, `confirm.inline`, `open.inline`.
- Cards should persist as **snapshots** in the chat log; refresh is explicit (no background polling).
- Providers supply **data only**; NexusChat renders canonical UI (no embedded web/JS runtime).
  - See: `TASK-0126D-chat-action-cards-v0-host-inline-track-confirm-open.md`

#### Encryption
- **E2EE by default**: all DMs and private rooms use Matrix E2EE.
- **Key management**: device verification, key backup (via `keystored`).
- **Security indicators**: verified/unverified device badges.

#### Non-goals (v1 - NexusChat)
- Voice/video calls (v1.1+).
- Bridges to WhatsApp/Telegram (keep it pure Matrix).
- Bots/integrations (v1.1+).

---

## App 4: NexusForum (Lemmy/ActivityPub — Communities)

### Product positioning (NexusForum)
"Like Reddit, but community-owned and federated"

### MVP feature set (v1 - NexusForum)

#### Account + onboarding (NexusForum)

- Lemmy instance selection (similar to Mastodon).
- Account creation or sign-in.
- Optional: "Create a Nexus account" fast-path (Lemmy instance hosted by project).

#### Core community flows
- **Browse communities**: discover and subscribe to communities (subreddit-style).
- **Feed**: home feed (subscribed communities) + all feed (federated).
- **Post**: create text posts, link posts, image posts.
- **Comments**: threaded comments with upvote/downvote.
- **Moderation**: report posts/comments (basic).

#### Voting + sorting
- **Upvote/downvote**: Reddit-style voting.
- **Sort options**: hot, new, top (day/week/month/year/all).
- **Deterministic**: stable sort ordering for tests.

#### Non-goals (v1 - NexusForum)
- Moderator tools (ban/remove; v1.1+).
- Awards/coins (keep it simple).
- Chat rooms (use NexusChat instead).

---

## Shared onboarding strategy (all apps)

### "Nexus account" as convenience (optional)

All apps may offer:
```text
Welcome to [NexusSocial/NexusMoments/NexusChat/NexusForum]!

Quick start:
○ Create a Nexus account (recommended)
  → Works across all Nexus apps
  → Hosted in EU, GDPR-compliant

Advanced:
○ Use another server
  → Choose your own [Mastodon/Pixelfed/Matrix/Lemmy] instance
```

**Key principles:**
- "Nexus account" is just a default instance recommendation (not a special protocol).
- Users can always choose another instance.
- No vendor lock-in: users can migrate later (ActivityPub/Matrix support migration).

---

## Shared technical primitives (all apps)

### Protocol clients (reusable)
- `nexus-activitypub`: ActivityPub client (NexusSocial, NexusMoments, NexusForum).
- `nexus-matrix`: Matrix client (NexusChat).

### UI components (reusable)
- Timeline/feed renderer (virtualized lists).
- Compose/editor widget.
- Media picker/uploader.
- Notification grouping.
- Recipe attachment button (Moments/Video → Recipes) (optional).

### Capability gates (shared)
- `fediverse.social.post` (NexusSocial).
- `fediverse.moments.post` (NexusMoments).
- `matrix.message.send` (NexusChat).
- `fediverse.forum.post` (NexusForum).

---

## Non-goals (track-level)

- DMs in social apps (use NexusChat instead).
- Full moderation tooling / admin panels (v1.1+).
- Advanced media editing, full offline queueing, push notifications (can come later).

## Naming note: “Gallery” is the right name (avoid confusion without renaming everything)

We keep **Gallery** as the user-facing name for the local photo library, but we must avoid authority drift:

- **NexusMoments**: the Pixelfed/ActivityPub app (Fediverse posting + feed).  
- **Gallery (local)**: the device photo/capture library app (camera roll). This is tracked separately (see `TASK-0106`).

Launcher/UI may disambiguate by subtitle (e.g. “Moments — Fediverse” vs “Gallery — Local”) while keeping the
local library primary brand “Gallery”.

## Adjacent tracks (explicitly out of scope, but designed to integrate)

These are intentionally tracked elsewhere to keep Fediverse apps focused:

- **Online identity / cloud storage / backup upload**: `tasks/TRACK-NEXUSACCOUNT.md`
  - All Fediverse apps can consume it via account grants (optional).
- **Video (PeerTube) + creator economy**: `tasks/TRACK-NEXUSVIDEO.md`
  - Shares payment/entitlement primitives and "creator-first" UX patterns, but is tracked separately due to complexity.

## Determinism + test strategy (track contract)

- **Host-first**: protocol + domain + cache behavior has deterministic tests (no wallclock flakiness).
- **Bounds everywhere**: max response bytes, max JSON depth/fields, timeouts, redirect policy.
- **No fake success**: only emit `*: ready` / `SELFTEST:* ok` markers when behavior is real.

## Gates (RED / YELLOW / GREEN)

- **RED (blocking)**:
  - tokens stored outside `keystored`,
  - unbounded network reads / JSON parsing on untrusted responses,
  - secrets in logs.
- **YELLOW (risky / needs explicit design in TASK)**:
  - background refresh/sync (must be bounded + deterministic),
  - media uploads/download caching (budgets and content-type validation),
  - instance recommendation defaults (must be transparent and user-overridable).
- **GREEN (confirmed direction)**:
  - ActivityPub/Mastodon-compatible client,
  - policy-gated network + auditable actions,
  - cache-first timeline rendering.

## Phase map (what "done" means by phase)

### Phase 0 — Foundations (host-first, all apps)

Deliverables:

- Protocol clients: `nexus-activitypub` (Mastodon/Pixelfed/Lemmy) + `nexus-matrix` (Matrix).
- OAuth flow integration (host harness) + keystore-backed token storage abstraction.
- Cache layer (read-through) that supports offline rendering.
- Host test suite covering: parsing bounds, error mapping, pagination determinism.

### Phase 1 — MVP apps (prioritized)

**Priority 1: NexusSocial** (6 months)
- Timeline UI + compose + threads + notifications.
- Proof: Deterministic host tests + OS/QEMU markers (`nexussocial: ready`).

**Priority 2: NexusChat** (parallel with NexusSocial, 6 months)
- DMs + group chats + E2EE.
- Proof: Deterministic host tests + OS/QEMU markers (`nexuschat: ready`).

**Priority 3: NexusMoments** (+3 months after NexusSocial)
- Feed + grid + post + stories (optional).
- Proof: Deterministic host tests + OS/QEMU markers (`nexusgallery: ready`).

**Priority 4: NexusForum** (+3 months after NexusMoments)
- Communities + posts + comments + voting.
- Proof: Deterministic host tests + OS/QEMU markers (`nexusforum: ready`).

### Phase 2 — Polish + performance + resilience (all apps)

Deliverables:

- Better caching policy (staleness, invalidation, bounded storage).
- Rate limit handling, retry/backoff (bounded, deterministic).
- UI performance work (virtualized lists, image placeholders).
- Accessibility and i18n baseline.

### Phase 3 — v1.1+ expansions (carefully gated)

Candidate areas:

- Media upload improvements (filters, editing).
- Lists/filters and muted keywords (client-side).
- Background refresh (strict budgets; opt-in).
- Voice/video calls (NexusChat).
- Stories (NexusMoments).

## Candidate subtasks (to be extracted into real TASK-XXXX)

### Shared infrastructure
- **CAND-FEDIVERSE-000: nexus-activitypub client library (Mastodon/Pixelfed/Lemmy; strict bounds)**
- **CAND-FEDIVERSE-010: nexus-matrix client library (Matrix protocol; E2EE)**
- **CAND-FEDIVERSE-020: OAuth + keystored token storage (shared across all apps)**
- **CAND-FEDIVERSE-030: Cache + merge rules (offline-first rendering; shared)**
- **CAND-FEDIVERSE-040: Shared UI components (timeline/compose/media picker)**
- **CAND-FEDIVERSE-050: NexusAccount optional onboarding (Accounts UI + per-app grant flow)**

### NexusSocial (Mastodon)
- **CAND-NEXUSSOCIAL-100: Timeline UI v1 (virtualized list + stable rendering)**
- **CAND-NEXUSSOCIAL-110: Compose v1 (post/reply + CW + visibility)**
- **CAND-NEXUSSOCIAL-120: Thread view v1 (context + replies + collapse)**
- **CAND-NEXUSSOCIAL-130: Notifications v1 (grouping + actions)**
- **CAND-NEXUSSOCIAL-140: Search v0 (accounts/hashtags, bounded)**
- **CAND-NEXUSSOCIAL-150: OS wiring + deterministic markers + smoke/selftests**

### NexusMoments (Pixelfed)
- **CAND-NEXUSMOMENTS-100: Feed + grid view v1 (photo/video timeline + profile grid)**
- **CAND-NEXUSMOMENTS-110: Post v1 (upload + caption + hashtags)**
- **CAND-NEXUSMOMENTS-120: Stories v1 (24h ephemeral posts; optional)**
- **CAND-NEXUSMOMENTS-130: Likes + comments v1**
- **CAND-NEXUSMOMENTS-140: Explore v1 (hashtag discovery; bounded)**
- **CAND-NEXUSMOMENTS-150: OS wiring + deterministic markers + smoke/selftests**

### NexusChat (Matrix)
- **CAND-NEXUSCHAT-100: DMs v1 (1-on-1 E2EE chats)**
- **CAND-NEXUSCHAT-110: Group chats v1 (rooms + invites)**
- **CAND-NEXUSCHAT-120: Media sharing v1 (images/videos/files; bounded)**
- **CAND-NEXUSCHAT-130: Key management v1 (device verification + backup)**
- **CAND-NEXUSCHAT-140: Notifications v1 (new message alerts)**
- **CAND-NEXUSCHAT-150: OS wiring + deterministic markers + smoke/selftests**

### NexusForum (Lemmy)
- **CAND-NEXUSFORUM-100: Browse communities v1 (discover + subscribe)**
- **CAND-NEXUSFORUM-110: Feed v1 (home + all; sorted)**
- **CAND-NEXUSFORUM-120: Post v1 (text/link/image posts)**
- **CAND-NEXUSFORUM-130: Comments v1 (threaded + voting)**
- **CAND-NEXUSFORUM-140: Moderation v1 (report; basic)**
- **CAND-NEXUSFORUM-150: OS wiring + deterministic markers + smoke/selftests**

## Extraction rules (how candidates become real tasks)

A candidate becomes a real `TASK-XXXX` only when it:

- declares **Touched paths (allowlist)** and stays within them,
- states bounds (bytes/timeouts/depth) and determinism constraints explicitly,
- provides host-first proof requirements (tests/goldens),
- documents what is stubbed vs real (no fake success),
- and routes policy through `policyd` (no duplicated allow/deny logic).

## Security checklist (for security-relevant Fediverse app work)

When touching auth/network/account storage/E2EE:

- [ ] Threat model + invariants included in the task
- [ ] `test_reject_*` negative tests for bad inputs (oversize JSON, invalid redirects, bad tokens)
- [ ] No `unwrap`/`expect` on untrusted input (network responses, URLs)
- [ ] No secrets in logs/errors (tokens, auth codes, headers, encryption keys)
- [ ] Policy decisions are auditable and deny-by-default (`policyd`)
- [ ] E2EE keys (Matrix) stored in `keystored` with proper lifecycle management

## DON'T DO (Hard Failures)

### Legal / product

- DON'T add inofficial proprietary bridges/scrapers “just for users”.
- DON'T use confusing branding (“official”, third-party logos, trade dress clones).

### Security / architecture

- DON'T store refresh/access tokens outside `keystored`.
- DON'T allow unbounded network reads, redirects, or JSON parse depth/size.
- DON'T implement policy locally in the app when `policyd` should decide.
- DON'T claim readiness markers without real behavior and proof.
