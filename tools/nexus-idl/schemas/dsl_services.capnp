@0xd4a7c1e89b3f5620;
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

# Nexus UI DSL — the platform service surface (`svc.*`)
#
# SSOT for every service method the DSL may call. The frontend's signature
# table (`nexus-dsl-core`, unknown-service/method/arity diagnostics) is
# GENERATED from this file at build time, and the app-host routes `svc.*`
# calls against the same list — the DSL service surface IS the platform IDL,
# never a hand-maintained parallel definition (docs/dev/dsl/services.md).
#
# STYLE CONTRACT (the generator parses this file):
#   - one `(service = "…", method = "…", args = […], result = "…")` per entry
#   - `args`/`result` use DSL type names: Bool, Int, Fx, Str, List<Str>, …
#   - entries are sorted by (service, method); append in order.

struct DslMethod {
  service @0 :Text;
  method  @1 :Text;
  args    @2 :List(Text);   # DSL argument types, positional
  result  @3 :Text;         # DSL result type (the Ok payload)
}

const dslSurface :List(DslMethod) = [
  # -- app state (statefsd-backed contract)
  (service = "appState", method = "get", args = ["Str"], result = "Str"),
  (service = "appState", method = "put", args = ["Str", "Str"], result = "Bool"),
  # -- demo/test surface (conformance corpus + example apps)
  (service = "catalog", method = "list", args = [], result = "List<Str>"),
  (service = "db", method = "put", args = ["Str", "Str"], result = "Bool"),
  (service = "library", method = "get", args = ["Str"], result = "Str"),
  (service = "library", method = "list", args = [], result = "List<Str>"),
  (service = "search", method = "query", args = ["Str"], result = "List<Str>"),
  (service = "stats", method = "count", args = ["Str"], result = "Int"),
  (service = "todos", method = "list", args = [], result = "List<Str>"),
  (service = "users", method = "list", args = [], result = "List<User>"),
];
