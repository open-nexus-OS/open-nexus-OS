---
title: TASK-0075 DSL v0.1a: frontend (lexer/parser/AST/typecheck/lints/fmt) + canonical capnp IR + nx-dsl CLI backend + host proofs
status: Draft
owner: @ui @runtime
created: 2025-12-23
updated: 2026-07-06
depends-on: []
follow-up-tasks:
  - tasks/TASK-0076-dsl-v0_1b-interpreter-snapshots-os-demo.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Language reference (SSOT this task implements): docs/dev/dsl/grammar.md, docs/dev/dsl/types.md,
    docs/dev/dsl/modifiers.md, docs/dev/dsl/principles.md, docs/dev/dsl/ir.md
  - Data formats rubric: docs/adr/0021-structured-data-formats-json-vs-capnp.md
  - CLI shim this backend satisfies: tools/nx/src/commands/dsl.rs (NX_DSL_BACKEND delegation)
  - IDL convention: tools/nexus-idl/schemas/ (new ui_ir.capnp lives here)
  - UI layout pipeline contract: docs/dev/ui/foundations/layout/layout-pipeline.md
  - Design-system consumer baseline: tasks/TASK-0073-ui-v10a-design-system-primitives-goldens.md
---

## Context (updated 2026-07-06)

The DSL is 0% implemented: the only code is the `nx dsl` CLI shim
(`tools/nx/src/commands/dsl.rs`) which delegates `fmt/lint/build` to an external
`NX_DSL_BACKEND` binary that does not exist. The language reference in `docs/dev/dsl/`
(grammar/types/modifiers/principles/ir) is the normative SSOT this task turns into code.

This task delivers the deterministic foundations — frontend, canonical IR, CLI — for the
**v1 canonical surface** (see `grammar.md#changelog`): direct store fields, top-level
`Event`/`reduce`/`@effect on`, `if/else` view conditionals, chained hybrid-utility
modifiers (`.padding(4) .bg(accent) .textSize(sm) .rounded(md)`), `Page` body = view,
`List(expr) { item in … }` keyed collections.

**Fixed architecture decisions (masterplan 2026-07-06):**
- Reducers/effects lower to **typed total expression trees** (no bytecode VM);
  termination by construction; numerics = `Int`(i64) + `Fx`(Q32.32), no floats.
- Canonical IR = Cap'n Proto (`tools/nexus-idl/schemas/ui_ir.capnp`), derived
  `.nxir.json` for goldens only. Determinism: `build; build; cmp` byte-identical.
- The checker core must be **no_std-capable** so it can later run in-system
  (host tests now, on-device validation later).
- No godfiles: module layout per crate is pinned in the masterplan (≤ ~600 LOC/file).

## Goal

1. **Crates** (explicit members in root `Cargo.toml`):
   - `userspace/dsl/core` (`nexus-dsl-core`, no_std+alloc, feature `std` for host IO/
     pretty diagnostics): `lexer.rs`, `parser/` (one module per construct), `ast.rs`,
     `resolve.rs`, `typeck/`, `lint/` (one module per lint + registry), `canon.rs`,
     `fmt.rs`, `diag.rs` (structured diagnostics: code/span/message-id), `lower/`.
     Contains `modifiers.toml` (SSOT: modifier catalog + field classes layout/paint/
     semantics); `build.rs` generates the Rust table + the `docs/dev/dsl/modifiers.md`
     catalog table.
   - `userspace/dsl/ir` (`nexus-dsl-ir`, no_std+alloc): typed zero-copy wrappers over
     the generated `ui_ir` capnp module, structural validation (budgets, version gate,
     re-typecheck), canonical hashing, stable NodeId derivation, field-class table.
     Writer side behind feature `write`.
   - `userspace/dsl/cli` (`nx-dsl` bin, std): satisfies the shim's delegation contract
     (`fmt`/`lint`/`build` argv), plus direct verbs `check`, `hash`, `explain <code>`.
     `NX_DSL_BACKEND` gets wired by the justfile/dev scripts.
2. **Grammar v1** per `docs/dev/dsl/grammar.md` (the whole surface: stores/events/
   reducers/effects, pages/components/props, if/for/match/collections, modifiers,
   handlers, routes, `@t`, `$state`/`$props`/`device.*`, `NativeWidget` parse-only).
