//! Phase: exec (extracted in Cut P2-08 — execd spawn/exit/minidump + forged-metadata/spoof/malformed rejects).

use crate::os_lite::context::PhaseCtx;

#[allow(dead_code)]
pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    Ok(())
}
