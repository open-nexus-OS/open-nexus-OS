---
schema: 1
subsystems:
  - slug: kernel
    name: "Kernel core and runtime"
    tier: production-grade
    release: "consumer + kiosk/IoT"
    focus: "scheduling, IPC, memory, QoS and hardening authority"
  - slug: distributed
    name: "DSoftBus and distributed"
    tier: production-floor
    release: "consumer + distributed beta"
    focus: "distributed session, transport, mux and remote services"
  - slug: networking
    name: "Networking and transport"
    tier: production-floor
    release: "consumer + kiosk/IoT"
    focus: "netstack, drivers, ingress bounds, real-connect proofs"
  - slug: observability
    name: "Observability, crash, perf and diagnostics"
    tier: production-floor
    release: "consumer + hardware beta"
    focus: "logs, traces, crash evidence, perf budgets, soak and flake gates"
  - slug: sessions
    name: "Accounts, ability and sessions"
    tier: production-floor
    release: consumer
    focus: "session lifecycle, greeter and home isolation, ability kill semantics"
  - slug: security
    name: "Security, policy and identity"
    tier: production-grade
    release: "consumer + kiosk/IoT"
    focus: "policy authority, identity, sandboxing, ABI guardrails"
  - slug: storage
    name: "Storage, PackageFS and content"
    tier: production-grade
    release: "consumer + kiosk/IoT"
    focus: "persistent state, VFS and content contracts, quotas, zero-copy content paths"
  - slug: updates
    name: "Updates, packaging and recovery"
    tier: production-grade
    release: "consumer + kiosk/IoT"
    focus: "verified boot, anti-rollback, measured boot, trusted recovery and updates"
  - slug: hardware
    name: "Bring-up, hardware and drivers"
    tier: beta-floor
    release: "hardware bring-up beta"
    focus: "board support, device-class services, driver contract proofs"
  - slug: ui
    name: "Windowing, UI and graphics"
    tier: production-floor
    release: consumer
    focus: "compositor, GPU path, input to photon, frame budgets, the glass material"
  - slug: text
    name: "Text, IME, i18n and accessibility"
    tier: beta-floor
    release: "consumer beta"
    focus: "text shaping, input methods, locale, accessibility spine"
  - slug: media
    name: "Media and creative"
    tier: beta-floor
    release: "consumer beta"
    focus: "audio and media sessions, bounded decode and capture"
  - slug: messaging
    name: "Messaging, search, store and sharing"
    tier: beta-floor
    release: "consumer beta"
    focus: "search, share, notifications, store without authority drift"
  - slug: app-platform
    name: "DSL, app platform and SDK"
    tier: beta-floor
    release: "ecosystem beta"
    focus: "DSL and runtime contracts, app platform, SDK, the std target"
  - slug: devx
    name: "DevX, config and tooling"
    tier: production-floor
    release: "internal + hardware beta"
    focus: "one CLI, schema and config discipline, deterministic harness, planning tooling"
---

# Subsystems

The one list a task's or an RFC's `subsystem` refers to (RFC-0115). A subsystem is a reference
class for estimates and a gate tier for release claims. Membership is the `subsystem` key of each
document; counts and progress are derived (`nx ledger list --subsystem <slug>`), never written here.

## Gate tiers

- `production-grade` — release-blocking: real enforcement, reject-path proofs, bounded failure
  behaviour, recovery and security closure; consumer and kiosk/IoT claims would be dishonest
  without it.
- `production-floor` — real, coherent and test-backed; may still grow in breadth or speed, but
  behaves predictably under normal and bounded-stress load and never relies on fake-success
  markers.
- `beta-floor` — real behaviour with deterministic proofs and explicit limits; good enough for
  bring-up or subsystem beta work, not yet for release claims.

The closure contract per tier: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.
