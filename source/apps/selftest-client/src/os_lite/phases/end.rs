//! Phase: end (extracted in Cut P2-13 of TASK-0023B).
//!
//! Owns the end-of-run slice:
//!   `SELFTEST: end` marker emission +
//!   cooperative idle loop (yield until reset).
//!
//! Marker order and marker strings are byte-identical to the pre-cut body.
//!
//! Returns `!` because the cooperative idle loop never exits; this phase is
//! always the last call in `os_lite::run()`.

use nexus_abi::yield_;

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> ! {
    emit_line("SELFTEST: end");

    // Stay alive (cooperative).
    loop {
        let _ = yield_();
    }
}
