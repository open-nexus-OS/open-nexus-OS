---
schema: 1
title: "policyd and the policy tree"
role: guide
subsystem: security
sources: [source/services/policyd, userspace/policy, policies, tools/nx/src/commands/policy.rs, source/init/nexus-init/src/bootstrap/policyd.rs]
contracts: [ADR-0014, RFC-0015, RFC-0045, RFC-0091, ADR-0084, RFC-0111]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# policyd and the policy tree

> **Purpose** — who decides whether a subject may use a capability, where that decision's inputs live, and how to author and check them.
> **Owns** — `source/services/policyd/`, `userspace/policy/`, `policies/`, `tools/nx/src/commands/policy.rs`
> **Contracts** — [ADR-0014](../adr/0014-policy-architecture.md) (policyd decides, deny-by-default), [RFC-0015](../rfcs/RFC-0015-policy-authority-audit-baseline-v1.md) (authority + audit baseline), [RFC-0045](../rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md) (policy as code, `nx policy`), [RFC-0091](../rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md) (argument profiles), [ADR-0084](../adr/0084-one-permission-authority-policyd-holds-every-app-grant-published-by-the-bundle-manager-asked-by-the-launchers.md) (policyd holds every app grant)
> **Invariants** — identity is the kernel's `sender_service_id`, never a name in the payload. No allow rule ⇒ deny. `policies/` is the only policy source; `recipes/policy/` is a legacy note.
> **Read next** — [capabilities.md](capabilities.md), [abi-filters.md](abi-filters.md), [network-policy.md](network-policy.md), [signing.md](signing.md)

## What policyd decides

| Question | Wire op (`source/libs/nexus-wire/src/policyd.rs`) | Asked by |
|---|---|---|
| Does the sender hold capability `c`? | `OP_CHECK_CAP` (4) | any service, about itself |
| Does subject `s` hold `c`? | `OP_CHECK_CAP_DELEGATED` (5) | enforcement points holding `policy.delegate` (keystored, statefsd, updated, bootctld, netstackd, ingressd, …) |
| May `a` route to `b`? | `OP_ROUTE` (2) — `ipc.core`; target `execd` needs `route.execd` | init, on every name-route request |
| May `a` spawn? | `OP_EXEC` (3) — `proc.spawn` | init, forwarding execd's exec check |
| RFC-0091 argument profile / evaluation / mode | `OP_ABI_PROFILE_GET` (6), `OP_SET_ABI_MODE` (7), `OP_ABI_EVAL` (8), `OP_ABI_LEARN_STATS` (9) | seams — see [abi-filters.md](abi-filters.md) |
| An installed app's grants | `OP_APP_GRANTS_PUT` (10), `OP_APP_GRANTS_GET` (11) | bundlemgrd publishes; abilitymgr, execd read |

Rules every handler follows:

- **Identity binding.** A payload may name a requester, but outside init's privileged proxy channels the name must hash (`service_id_from_name`) to the kernel sender id, or the answer is `DENY` with an identity-mismatch audit.
- **Deny-by-default.** A subject with no `[allow]` entry has no capabilities.
- **Delegation is a capability.** Only a sender holding `policy.delegate` may ask about another subject.
- **Route checks fail closed.** If policyd does not answer, init denies the route and names the outage. The exec check in `source/init/nexus-init/src/bootstrap/responder.rs` still admits when policyd gives no answer (`unwrap_or(true)`).

## Two runtimes, one tree

| | OS (`os-lite`, `source/services/policyd/src/os_lite.rs`) | Host (`std_server.rs`) |
|---|---|---|
| Policy table | compiled at build time by `source/services/policyd/build.rs` from `policies/nexus.policy.toml` + includes | loaded through `userspace/policy` (`PolicyTree`) |
| Reload | none — static per boot | Config v1: configd's effective snapshot carries `policy.root`; `PolicyConfigConsumer` prepares the candidate tree and the 2PC commit applies it; an invalid candidate never replaces the active version |
| App grants | runtime table (32 apps × 16 grants), replaced per app on each publish | same codec, host tests |
| Modes | RFC-0091 Enforce/Learn per subject (`OP_SET_ABI_MODE`) | `Version`, `Eval`, `ModeGet`, `ModeSet` frames through `PolicyAuthority` |

