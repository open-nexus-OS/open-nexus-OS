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
  # -- launch authority (abilitymgr, RFC-0065; TASK-0080B shell surface)
  (service = "ability", method = "launch", args = ["Str"], result = "Bool"),
  # -- app state (statefsd-backed contract)
  (service = "appState", method = "get", args = ["Str"], result = "Str"),
  (service = "appState", method = "put", args = ["Str", "Str"], result = "Bool"),
  # -- app registry (bundlemgrd ENUMERATE; filter = "" lists all)
  (service = "bundlemgr", method = "enumerate", args = ["Str"], result = "List<AppEntry>"),
  # -- demo/test surface (conformance corpus + example apps)
  (service = "catalog", method = "list", args = [], result = "List<Str>"),
  (service = "db", method = "put", args = ["Str", "Str"], result = "Bool"),
  # -- file surface (vfsd via RFC-0073; FILES permission, filemanager role)
  (service = "files", method = "list", args = ["Str", "Int", "Str"], result = "List<FileEntry>"),
  (service = "files", method = "mkdir", args = ["Str"], result = "Bool"),
  (service = "files", method = "remove", args = ["Str"], result = "Bool"),
  (service = "files", method = "rename", args = ["Str", "Str"], result = "Bool"),
  (service = "files", method = "copy", args = ["Str", "Str"], result = "Bool"),
  (service = "files", method = "count", args = ["Str"], result = "Int"),
  (service = "files", method = "stat", args = ["Str"], result = "FileEntry"),
  (service = "library", method = "get", args = ["Str"], result = "Str"),
  (service = "library", method = "list", args = [], result = "List<Str>"),
  (service = "search", method = "query", args = ["Str"], result = "List<Str>"),
  # -- system settings (settingsd typed registry; presentation keys
  #    `ui.theme.mode`/`ui.shell.mode` route through windowd — the single
  #    presentation authority — and come back as theme/profile pushes)
  (service = "settings", method = "get", args = ["Str"], result = "Str"),
  (service = "settings", method = "set", args = ["Str", "Str"], result = "Bool"),
  # -- session authority (sessiond, TASK-0065B contract; the DSL greeter
  #    renders and dispatches — sessiond DECIDES (authority stays there))
  (service = "session", method = "login", args = ["Str", "Str"], result = "Bool"),
  (service = "session", method = "users", args = [], result = "List<Str>"),
  # -- OSK key injection (imed's dedicated osk endpoint, RFC-0075 Phase 2;
  #    ime-type bundles only). `key` commits ONE character; `action` sends a
  #    control action ("backspace" | "enter").
  (service = "ime", method = "key", args = ["Str"], result = "Bool"),
  (service = "ime", method = "action", args = ["Str"], result = "Bool"),
  (service = "ime", method = "select", args = ["Int"], result = "Bool"),
  (service = "ime", method = "layout", args = ["Str"], result = "Bool"),
  # OSK row DATA (RFC-0075 Phase 8b): rows come from the keymaps SSOT —
  # adding a language is adding data, never an if-arm in an app.
  (service = "ime", method = "rows", args = ["Str", "Int"], result = "List<OskKey>"),
  # Cycles to the platform's NEXT layout after `current` (order = keymaps
  # SSOT) and switches it system-wide (imed persists input.keymap).
  (service = "ime", method = "cycle", args = ["Str"], result = "Bool"),
  (service = "stats", method = "count", args = ["Str"], result = "Int"),
  (service = "todos", method = "list", args = [], result = "List<Str>"),
  (service = "users", method = "list", args = [], result = "List<User>"),
];
