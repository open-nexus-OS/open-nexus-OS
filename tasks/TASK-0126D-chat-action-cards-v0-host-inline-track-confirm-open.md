---
title: TASK-0126D Chat Action Cards v0 (host-first): inline cards for track/confirm/open + provider registry + snapshot persistence + deterministic tests
status: Draft
owner: @ui @runtime
created: 2026-01-28
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - System Delegation track: tasks/TRACK-SYSTEM-DELEGATION.md
  - Action-based intents baseline: tasks/TASK-0126B-system-delegation-v1a-intent-actions-defaults-policy-host.md
  - Chooser/defaults pattern: tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - Policy v1 (intents caps, audit): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Scoped grants (content://): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
---

## Context

We want “WeChat-like convenience” without a super-app:
users should be able to complete common flows **inside the chat** (inline), without constantly opening other apps.

At the same time:
- we must avoid a mini-app runtime (no embedded arbitrary UI/code),
- keep strict policy boundaries (no ambient authority),
- and keep behavior deterministic and testable.

This task defines **Chat Action Cards v0**: a small, bounded “inline card” surface rendered by the chat app,
fed by a provider model and routed via system delegation.

## Goal

Deliver (host-first):

1. v0 action catalog (minimal, low-risk first):
   - `track.inline` — show tracking status inline (snapshot + manual refresh)
   - `confirm.inline` — “hold to confirm” / bounded confirm UI, produces a receipt
   - `open.inline` — inline preview/summary for `content://` items where possible, otherwise safe “Open full”
2. Provider registry model (no UI code from providers):
   - providers register supported actions + constraints (bounds, offline/online capability)
   - providers produce **data/state** for cards; the chat app renders a canonical UI
3. Snapshot persistence model (chosen UX):
   - cards persist as **snapshots** in the chat log (stable content, deterministic)
   - refresh is explicit (user taps “Refresh”), not automatic polling
4. Deterministic routing semantics:
   - action targets are selected deterministically (default + MRU + stable tie-breakers)
   - no “free URL” actions; no arbitrary http(s) navigation via chat cards
5. Host-first proofs and rejection tests.

## Action naming + fallback matrix (v0 canonical)

We intentionally keep the action catalog small and define one “golden path” UX per action:

- `track.inline`: stay in chat (inline card)
- `confirm.inline`: stay in chat (inline card)
- `open.inline`: stay in chat *when safe* (inline preview), otherwise fall back to “Open full”

### Fallback rules (to avoid “opens new app” fatigue)

The default should be: **stay in chat**. We only leave the chat surface when required by safety or scope.

| Action | Preferred UX | When inline is allowed | Fallback (“Open full”) |
| ------ | ------------ | ---------------------- | ---------------------- |
| `track.inline` | Inline card | Always (snapshot). Refresh is explicit. | Optional: “Details” can open a dedicated tracking app later (non-default path). |
| `confirm.inline` | Inline card | Always (hold/slide). | Never needed in v0; only future “advanced confirm” may open a details page. |
| `open.inline` | Inline preview | Only for `content://` URIs with bounded preview/rendering. | If not `content://` or preview is unsupported/denied → show a safe “Open full” button that delegates to the default handler. |

Notes:
- `open.inline` must not become a generic URL launcher; do not accept arbitrary `http(s)` in v0.
- The fallback must be deterministic (stable errors and stable “why” strings) so tests and UX remain predictable.

## Key design rules (hard requirements)

- **No embedded UI/runtime**: providers never supply HTML/JS/UI trees; only bounded typed data.
- **Unspoofable origin**: card must display channel-bound origin (no “app name” strings trusted from payload).
- **No confirm-hölle**: low-risk actions must not require modal confirmation each time.
  - Use “manual refresh” + “hold to confirm” patterns instead of dialogs where possible.
- **No free URLs**: `open.inline` is limited to safe schemes; anything risky must go through the browser surface with policy.
- **Deterministic + bounded**: strict size caps for card payloads and provider outputs.

## v0 UX contract (chosen)

### Tracking (track.inline)

- Default: show a compact status card inline:
  - carrier label (from provider / validated)
  - status line + last update timestamp (displayed deterministically)
  - up to N recent events (bounded)
- “Refresh” button triggers an explicit provider refresh call (policy-gated if network is used in future).

### Confirm (confirm.inline)

- Inline “Hold to confirm” (or “Slide to confirm”) control.
- On confirm:
  - generate a bounded **receipt** object (stable text + stable ID)
  - optionally post a confirmation message/event back into the conversation (future wiring)

### Open (open.inline)

- If payload is `content://`:
  - render a small preview/summary inline (bounded)
  - provide “Open full” which delegates to the appropriate default handler
- If not `content://`:
  - do not preview; show safe “Open full” only (and only for allowlisted schemes)

## Constraints / invariants

- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Deterministic errors and stable ordering.
- Strict budgets:
  - max card payload bytes
  - max provider response bytes
  - max events per tracking card
  - max confirm text/receipt bytes

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add `tests/chat_action_cards_v0_host/`:

- deterministic rendering model:
  - given the same inputs, the card model output is stable (hash/golden)
- routing determinism:
  - default + MRU ordering produces stable target selection
- `test_reject_*` cases:
  - reject oversize provider response
  - reject invalid/unknown action
  - reject non-`content://` preview attempts
  - reject free-url attempts (http/https) deterministically
- snapshot persistence semantics:
  - snapshot stored; refresh updates snapshot deterministically when provider response changes (fixture-based)

## Touched paths (allowlist)

- `tests/chat_action_cards_v0_host/` (new)
- (future wiring task will touch chat app + intents/service layers; not part of v0 host contract)

## Follow-ups

- OS wiring: register first providers and render cards in NexusChat UI.
- Networked tracking: only after NexusNet/policy gates exist; keep “Refresh” explicit and auditable.
- Payment cards: later, via wallet/provider authority; must be user-mediated and anti-phishing hardened.
