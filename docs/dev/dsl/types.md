<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Type System

The DSL is statically typed, deterministic, and deliberately small. Every value that can
appear in state, props, events, or expressions has one of the types below. There are no
user-defined generics (see `patterns.md` for the component/props patterns that replace
them); the built-in parametric types (`List`, `Option`, `Result`) are the only ones.

## Scalar types

| Type   | Representation | Notes |
|--------|----------------|-------|
| `Bool` | true/false | |
| `Int`  | signed 64-bit integer | wrapping is an error (checked); use explicit saturating ops if needed |
| `Fx`   | Q32.32 fixed-point | the **only** fractional type — no floats anywhere in observable semantics; matches the layout engine's fixed-point pixels |
| `Str`  | UTF-8, length-capped | cap from the program budget or a per-field cap; concatenation is bounded |

Float nondeterminism is excluded structurally: there is no `Float` type. Literals with a
decimal point (`1.5`) are `Fx`.

## Composite types

| Type | Notes |
|------|-------|
| `Enum` | declared by `Event` declarations and standalone enums; matched exhaustively |
| `Record` | nominal, flat-ish product type; fields are typed; construction is total (all fields) |
| `List<T>` | ordered, **capacity-capped** (cap from budget or declaration); the workhorse collection |
| `Option<T>` | present/absent; must be matched (no implicit unwrap) |
| `Result<T, E>` | returned by every `svc.*` call; `E` is always a stable error-code enum; both arms must be handled (error otherwise) |

## Id and token types

- **Typed ids**: route params and entity references are nominal id types
  (e.g. `UserId`), not bare `Int`/`Str` — prevents cross-wiring.
- **Design tokens**: modifier arguments are token types (`ColorToken`, `LengthToken`,
  `TypographyToken`, `MotionToken`, …) generated from the theme SSOT
  (`resources/themes/*.nxtheme.toml`). Raw values (hex colors, px numbers) are not
  expressible in app code; raw values belong to theme authoring.
- **Field handles**: query predicates take generated typed field handles
  (e.g. `UserField::Role`), never strings (see `db-queries.md`).

## Where types live

| Position | Allowed types |
|----------|---------------|
| `Store` fields | all data types; `@persist` fields must be serializable (all data types are) |
| `Event` payloads | all data types |
| `Component` props | all data types + `EventRef` (a typed reference to an event to emit) |
| Route params | scalar + id types |
| Modifier args | token types + `Int`/`Fx`/`Bool` where the catalog says so |
| `device.*` | read-only environment record (see `profiles.md`) |

## Budgets

Budgets are part of the type system's contract and are carried in the IR:

- every `List<T>` has a capacity (default from the program budget, override per field);
- every `Str` has a length cap;
- expression trees have a node budget per reducer/effect;
- exceeding a budget at build time is an error; at runtime (e.g. a service returning too
  many rows) it is a deterministic, observable error value — never silent truncation.

## Type inference

Local and minimal: literals and `let` bindings infer from the right-hand side; state and
prop fields, event payloads, and props are always explicitly annotated. There is no
global inference — a declaration's type is readable at its site.

## Changelog

- **v1 (2026-07-06)** — initial contract: scalars (`Bool/Int/Fx/Str`), composites
  (`Enum/Record/List/Option/Result`), typed ids, token types, budgets-as-types.
