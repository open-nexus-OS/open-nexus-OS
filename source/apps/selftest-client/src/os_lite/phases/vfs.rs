//! Phase: vfs (extracted in Cut P2-10 of TASK-0023B).
//!
//! Owns the userspace VFS probe over kernel IPC v1 (cross-process):
//!   `vfs::verify_vfs()` (success path emits its own granular markers from
//!   inside the verify routine; only the FAIL marker is emitted at this layer).
//!
//! Marker order and marker strings are byte-identical to the pre-cut body.

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::vfs;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    if vfs::verify_vfs().is_err() {
        emit_line("SELFTEST: vfs FAIL");
    }
    Ok(())
}
