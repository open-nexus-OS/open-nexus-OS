# Process lifecycle & supervision

The Neuron kernel now models a task's lifetime with explicit `Running → Zombie → Reaped`
transitions. A task invokes the `exit` syscall to publish its status and transition into the
`Zombie` state. The kernel preserves the task control block, address space handle, and exit
code until the parent issues a matching `wait`. Parents may wait on a concrete child PID or
any zombie descendant. Once a parent reaps a child the kernel frees the remaining task
resources and returns the `(pid, status)` pair to userspace.

## Syscalls

Two syscalls drive the lifecycle protocol:

- `exit(status: i32)` never returns. The kernel records `status`, tears down the running
  context, and leaves the task in the zombie table until a parent reaps it.
- `wait(pid: i32)` blocks (with cooperative yields) until a zombie child is available. A
  positive `pid` targets a single child, while `pid <= 0` matches the first zombie owned by
  the caller. Errors map to the conventional errno set: `ECHILD` when no children exist,
  `ESRCH` for unrelated PIDs, and `EINVAL` when arguments are malformed.

The userspace ABI exposes safe wrappers for both syscalls. `nexus_abi::exit` performs the
non-returning call, and `nexus_abi::wait` returns a `(Pid, i32)` pair mirroring the kernel
semantics while translating lifecycle errors into `AbiError` variants.

## execd supervision loop

`execd` maintains a registry of launched services keyed by bundle name. Each entry records
its kernel PID, restart policy, and the `argv`/`env` vectors required to respawn the service.
A dedicated reaper thread calls `nexus_abi::wait(0)` to harvest any exited child and logs a
marker of the form `execd: child exited pid=<pid> code=<status>` whenever a termination is
observed. If the registered policy is `restart=always` the reaper transparently re-execs the
bundle and emits `execd: restart <service> pid=<pid>` when the replacement process starts.

Policies currently supported by the supervisor are:

- `always` – respawn immediately after every exit.
- `never` – log the exit and leave the service stopped.

Future revisions can extend the enum to cover additional strategies (e.g. `on-failure`).

## init supervision hints

`nexus-init` publishes the restart posture for each core daemon at boot time. The init log
now includes lines such as `init: supervise samgrd restart=always`, which describe the
expected policy that execd should enforce for the service tree.

## Demo workload

The `demo.exit0` payload packages a tiny RISC-V ELF that prints `child: exit0 start`, calls
`nexus_abi::exit(0)`, and ships with a manifest suitable for bundlemgrd staging. The OS
selftest client installs this bundle, starts it through execd, waits for the supervisor log,
and prints `SELFTEST: child exit ok` once the lifecycle markers appear.