3. **IR v1.0** per `docs/dev/dsl/ir.md`: `UiProgram` with interned sorted symbols,
   expression-tree reducer bodies, bounded effect plans, persisted stable NodeIds
   (`hash64(component ∥ path ∥ key)`), field classes per binding site, budgets,
   `programHash`/`sourceDigest`, canonical encoding.
4. **Lints (Error unless noted)**: reducer purity; unhandled `Result` arms; missing
   `.key(expr)` on collection items; missing `.label()` on unlabeled interactive nodes;
   duplicate modifiers; non-exhaustive match; unbounded `for`; import conflicts
   (deterministic ordering); `if device.profile` without final `else` (Warning,
   `--deny-warn` promotes). Spans + stable diagnostic codes on everything.
5. **Module resolution**: explicit `import` only; stable alias roots (`@app/...`);
   deterministic conflict errors.

## Non-Goals

- Interpreter/rendering/snapshots (TASK-0076), stores runtime (TASK-0077),
  codegen (TASK-0079), OS anything.
- Kernel changes.

## Constraints / invariants (hard requirements)

- Deterministic formatting, hashing, IR serialization; no host timestamps/paths in
  outputs.
- Bounded parsing: cap file size, recursion depth, identifier length.
- `nexus-dsl-core` + `nexus-dsl-ir` build for `riscv64imac-unknown-none-elf`
  (no_std check in CI) — the in-system-checker requirement.
