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