The build script and the host crate parse the same grammar: `userspace/policy/src/schema.rs` is included by path into policyd's build, so the OS table cannot accept a rule the tooling rejects.

## App grants (ADR-0084)

policyd is the one permission authority for apps. bundlemgrd reads each app's `caps` and `background` from the verified volume's `meta/app.properties` sidecar and publishes them once per boot (`source/services/bundlemgrd/src/grants_publish.rs`, needs `app.grants.publish`). abilitymgr asks at every launch, execd at every spawn (`app.grants.read`); no answer means no launch and no routes. A publish naming an unknown permission rejects the whole app (`policyd: app grants reject (app= cap=)`). A non-empty `background = [...]` becomes `nexus.permission.BACKGROUND`; `restorable = true` becomes `nexus.permission.STATE`.

## The policy tree (`policies/`)

| File | Role |
|---|---|
| `policies/nexus.policy.toml` | root: `version` + `include = [...]` |
| `policies/base.toml` | `[allow]` per service, `[abi_profile."<svc>"]`, `[[expose."<svc>"]]` |
| `policies/manifest.json` | deterministic tree hash; required evidence for `nx policy validate` |
| `policies/tests/` | `ok_*` / `reject_*` grammar fixtures |
| `policies/*-trust.toml` | signing trust anchors — not policy, see [signing.md](signing.md) |

Service and capability names are trimmed and lowercased before evaluation. Includes must stay inside `policies/` (no `..`, no absolute path). Equivalent inputs produce the same `PolicyVersion`; derived JSON views are not a second authority.

## `nx policy`

| Command | Does |
|---|---|
| `nx policy validate [--root <dir>] [--write-manifest]` | load + validate the tree; fails on a missing or stale `manifest.json`. `--write-manifest` rewrites it first (the step after every policy edit; commit the result) |
| `nx policy diff --from <dir> --to <dir>` | compares the `PolicyVersion` of two trees (`changed`, both versions) — not a rule-level diff |
| `nx policy explain --subject <svc> --cap <c>… [--mode enforce\|dry-run\|learn]` | the decision plus a bounded explain trace |
| `nx policy mode --set <mode> --observed-version <v> --actor-service-id <id> [--authorized]` | host-side precondition check only; it does not change a running daemon |
| `nx policy learn-gen <log> --subject <svc> --out <toml> [--allow-any]` | turn `abi.learn` records into a review-first `[abi_profile]` skeleton ([abi-filters.md](abi-filters.md)) |

All take `--json`. Dry-run and learn report would-deny decisions; they never grant what enforce denies.

## Audit

Every decision becomes one record on logd scope `policyd.audit` (`source/services/policyd/src/audit_os.rs`):

```text
audit v1 op=<check|route|exec|…> decision=<allow|deny> subject=0x<sid> [target=0x<id>] reason=<r>
```

`reason` comes from the one deny taxonomy `nexus_ipc::audit::DenyReason` (`userspace/nexus-ipc/src/audit.rs`): `policy`, `abi-rule:statefs`, `ingress-denied`, `egress-denied`, `abi-mode`, `quota-exceeded`. Emission is bounded per boot and best-effort; it never blocks a decision. Records carry no key material.

## Proof

- Host: `cargo test -p policy`, `cargo test -p policyd`, `cargo test -p configd`, `cargo test -p nx --test policy_cli`.
- QEMU (proof-manifest phase `policy`): `SELFTEST: policy deny audit ok`, `SELFTEST: policy allow audit ok`, `SELFTEST: keystored sign denied ok`; app grants: `SELFTEST: app grants ok (app=…)`.

## Do not

- Add a second allowlist or a policy table compiled into another service — ask policyd.
- Restore `recipes/policy/` as a live root, or add a policy file watcher; host reload goes through Config v1.
- Treat `nx policy mode` as a live mode switch.
- Invent a new on-disk policy format without an RFC.