- No `unwrap/expect`; no blanket `allow(dead_code)`; no company/product names.
- No godfiles (module layouts above are binding).

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_1a_host/`:

- parser corpus: accept + reject fixtures with snapshotted diagnostic codes/spans;
- `parse → fmt → parse` idempotent (structural AST equality); `fmt(fmt(x)) == fmt(x)`;
- `nx dsl build` twice → **byte-identical** `.nxir` (`cmp`); IR golden fixtures
  (canonical bytes + derived JSON) stable;
- the load-time validator rejects: budget overflow, unknown major version, type
  mismatch, dangling symbol refs (fixture per case);
- capnp no_std probe: a ≥100 KB `.nxir` fixture read with bounded traversal limits on
  the riscv target build (unit test compiled for host, build-checked for riscv).

CLI:

- `nx dsl fmt --check` non-zero on needed changes; `nx dsl lint` non-zero on errors;
  `nx dsl build` emits canonical `.nxir` under `target/dsl/<app>/`;
  `--emit-json` adds `.nxir.json`;
- the existing shim (`tools/nx dsl …`) round-trips through `NX_DSL_BACKEND` (delegation
  smoke test).

Fixtures include one page modeling the shared proof surface (text, icon, list,
overlay areas) so later interpreter/OS tasks mount the same structure.

### Docs — required (reference grade)

- `docs/dev/dsl/{syntax,cli,modifiers}.md` current with the shipped surface;
  `modifiers.md` table generated from `modifiers.toml`;
- `docs/dev/dsl/ir.md` changelog entry v1.0 + regenerated IR goldens;
- diagnostics catalog (`nx dsl explain`) documented in `cli.md`.

## Touched paths (allowlist)

- `userspace/dsl/{core,ir,cli}/` (new), root `Cargo.toml` (members)
- `tools/nexus-idl/schemas/ui_ir.capnp` (new) + `userspace/nexus-idl-runtime` (module)
- `tests/dsl_v0_1a_host/` (new)
- `docs/dev/dsl/{syntax,cli,modifiers,ir}.md`, `justfile` (NX_DSL_BACKEND wiring)

## Plan (small PRs)

1. crate scaffolding + `ui_ir.capnp` v1.0 + idl-runtime module + no_std CI checks
2. lexer + parser + AST + diagnostics (per-construct parser modules)
3. resolve + typecheck + lint registry
4. canonicalize + fmt (idempotence proofs)
5. lower/ (AST → IR) + validation + hashing + NodeIds
6. `nx-dsl` CLI + shim wiring + determinism proofs + docs

---

## STATUS / PROGRESS LEDGER (updated 2026-07-06)

> First implementation day. The end-to-end chain **works**: `.nx` source →
> lexer → parser → checker → canonical fmt → lowering → canonical `.nxir`
> (capnp, byte-deterministic, hash-verified) → CLI. All host tests green
> (23 core units + 3 ir units + 8 host-suite), riscv no_std green for
> core+ir (incl. capnp+sha2), workspace metadata intact. Uncommitted.

### ✅ DONE

- **Crates**: `userspace/dsl/{core,ir,cli}` (workspace members + `userspace/dsl` excluded from the glob).
  Module layout as pinned (no godfiles): core = `lexer/ast/diag/parser{mod,decls,stmt,expr,view}/fmt/registry/check{mod,names,lints}/lower{mod,exprs,views}`.
- **Schema**: `tools/nexus-idl/schemas/ui_ir.capnp` v1.0 (UiProgram, budgets, expression-tree
  reducers, linear effect plans w/ onOk/onErr semantics, persisted NodeIds, TypeRef incl.
  `opaque` placeholder). Changelog entry in `docs/dev/dsl/ir.md`.
- **nexus-dsl-ir**: bounded `ProgramReader` (traversal/nesting limits, single-segment form),
  `validate_program` (major gate, hash recompute via re-canonicalization, symbol canonicality,
  ref bounds, view budgets), SHA-256 `programHash`, FNV-1a-64 NodeIds (`static_node_id`/`keyed_item_id`).
- **Lexer** (bounded: 512KiB file, 128B idents; Fx=Q32.32 pure integer math) + **parser**
  (full v1 grammar: stores/events/reduce/@effect/pages/components/props/routes, if/for/match,
  collections `List(e){x in}`, positional sugar, modifiers, handlers; fail-fast, nesting≤64).
- **Formatter**: canonical layout, `fmt∘parse∘fmt = fmt` proven; precedence-exact paren
  re-emission; Fx 10-digit round-trip-exact printing.
- **Checker** (collects all diagnostics): duplicates, unknown event/case/widget/modifier/
  device-field, reduce exhaustiveness + binds arity, dispatch arity, route dup/unknown page,
  reducer purity, collection `.key` (incl. through if/match branch roots), a11y labels,
  duplicate modifiers, bounded `for`, profile-else warning, svc timeout/result (warnings in
  v0.1, → errors with 0077B/0078).
- **Lowering**: interned sorted symbols; stores/events/components/routes/reducers/effects in
  canonical order (decl order proven not to leak); expression lowering (state/props/device/
  locals/record-get chains/i18n); effect plans (`let x = svc…` = call step + continue-on-Ok,
  `match svc…{Ok/Err}` → onOk/onErr); NodeIds persisted; **build×2 byte-identical**;
  self-validating incl. hash; tamper test fails closed. v0.1 subset limits report `NX0501`.
- **CLI `nx-dsl`**: fmt/lint/check/build(-o/--emit-json)/hash/explain; satisfies the
  `nx dsl` shim (`NX_DSL_BACKEND` delegation verified); `just dsl` + `just nx-dsl-shim`.
- **Host suite** `tests/dsl_v0_1a_host/`: reject corpus w/ stable codes, accept corpus,
  byte-determinism, IR golden (`goldens/proof_surface.nxir`, UPDATE_GOLDENS=1), loader
  reject tests, fmt-reflow-same-IR. Proof-surface fixture = the shared mount target for 0076B+.
- **Docs**: `cli.md` reference-grade (shipped vs planned); `ir.md` changelog v1.0.

### ⬜ OPEN (within this task)

- **Full type inference** (typeck/): v0.1 lowers unknown/derived types as `opaque` (validator
  skips those); real inference + `Result`-arm typing lands before the interpreter needs it
  (with 0076) or as a follow-up increment here. `NX0301/0306/0307` are defined but not yet raised.
- **Module resolution** (`import`, `@app/…` roots, cross-file merge): parsed but single-file
  programs only; multi-file + conflict codes (NX0203) pending.
- **`modifiers.md` table generation** from `registry.rs` (SSOT exists in code; docs table is
  hand-maintained until the emit verb lands — decision: const table in `registry.rs` replaced
  the planned modifiers.toml, same SSOT function, zero build deps).
- **capnp no_std riscv READ probe as a runtime test** (build-check green; an on-target read
  exercise rides with 0076B's selftest).
- justfile `NX_DSL_BACKEND` default export for dev shells (recipes exist; env wiring optional).

### Notes for whoever continues

- Semantics decisions already binding: effect-plan step semantics (continue-on-Ok,
  onErr-stops), `state.x` (reducers) ≡ `$state.x` (views), single-store v0.1 binding,
  match-view lowers to equality branch chain (binds unsupported → NX0501).
- Canonical-source contract: `build` lowers from the **formatted** text (sourceDigest =
  sha256(canonical source)), so fmt-then-build == build.
- Never change NodeId hashing or canonicalization within schema major 1 — goldens +
  `docs/dev/dsl/ir.md#schema-evolution-rules` enforce.
