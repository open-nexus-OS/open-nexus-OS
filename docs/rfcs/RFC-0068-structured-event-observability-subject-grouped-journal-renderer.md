# RFC-0068: Structured-event observability — subject-grouped journal + configurable renderer

- Status: **In progress** (2026-06-29) — the future-proof shape of the UART observability theme. Supersedes the *grouping axis* of the per-process verdict prototype (kept as the proven UX), not its UX.
- Owners: @runtime
- Created: 2026-06-29
- Links:
  - Builds on: **ADR-0040** (unified logging policy + the pure-observer proof model — this RFC is the concrete data model behind §3/§6), RFC-0003 (unified logging), RFC-0011 (logd journal), ADR-0033 (soft-real-time spine: `nsec`/spans), ADR-0027 (selftest two-axis).
  - Code: `source/libs/nexus-log`, `source/libs/nexus-abi` (the per-process verdict counters + `SYSCALL_BOOT_MODE`), `source/kernel/neuron/src/diag/log.rs` (kernel `GROUP` table), `source/services/logd` (`journal.rs`, `protocol.rs` — already a ring journal with `OP_APPEND`/`OP_QUERY`/`OP_STATS`), `source/apps/selftest-client`, `source/libs/nexus-proof-manifest`.
  - Memory: `uart-grid-config-expand-vision`, `fake-proof-marker-audit`.

## Problem — the grouping axis is wrong for the future

The implemented verdict grid (ADR-0040 §8) folds each subsystem's markers into one `[ts] OK <group> N/N <ms>` line, with a configurable per-group expand (`NEXUS_LOG_EXPAND=<group>`). The UX is right and proven. But the grouping is by **emitting process**, and that is the wrong axis:

- A subsystem's story is split across **three emitters**. Everything about `policyd` is spread over: `init` wiring it (grant/slots/wire — emitted by the *init* process, tagged `init:`), policyd's own boot markers (`policyd N/N`), and policyd's runtime (the selftest exercising it — printed raw post-flush). `NEXUS_LOG_EXPAND=policyd` shows only the middle slice. The same is true for windowd, gpud, every service.
- **A process cannot fold another process's lines.** Per-process atomic counters (what the prototype uses) can only group by source. Cross-process *subject* grouping (init's policyd line → the `policyd` group) is structurally impossible with per-process counters.
- **Text lines are not queryable or auditable.** "is this `ok` a real proof?" requires reading code; the proof harness greps text; post-run analysis re-parses text. There is no structured status, subject, or span.

We want the axis mature observability stacks use: group by **subject** (the subsystem a record is *about*), not by sender; and decouple **emission** from **rendering** so the console is a compact configurable view over a complete, structured stream.

## Design — structured events in spans, subject-scoped, one journal, many renderers

### 1. The Event (replaces the text line)

Every marker becomes a structured, **alloc-free, fixed-layout** record:

```
Event {
  ts_ns:   u64,          // monotonic nsec
  level:   Level,        // Error|Warn|Info|Debug|Trace
  subject: SubjectId,    // the SUBSYSTEM it is about — first-class (see §3)
  span:    SpanId,       // the lifecycle it belongs to (0 = none)
  name:    Str,          // short event name ("grant", "ready", "draw")
  status:  Status,       // Ok | Warn | Fail | Lifecycle  (see §5)
  fields:  [(Key, Val)]  // bounded inline kv (e.g. svc=policyd, ms=12) — no heap
}
```

`Str`/`fields` use bounded inline buffers (the existing `nexus-log` `LineBuilder`/`StrRef` discipline) — no `Vec`, no `format!`, consistent with the kernel UART+alloc constraint that shaped the current solution.

### 2. The Span = a subject lifecycle

A `Span` is `{ subject, begin_ns, end_ns }`. Events belong to a span. The verdict `policyd N/N <ms>` **is** a span summary: N child events, M with `status=Fail`, `end-begin` duration. Soft-real-time falls out for free (a span over its budget renders `WARN … slow`). Spans nest (boot → service → request).

