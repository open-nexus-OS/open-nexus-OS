# ADR-0047: nexus-inet — minimal interaction-net evaluator backend

## Status

Accepted (2026-07-17). Implemented and boot-proven (SMP=1 + SMP=2 marker
gates).

## Context

The compute broker's promise (ADR-0045) is an exchangeable evaluator behind
a stable declarative interface. To make that promise real — and to lay the
foundation for tree-shaped symbolic workloads (compiler passes, type
checking) that do not fit the flat partition→map shape — we built a minimal
interaction-combinator net evaluator. Naming rule: the implementation is
described generically (Lafont-style interaction combinators); third-party
product names do not appear in code or docs.

## Decision

`source/libs/nexus-inet/` (no_std, `forbid(unsafe_code)`):

- **Calculus**: agents ERA/CON/DUP (classic combinators) plus a number leaf
  (NUM) and a binary add pair (ADD2 → ADD1 via in-place node repurposing, so
  arithmetic tree reduction allocates nothing). Interactions are LOCAL: a
  rule touches the two redex nodes and the peers of their ports only.
- **Store**: a bounded, RECYCLING arena — a spin-locked free list (bump-only
  would exhaust: reduction frees as much as it allocates). Exhaustion is an
  explicit `ArenaExhausted` reject, never OOM. Ports are packed `AtomicU64`s:
  workers share `&Arena` in safe Rust; an interaction only writes ports whose
  previous peer it consumed, so writes are disjoint by protocol and atomics
  make the sharing race-free at the language level.
- **Reduction is round-based**: each round's redex list is partitioned with
  the workpool's deterministic chunk math; `reduce_chunk` is the ONE
  worker-agnostic kernel (single-thread driver, host equality-matrix threads
  and pinched's workpool workers all run the same function). New redexes are
  merged through a locked deque (TASK-0277: no lock-free port-linking in v1
  — that is a later, separately proven step).
- **Determinism by construction**: the calculus is confluent, so the normal
  form is independent of partitioning and merge order — `workers = 1 ≡ N`
  for both the result value AND the total interaction count (host-proven).
- Every rule's `link()` result must be pushed as a potential new redex: the
  cascade link (a folded result meeting its waiting parent) IS an active
  pair — dropping it strands the net in a false normal form (found and fixed
  during bring-up; the depth-2 trace test guards it).

## Consequences

- pinched serves `JOB_INET_TREE_SUM` on this backend; markers
  `SELFTEST: inet determinism / bounded / parallel exec ok` gate both SMP
  profiles.
- v1 clients declare WORKLOADS (e.g. tree depth), not raw nets — the generic
  net-serialization wire format is a follow-up with its own validation
  budget (arbitrary nets from a wire are an attack surface).
- Round barriers cost one fence round-trip per round; fine for batch trees,
  wrong for fine-grained nets — an always-on worker loop with a shared redex
  deque is the later optimization, gated on its own proofs.

## How to use and extend

- **Use**: `Arena::new(cap)` → build a net (`build_tree_sum` or manual
  `alloc`/`link`) → `reduce_to_normal_form` (single-thread) or round-based
  `reduce_chunk` across workers → read results (`root_value`).
- **New agent/rule**: add the `NodeKind`, its interaction arms in
  `interact()` (normalize by kind order; push EVERY `link()` redex), keep
  every failure an explicit `InetError`, and extend the host tests: value
  pins, node-balance (recycling) and the equality matrix must stay green.
- **First real customer**: tree-shaped compiler/type-checker passes.
  GPU/distributed evaluation stays future scope behind the same broker wire.

## References

- ADR-0045 (broker), ADR-0046 (workpool)
- `source/libs/nexus-inet/src/{arena,reduce}.rs` and its host tests
