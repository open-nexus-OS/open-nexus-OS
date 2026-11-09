<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Types and Values

The language is deliberately small and deterministic. Declarations carry type annotations;
the values the runtime holds are a closed set; there are no floats and no user-defined
generics ([patterns.md](patterns.md) shows the component/props patterns that replace
them).

## Annotations

Annotations appear on store fields, component props, component `state:` fields, event case
payloads, route params and query params.

| Annotation | Values | Zero value | Notes |
|---|---|---|---|
| `Bool` | `true` / `false` | `false` | |
| `Int` | signed 64-bit | `0` | arithmetic is checked: overflow and division by zero are runtime errors (`Overflow`, `DivByZero`), never wrapping |
| `Fx` | Q32.32 fixed point | `0` | the **only** fractional type; a literal with a decimal point (`1.5`) is `Fx`; multiplication and division widen to 128 bits and report `Overflow` if the result does not fit |
| `Str` | UTF-8 text | `""` | `+` concatenates |
| `List<T>` | ordered list | empty | `+` concatenates lists; `len`, `take`, `skip`, `tail` read them ([ir.md](ir.md)) |
| `Option<T>` | accepted as an annotation | no value | the surface language has no `Some`/`None` literal |
| `EventRef` | a reference to an event case | — | a **prop** type: how a component raises an event ([patterns.md](patterns.md)) |

Any other type name is accepted and lowers to `opaque` — "not statically known" — so it is
not checked. There is no `Float` type, so float nondeterminism cannot enter the semantics.

```nx
Store CartStore {
    count: Int = 0,
    scale: Fx = 1.5,
    title: Str = "cart",
    names: List<Str> = [],
}

Event CartEvent {
    Add(Str),
    Clear,
}

reduce CartEvent {
    Add(name) => state.names = state.names + [name],
    Clear => state.names = [],
}

Page CartPage {
    Stack {
        Text($state.title)
        Button { label: "Clear" }
        on Tap -> dispatch(Clear)
    }
}
```

## What the compiler checks, and what the runtime does

The compiler resolves names, arities and enum cases, requires every `reduce` and `match` to
cover every case (`NX0304`), enforces reducer purity (`NX0405`) and the `Ok`/`Err` pairing
of service calls (`NX0407`), and validates token arguments (below). It does **not** infer
or compare expression types: `Int + Str` compiles and fails when evaluated. At runtime
every value carries its kind (`Unit`, `Bool`, `Int`, `Fx`, `Str`, `List`, enum case,
record) and an operation on the wrong kind stops that evaluation with `TypeMismatch`.
Declare fields with the right annotation and keep expressions homogeneous.

## Composite values

| Value | Where it comes from |
|---|---|
| Enum case | an `Event` declaration: `Event NavEvent { Go, Pick(Str) }` declares the cases and their payload types; a case is written bare inside `dispatch(Pick("a"))` and as `NavEvent::Pick("a")` in expression position (for instance an `EventRef` prop); `reduce` and `match` cover every case |
| Record | the result of a service call (`row.name` reads a field); there is no record declaration or literal in the language |
| Service result | every `svc.*` call yields a result that must be matched with both `Ok(v)` and `Err(e)` arms; `Err` carries a stable numeric error code, never a formatted string ([services.md](services.md)) |

## Where annotations are allowed

| Position | Notes |
|---|---|
| `Store` fields | any annotation; a trailing `@persist` marks the field for the restore snapshot ([runtime.md](runtime.md)) |
| `Event` case payloads | `Case(Int, Str)` |
| `Component` `props:` | any annotation plus `EventRef` |
| Component `state:` | per-instance fields with a default ([state.md](state.md)) |
| Route params | `"/detail" -> DetailPage(id: Int)` ([navigation.md](navigation.md)) |
| `Query` params | `Bool`, `Int`, `Fx`, `Str` only (`NX0306` otherwise); `limit` is 1–1000 ([db-queries.md](db-queries.md)) |
| Modifier args | tokens, `Int`, `Fx`, `Bool` or expressions, as the catalog says ([modifiers.md](modifiers.md)) |
| `device.*` | read-only environment values ([profiles.md](profiles.md)) |

## Design tokens

Modifier arguments name the one token set (RFC-0116): colour roles, text styles, radii,
shadows, materials and motion tokens. They are closed vocabularies checked at compile time
(`TOKEN_VOCABULARIES` in `userspace/dsl/core/src/registry/vocab.rs`) and resolved through the
typed tokens (`ColorToken`, `TextStyleToken`, `RadiusToken`, `ShadowToken`, … in
`nexus-theme-tokens`), whose values come from `resources/themes/*.nxtheme.toml`. A name
outside the vocabulary is an error with a "did you mean" hint, and a retired name names its
replacement. A colour role may also come from data as a role-name `Str`
(`.bg($props.tint)`). A hex colour is not expressible in app code; only `.bgGradient`
artwork, which is data, takes `"#rrggbb"` strings. Spacing is whole px on the grid
`0 2 4 6 8 12 16 20 24 32 40 48` or a spacing name; sizes (`.width(320)`) and radii take raw
px as well.

## Bounds

| Bound | Value | Where |
|---|---|---|
| Source file | 512 KiB | lexer, `NX0003` |
| Identifier | 128 bytes | lexer, `NX0004` |
| Nesting depth | 64 levels | parser, `NX0105` |
| Literals | `Int` is `i64`; an out-of-range `Int` or `Fx` literal is `NX0005` | lexer |
| View nodes / children per node | 4096 / 64 | loader ([ir.md](ir.md)) |
| Query `limit` | 1–1000 | checker, `NX0410` |

Every program also carries list, string, expression and effect-step caps in its IR budgets;
the loader does not check them and the runtime does not truncate or reject a list or string
for its length.
