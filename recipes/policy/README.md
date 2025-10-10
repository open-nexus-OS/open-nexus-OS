# Policy recipes

Capability policies for services live in this directory. Each `*.toml` file must
conform to the following structure:

```toml
[allow]
"service-name" = ["capability.one", "capability.two"]
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
