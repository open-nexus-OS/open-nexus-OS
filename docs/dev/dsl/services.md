<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Services & Effects

Service calls are performed from **effects**, not reducers.

## Rule: reducers are pure

- no IO
- no `svc.*`
- no DB/files

## Service adapters

Place thin adapters under `ui/services/**.service.nx` to keep effects readable and consistent:

- stable timeouts
- deterministic error mapping
- explicit policy boundaries (no hidden ambient authority)

## Query posture

For query-shaped data loading, use this split:

- build QuerySpec in pure store/composable code,
- execute QuerySpec only from effects/service adapters,
- keep limits/order/page tokens explicit,
- and return stable error codes rather than stringly failures.

This lets data surfaces stay deterministic while still using real services.

Good fit:

- picker/files/content-provider queries,
- history/bookmark/search-like local indexes,
- feed/timeline cache views,
- and connector-backed tables or dashboards.

Bad fit:

- command actions,
- mutations/settings writes,
- send/launch/open side effects,
- or protocol/session control flows.

Those should remain domain-specific service calls even if they internally consult queryable storage.

## Example (illustrative)

```nx
effect(event) {
  match event {
    LoadRequested => {
      let res = UserService.getUserList(timeoutMs=250)
      emit(Loaded(res))
    }
  }
}
```
