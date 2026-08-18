# Kernel Hardening Objectives Status

This checklist captures the state of the hardening objectives from the October 15 brief. It is meant to provide a quick answer to whether the current tree satisfies every requested item.

| Objective | Scope | Status | Notes |
| --- | --- | --- | --- |
| 1. Kernel mapping | Final-image linker symbols, GLOBAL flags, RX guard | ✅ Completed | `map_kernel_segments` uses `__text_start/__text_end` (RX) and `__bss_end` plus stack symbols for RW mappings and emits the required marker. The RX guard reads bytes before any SATP switch. |
| 2. SATP switch island | Dedicated same-page trampoline and post-switch marker | ✅ Completed | All SATP activations route through the switch island, which performs the RX sanity probe, swaps stacks within the identity page, fences, and emits `AS: post-satp OK`. |
| 3. Syscall discipline | Typed decoders, canonical VAs, W^X denial | ✅ Completed | `types.rs` defines `VirtAddr`, `PageLen`, `AsHandle`, and `SlotIndex`; syscall decoders enforce alignment, canonicality, and W⊕X. |
| 4. Spawn in child AS | Fresh Sv39 AS with guarded stack or caller-provided AS | ✅ Completed | `TaskTable::spawn` allocates a guarded stack for new address spaces, validates entry PCs, and respects custom handles. |
| 5. Self-tests & markers | Ordered acceptance markers | ✅ Completed | Selftests emit the requested `KSELFTEST` markers and rely on feature-gated verbosity. |
| 6. Debug/dev hardening | Guard pages, lockdep-light, heap redzones, PT verifier, trap ring buffer | ✅ Completed | Kernel and selftest stacks gain unmapped guards, the SATP island reuses the guarded kernel stack, debug builds expose a trap ring buffer (`trap_ring`) and optional `trap_symbols`, and CI triage aborts on PANIC/EXC/ILLEGAL/RX markers. |
| 7. Verifiable-style refactors | Newtypes, pure helpers, structured logging | ✅ Completed | Typed IDs/lengths gate syscall inputs, helper routines stay pure, and the kernel now routes output through leveled log macros so debug chatter is suppressed in release builds. |

**Bottom line:** The hardening objectives are satisfied for mapping/guards/W^X/typed decoding.

**Current state note (2025-12-18):** syscall handlers return expected errors as `-errno` in `a0`.
The kernel may still terminate tasks in true "no forward progress" situations (e.g. repeated
ECALL storms), but ordinary syscall errors are returned to userspace.

## Userspace Policy Hardening (TASK-0008, 2026-01-25)

| Objective | Status | Notes |
| --- | --- | --- |
| Policy engine (`userspace/policy` + `policyd`) | ✅ Complete host-first | Policy as Code v1 tree, deterministic `PolicyVersion`, bounded evaluator, deny-by-default |
| Audit trail | ✅ Complete | All allow/deny decisions logged via logd |
| Channel-bound identity | ✅ Complete | Policy binds to `sender_service_id`, not payload strings |
| Policy-gated operations | ✅ Complete | keystored `OP_SIGN` requires `crypto.sign` capability |
| Single authority | ✅ Complete | `policyd` is sole decision service; live policy root is `policies/nexus.policy.toml` |

**QEMU proofs:** `RUN_PHASE=policy RUN_TIMEOUT=190s just test-os` (green)

**RFC:** `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`

## Crashdump v1 hardening snapshot (TASK-0018, 2026-03-26)

| Objective | Status | Notes |
| --- | --- | --- |
| Deterministic bounded dump format + path scope | ✅ In Review | `userspace/crash` enforces size/path bounds with `test_reject_*` coverage. |
| Marker honesty (`execd: minidump written`, `SELFTEST: minidump ok`) | ✅ In Review | Success markers emit only after verified artifact/report path. |
| Fail-closed metadata publish | ✅ In Review | `execd` rejects forged/no-artifact/mismatched metadata paths; unauthenticated publish remains denied. |
| Policy-bound child write path | ✅ In Review | `statefsd` uses narrow policy-bound canonicalization helper (no broad SID-0 bypass). |

**Proofs:** `cargo test -p crash`, `cargo test -p execd`, `cargo test -p statefsd`, `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

**RFC:** `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md`

## StateFS hardening snapshot (TASK-0025/0026/0027, 2026-08-18)

| Objective | Status | Notes |
| --- | --- | --- |
| Value authenticity + anti-rollback | ✅ Done | `NXEV` envelopes (HMAC-SHA256, key via label-scoped HKDF over a device-key signature) verified on put AND get; replay-fed seq high-water mark. Wire statuses 9/10. Never a signing key used raw as a MAC key. |
| Multi-key atomicity | ✅ Done | Journal v2 `PREPARE/PAYLOAD/COMMIT/ABORT`, committed-only replay — both-or-neither across restart at every write boundary (crash-injection suite cuts at every block write). |
| Bounded replay / boot availability | ✅ Done | Compaction (`CHECKPOINT` snapshot → inactive A/B region → atomic `NXS2` superblock flip) makes reopen cost proportional to live state, defusing the `MAX_REPLAY_RECORDS` hard open-failure of `/state`. |
| Offline repair | ✅ Done | `tools/fsck-statefs`: stable exit codes (0/1/2), append-only repair (`TXN_ABORT` per orphan), never rewrites committed data; decrypt-aware with an explicit key. |
| Encryption at rest (opt-in) | ✅ Done | XChaCha20-Poly1305 for enrolled non-boot-critical prefixes; deterministic nonces (no `getrandom` in the OS graph), AAD binds header + key path; boot-critical prefixes structurally unenrollable; enablement behind the `statefs.admin` capability. |
| Marker honesty | ✅ Done | `compaction done` requires a device readback verify; `encryption on` requires an in-process AEAD seal/open/tamper self-check; both coupled to their selftests by fake-green guards in `scripts/qemu-test.sh`. |
| Cold-boot durability | ✅ Done | Proven against a PRESERVED image (`NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1` second boot), not soft-reboot replay. |

**Known limits (documented, not defended):** no sealed storage on this platform — an attacker
with the device seed derives all keys; keys/paths stay plaintext; at-rest tampering of the
enablement meta record is inside the at-rest attacker's power; no key rotation yet.

**Proofs:** `cargo test -p statefs -p statefsd -p fsck-statefs`, plus the QEMU double boot
(`just test-os`, then `NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1 just test-os`).

**Contract:** `docs/storage/statefs.md` · **ADRs:** 0023 (persistence), 0043 (statefs stays
service KV), 0044 (block layout + keep-blk) · **RFC:** `RFC-0018` (v1 framing, frozen).

## Related canonical references

- Kernel overview: `docs/architecture/01-neuron-kernel.md`
- Kernel + layering quick reference: `docs/architecture/README.md`
- Policy flow: `docs/architecture/11-policyd-and-policy-flow.md`
- Testing methodology and CI marker discipline: `docs/testing/README.md` and `scripts/qemu-test.sh`
