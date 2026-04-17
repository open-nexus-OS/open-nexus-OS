//! Phase orchestration verbs (TASK-0023B Phase 2 two-axis architecture).
//!
//! Each `phases::<name>::run(&mut PhaseCtx)` owns the corresponding slice of the
//! original `os_lite::run()` body. Phase modules MUST NOT import other
//! `phases::*` modules (mechanically enforced from Phase 3 onward by
//! `scripts/check-selftest-arch.sh`). Allowed downstream imports: `services::*`,
//! `ipc::*`, `probes::*`, `dsoftbus::*`, `net::*`, `mmio::*`, `vfs::*`,
//! `timed::*`, `updated::*`, `markers::*`, `crate::os_lite::context::PhaseCtx`.

pub(crate) mod bringup;
pub(crate) mod end;
pub(crate) mod exec;
pub(crate) mod ipc_kernel;
pub(crate) mod logd;
pub(crate) mod mmio;
pub(crate) mod net;
pub(crate) mod ota;
pub(crate) mod policy;
pub(crate) mod remote;
pub(crate) mod routing;
pub(crate) mod vfs;