### 3. Subject is first-class (the fix)

Each event carries the subject it is *about*, independent of the emitter. `init`'s capability grant for policyd emits `{subject: policyd, name: "grant", ...}` even though the emitter is the init process. `SubjectId` is a small stable id from a registry (service names + kernel subsystems `boot`/`as`/`smp`/`syscall`/`sys`/`kself`/`init`). Cross-process records with the same subject collect into one group **at the journal**, not in any one process.

### 4. The journal (logd) is the SSOT; the kernel ring bridges early boot

- Events flow to **logd** — already a ring journal with `OP_APPEND`/`OP_QUERY`/`OP_STATS`. logd indexes by subject/level/span/time. **Nothing is ever lost**: the journal holds the full stream; the console is only a view.
- **Early boot** (kernel, init, services before logd is up) appends to a small **kernel event ring** (alloc-free), drained into logd once it is up. This is the layered emission of ADR-0040 §3, now carrying structured events instead of text.

### 5. Rendering is a separate, configurable concern

Renderers consume the journal; none of them change what is emitted:

- **Console (UART) = the verdict grid**, grouped by **subject-span**, folded by default. The config is pure *render* state:
  - `expand=<subject,…>` — show that subject's full event stream raw, everything else folded (the workflow lever, now subject-correct: `expand=policyd` shows init's policyd-grant + policyd boot + policyd runtime together).
    - **Realized today WITHOUT the central collector (subject-keyed expand at the emit layer):** each emitter independently prints its lines raw when their SUBJECT is in the expand set, keyed by the bare subject name (not the emitter). So `NEXUS_LOG_EXPAND=policyd` makes init print its policyd wiring raw (init's `subject_expanded("init:policyd")` strips the `init:` prefix → matches `policyd`) AND policyd print its own markers raw (its per-process expand) — one keyword expands a subject's whole cross-process story while the compact `init`/`policyd` verdicts still summarize the default view. Implemented for the init wiring fold (orchestrator `iw`/`subject_expanded`, boot-proven 2026-06-29); the per-emitter verdict still groups by emitter, but the *debug* axis is already subject. The full default-view subject MERGE (one `policyd` row) still wants the journal (P4).
  - `level=<subject>=debug` — raise a subject's floor.
  - `filter=` — only certain subjects/levels.
  Default floor Warn + curated Info; Debug/Trace off. Early boot uses a minimal direct console renderer; once logd is up it drives.
- **Proof verifier** (`verify-uart` / `nexus-evidence`): matches the structured stream against the proof-manifest SSOT by `(subject, name, status)`, deny-by-default — **no text greps**.
- **Post-run**: `logd OP_QUERY` (filter by subject/level/span/time).

### 6. Fake-proof becomes structurally auditable

`status` is explicit: `Ok` is set **only** on a real check; a "service is up" marker is `status=Lifecycle` (not a proof). The verifier checks the *field*, not the text `"ok"`. Hollow `ready`-style markers can no longer masquerade as proofs (the `fake-proof-marker-audit` concern), and a green span verdict means real `Ok` events, not text that happens to contain "ok".

## Migration — the prototype is the stepping stone, not throwaway

The per-process verdict + config-expand (nexus_abi `set_verdict_fold`/`service_marker`/`service_verdict_arm`/`service_verdict_flush`/`set_verdict_expand`; kernel `diag/log.rs` GROUP table; `SYSCALL_BOOT_MODE`) proved the **UX** (grid + per-group expand + slow flag + proof-safe mode gate). The future model keeps that UX and swaps the foundation: text→events, sender→subject, distributed counters→central journal.

1. **Event/Span/SubjectId types** — shared no_std crate (extend `nexus-log`); the SubjectId registry.
2. **Emit path** — `nexus-log`/`diag::log` gain `event!(subject, name, status, fields)`; the existing facade calls become thin wrappers that fill `subject = self`. init tags `subject = svc` on its per-service records (`svc=X` is already in hand). Per-process counters become a *local* span cache that ships events.
3. **Kernel event ring** → drain to logd.
4. **logd renders** the grid + the verifier consumes structured (replaces the current per-process flush + `verify-uart` greps).
5. The per-process verdict functions become a thin compatibility shim, then retire.

## Phases (each builds + headless-self-tests: `just test-os headless` proof-green, plus an interactive headless boot to read the grid)

- **P1** — Event/Span/SubjectId types + host unit tests (format, subject registry, span math). No boot change. **DONE 2026-06-29**: `source/libs/nexus-event` (Level/Status/Subject/Event/SpanTally/`verdict_from`/Verdict + SLOW_BUDGET_MS), 9 host tests; fixed a real `0`-as-unset timestamp bug via an `Option<u64>` start anchor.
- **P2** — fold the two duplicated verdict-math copies into the P1 SSOT, then add the `event!` emit API. **P2a DONE 2026-06-29**: `nexus_abi::service_verdict_flush` + kernel `diag::log::flush_group` both call `nexus_event::verdict_from` (kernel gained `nexus-event` as its first workspace-lib dep — a pure no_std leaf, same category as `bitflags`; kernel groups now also get the slow-WARN flag). Behaviour identical, headless proof-green (proof mode never folds → byte-identical trace). **P2b TODO**: the `event!(subject,name,status)` producer in `nexus-log`/`diag::log` and the facade wrappers (`subject=self`).
- **P4 (now BEFORE P3 — dependency found during P1/P2)** — the central subject-keyed collector. Cross-process subject grouping (init's policyd-grant folding into `policyd`'s verdict) is **structurally impossible** in any single process — it needs the journal as the collector. So the kernel event ring + logd drain + the logd-side subject-grouping renderer must land before P3's subject grouping can render. Console becomes a logd view (the direct per-process renderer stays the early-boot fallback).
- **P3** — subject tagging at the emit sites, now that P4 gives subjects a home: init tags `subject=svc` on its per-service records; the logd renderer folds them into the matching subject group (the visible policyd-split fix). The init half intersects the init-orchestrator god-file track — sequence it there, do not force a per-line funnel into the god-file mid-RFC. **P3 first slice DONE 2026-06-29**: the unconditional `init: start/up X` spawn ladder (44 lines) folds into one `init:spawn N/N <ms>` verdict (surgical instrumentation in `orchestrator.rs`, no god-file refactor; a `SpanTally` + `init_fold` gate). Boot-verified: interactive shows `OK init:spawn 22/22 27ms` (492→450 lines); proof stays raw (44 lines, 0 verdict, proof-transparent). **TODO**: the asymmetric per-service wiring lines (priority-wired/slots/route/`wire <svc> xfer…`) → per-service `init:<svc>` via `VerdictTable` (alive through bootstrap) — this is where the cross-subject collector pays off, and where it overlaps the god-file track.
- **P5** — proof verifier consumes the structured stream by `(subject,name,status)`; `status=Lifecycle` vs `Ok`; manifest stays SSOT. Retire the per-process shim.

## Alternatives considered

- **Keep per-process verdicts, add subject by content-parsing the text** (e.g. grep `svc=policyd` to re-bucket). Rejected: fragile string parsing, and folding still cannot cross processes without a central collector — it would be the journal in disguise, minus the structure.
- **Per-process only, accept the split.** Rejected: it is exactly the limitation the user flagged; a subsystem's story stays scattered and `expand` is incomplete.
- **A heap-backed structured log (Vec/format!).** Rejected: violates the alloc-free constraint that shaped the existing solution (kernel UART+alloc problems).

## Cross-cutting

- **Alloc-free** throughout (fixed records + bounded inline fields + the kernel ring); no per-event heap.
- **Atomic console writes** preserved (ADR-0040 §4) — a rendered line is one write.
- **Proof stays deny-by-default**; the manifest is the SSOT; only the transport (text greps → structured match) changes.
- **Legal**: described generically; no company/product names in tree (the os_log/structured-tracing *pattern*, not any product).
