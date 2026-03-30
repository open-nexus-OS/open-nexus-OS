# Policy recipes

Capability policies for services live in this directory. Each `*.toml` file must
conform to the following structure:

```toml
[allow]
"service-name" = ["capability.one", "capability.two"]

[abi_profile."service-name"]
statefs_put_allow_prefix = "/state/app/service-name/"
net_bind_min_port = 1024
```

Files are merged in lexical order and later files override earlier entries for
the same service. Service and capability names are normalized to lowercase
before evaluation.

## Adding new policy files

1. Create a new `*.toml` file with your overrides or service additions.
2. List every capability the service requires; omit any optional capabilities.
3. Keep service names consistent with their bundle manifest entries.
4. Place temporary development overrides in `recipes/policy/local-*.toml` to
   ensure they sort after `base.toml`.

Unknown services default to an empty allowlist, so any non-empty capability
request will be denied unless explicitly permitted.

## ABI profile section (TASK-0019)

`[abi_profile."<service>"]` configures static boot/startup ABI syscall guardrail
profiles served by `policyd`:

- `statefs_put_allow_prefix`: optional bounded path prefix for `statefs.put`
  allow-rules. Unset means deny-by-default.
- `net_bind_min_port`: optional inclusive lower bound for `net.bind` allow-rules
  (`port >= min_port`). Unset means deny-by-default.

Profiles remain static in this task slice (no runtime hot reload / mode switch).
