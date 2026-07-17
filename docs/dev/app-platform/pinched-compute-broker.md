# pinched — the system-internal compute broker

`source/services/pinched/` is the system's parallel batch-compute broker: it
executes declarative partition→map jobs on the shared `nexus-workpool`
(fence-coordinated compute threads, deterministic chunking, `workers=1 ≡ N`
equality contract). The wire protocol is backend-agnostic so the execution
backend can be swapped without touching callers (v1 = native workpool; a
minimal interaction-net evaluator, `nexus-inet`, is the planned v2 backend).

## Visibility doctrine (binding)

**pinched is invisible to app developers.** There is no DSL surface, no
`dsl_services.capnp` entry, no app-facing permission, and no entry in
`nexus-sdk-routes`. Only system services and SDK internals may talk to it, and
they present the result as an ordinary synchronous function — a developer must
never need to understand (or even see) how parallelism happens.

## Latency doctrine (binding)

pinched may ONLY be used on paths where **latency is uncritical and the user
is already waiting** — batch work such as asset bakes, compiles, bulk
transforms. Rules:

- **Frame hotpaths are forbidden.** Nothing on a per-frame render, input, or
  compositor path may submit to pinched. Submitting a job costs an IPC
  round-trip plus VMO staging; that is noise for a batch job and poison for a
  frame budget.
- **Adopt only with a measured win.** A call site moves to pinched when the
  parallel result-proof shows a real improvement for that workload; "it could
  be parallel" is not a reason.
- **Fail open on LOCAL compute, never on waiting.** Every caller keeps its
  inline fallback (the broker itself falls back inline and reports
  `workers = 0` honestly). A broker outage may make a bake slower — never
  wedge it.

## Wire protocol v1

See `source/services/pinched/src/lib.rs` (`protocol` module — the SSOT).
Summary: an `OP_COMPUTE` frame names the job kind and partition domain; the
job data VMO travels via CAP_MOVE (the RFC-0072 splice pattern — no IPC frame
copies). The service computes in place and completes by writing the VMO's
16-byte header LAST (release fence; `DONE_MAGIC`). The header reports status,
element count, and `workers` — the honest dispatch counter (0 = inline
fallback). Oversized or malformed jobs are rejected through the header, never
queued or truncated.

Job kinds:

| kind | payload | partition domain |
| ---- | ------- | ---------------- |
| `JOB_MAP_MIX_U32` (1) | u32 elements, pure per-element transform | elements |
| `JOB_SVG_RASTER` (2) | UTF-8 SVG in, BGRA8888 pixels out | output rows |
| `JOB_INET_TREE_SUM` (3) | add-tree depth in, folded value + per-worker counters out | redexes per round |

## First workload: banded SVG rasterization (D4)

`nexus-svg` gained a reusable band API for this: `plan_document_at` tessellates
once into an immutable `RasterPlan`; `RasterPlan::rasterize_rows(y0, y1, …)`
renders any row band byte-identically to the full rasterize (rows carry no
cross-row state — proven by `userspace/ui/svg/tests/band_parity.rs`). pinched
parses + plans once per job, shares the plan read-only across workers, and
each worker rasterizes its row band into the shared output buffer.

Proofs (QEMU markers, SMP=1 and SMP=2 profiles):

- `SELFTEST: pinched bounded ok` — oversized jobs rejected via the header.
- `SELFTEST: pinched determinism ok` — parallel mix result equals the local
  reference and `workers >= 1`.
- `SELFTEST: pinched svg ok` — the broker's banded parallel raster is
  byte-identical to a local full rasterize with the same library and
  `workers >= 1` (the speedup result-proof: identical pixels + parallel
  dispatch; wall-clock timing is deliberately not gated — QEMU MTTCG timing
  is not stable evidence).

The icon/asset bakes today run at build time (`userspace/ui/app-icons/build.rs`
et al.), so the selftest is the first runtime client. The intended real
clients are future runtime bakes and tree-shaped compiler passes; when one
lands, it adopts the SDK-internal pattern above (inline fallback, batch path
only).

## Second backend: the interaction-net evaluator (Phase E)

`source/libs/nexus-inet/` is a minimal interaction-combinator net evaluator
(Lafont-style agents ERA/CON/DUP plus a number leaf and a binary-add pair) —
the proof that the broker's job-graph interface really is backend-agnostic.
Nets are stored in a bounded, RECYCLING arena (exhaustion = header reject,
never OOM); ports are atomics, so workers share the arena in safe Rust; the
next-round redex queue is a locked deque (no lock-free experiments in v1 —
atomic port-linking is a later, separately proven step). Reduction is
round-based on the same workpool: every round partitions the current redex
list with the deterministic chunk math. The calculus is confluent, so
`workers = 1 ≡ workers = N` holds by construction — proven by the host
equality matrix (real threads: identical value AND interaction count) and the
QEMU markers `SELFTEST: inet determinism / bounded / parallel exec ok`.

Honest v1 scope: clients declare workloads (`JOB_INET_TREE_SUM` carries only
the tree depth); a generic net-serialization wire format is a documented
follow-up, as is the first real customer (tree-shaped compiler/type-checker
passes — symbolic, embarrassingly tree-parallel). GPU/distributed backends
remain future scope behind the same job-graph interface.

## Known bounds (v1)

- Job size: `MAX_JOB_ELEMS` u32 elements (64 KiB payload), SVG targets up to
  `MAX_SVG_JOB_DIM`² and `MAX_SVG_BYTES` source bytes — all rejected loudly
  via the header when exceeded.
- The service's bump allocator never frees: per-job parse/plan allocations
  accumulate. Fine for occasional batch jobs; a high-frequency caller needs
  the arena-reset follow-up first (tracked in the SMP/pinched task ledger).
- One job at a time (the server loop is synchronous); backpressure is the
  blocked sender, completion latency is bounded by the workpool run deadline.
