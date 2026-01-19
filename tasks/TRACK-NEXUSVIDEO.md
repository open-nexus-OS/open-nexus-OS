---
title: TRACK NexusVideo (PeerTube): federated video + live + creator-first monetization + danmaku plugins (EU-friendly)
status: Draft
owner: @media @ui @platform
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Media Apps track (players + TV hub direction): tasks/TRACK-MEDIA-APPS.md
  - NexusNet SDK (network/account/grants): tasks/TRACK-NEXUSNET-SDK.md
  - Recipes app (recipe attachment import target): tasks/TRACK-RECIPES-APP.md
  - Store v2.2 (license tokens/ledger/parental primitives): tasks/TASK-0221-store-v2_2a-host-licensed-ledger-parental-payments.md
  - TRACK NexusAccount (online account provider): tasks/TRACK-NEXUSACCOUNT.md
---

## Goal (track-level)

Deliver **NexusVideo**, a first-party federated video experience built on **PeerTube** that:

- feels like a modern mainstream video app (fast feed, subscriptions, watch page, comments),
- remains open and federated (no proprietary scraping, no closed network lock-in),
- enables sustainable creator monetization (memberships/sponsorships) with transparent revenue split,
- supports community features (live streams + chat; optional “danmaku” overlay),
- and stays legally safe: own branding and UI identity (inspired by good UX patterns, not copied trade dress).

## Scope boundaries (anti-drift)

- NexusVideo is a **client** for PeerTube federation (and related open surfaces), not a replacement for PeerTube servers.
- Monetization is an additive layer; it must not require proprietary platform integration.
- Any payments/payouts work must be explicitly phased; avoid accidentally becoming a financial institution.

## Legal + brand guardrails (non-negotiable)

- No confusing branding (no third-party logos, names, or “official” claims).
- UI inspiration is allowed; exact trade dress copying is not.
- “Danmaku” is treated as a generic timed-comment overlay feature, not a brand.

## Architecture stance (OS-aligned)

- Account and payments must follow the NexusNet + NexusAccount model (per-app grants; tokens are secrets; auditable).
- Policy decisions (network, payments, mature content controls) are mediated by `policyd` and parental controls where relevant.
- Media playback and system surfaces should align with Media UX tracks (media sessions, mini-player, casting direction).

## Recipe attachment (optional extension; creator-friendly)

NexusVideo can support “recipe attached to video” posts:

- the video description contains a bounded, portable “recipe block” fallback (text),
- NexusVideo client renders an “Open recipe” button and dispatches to the Recipes app via Share v2 / Intents,
- the Recipes app imports into the user cookbook (`tasks/TRACK-RECIPES-APP.md`).

This stays federation-safe (no proprietary bridges) and keeps users in control.

## Creator economy: product model (win-win)

### Revenue streams (v1 direction)

- **Creator memberships**: monthly subscription tiers; perks can include badges, higher chat priority, exclusive posts.
- **One-time sponsorship**: “support this creator” with small amounts.
- **Enterprise channels (paid)**: paid “verified/managed” channels for companies (SLA, branding options, analytics, moderation tooling).

### Split principle

- Goal: creator receives materially more than mainstream platforms.
- Platform keeps a small, transparent fee to fund infra, moderation, and payment processing.

### Compliance stance (phased)

- Phase 1: use a payment processor that handles KYC/AML and payouts (reduces your operational burden).
- Phase 2: introduce platform ledger + tokenized entitlements (NLT-like) for memberships, still keeping payout compliance external.
- Phase 3: only if justified, bring more payout logic in-house (requires strong legal/compliance posture).

## Danmaku + live chat: plugin architecture stance

Treat overlays as a **plugin system** so the core player remains stable:

- **Overlay inputs**:
  - live chat stream,
  - timed comments track (video-relative timestamps),
  - moderator events (pin, highlight, hide).
- **Overlay renderer**:
  - bounded event rate (anti-spam),
  - deterministic layout rules for tests (seeded placement, stable ordering),
  - accessibility mode (overlay off by default or simplified).

## Phase map

### Phase 0 — PeerTube client MVP (read-only + basic account)

- Browse + search + subscriptions
- Watch page + comments (bounded)
- Playback integration with system media session surfaces where available

### Phase 1 — Live + chat (bounded)

- Live stream playback
- Chat stream with moderation basics (mute/slow mode)
- Optional overlay mode (danmaku) behind a feature toggle

### Phase 2 — Creator monetization MVP

- Memberships + sponsorship (processor-backed)
- Basic entitlements (membership badge, access control for exclusive posts/streams)
- Transparent receipts and audit logs (no secrets)

### Phase 3 — Enterprise channels

- Paid channel plan (verification, SLA, analytics dashboards)
- Org billing and admin roles (policy-gated)

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-NEXUSVIDEO-000: PeerTube client v0 (browse/watch/comments; strict bounds)**
- **CAND-NEXUSVIDEO-010: Live + chat v0 (bounded streams; moderation basics)**
- **CAND-NEXUSVIDEO-020: Danmaku overlay plugin v0 (feature-gated; deterministic layout tests)**
- **CAND-NEXUSVIDEO-030: Creator memberships v0 (processor-backed; entitlement tokens)**
- **CAND-NEXUSVIDEO-040: Sponsorship v0 (one-time support; abuse controls)**
- **CAND-NEXUSVIDEO-050: Enterprise channels v0 (paid plans; org admin; policy/audit)**
- **CAND-NEXUSVIDEO-060: Recipe attachment v0 (portable recipe block + “Open recipe” intent to Recipes)**

## Security + abuse checklist (video/monetization)

- [ ] Strict bounds on network payloads, chat message sizes, event rates
- [ ] No secrets in logs (payment tokens, auth headers)
- [ ] Fraud/abuse controls: rate limits, anti-spam, reporting flows
- [ ] Content controls: maturity flags and parental gating integration
- [ ] Clear separation: payments processor responsibilities vs platform ledger/audit

## DON'T DO (Hard Failures)

- DON'T ship proprietary scraping/bridges.
- DON'T claim strong payout/compliance guarantees without real KYC/AML posture.
- DON'T enable unbounded live chat/danmaku event streams (must be budgeted and testable).
