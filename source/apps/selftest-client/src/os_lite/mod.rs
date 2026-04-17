extern crate alloc;

mod context;
mod dsoftbus;
mod ipc;
mod mmio;
mod net;
mod phases;
mod probes;
mod services;
mod timed;
mod updated;
mod vfs;

pub fn run() -> core::result::Result<(), ()> {
    let mut ctx = context::PhaseCtx::bootstrap()?;
    phases::bringup::run(&mut ctx)?;
    phases::routing::run(&mut ctx)?;
    phases::ota::run(&mut ctx)?;
    phases::policy::run(&mut ctx)?;
    phases::exec::run(&mut ctx)?;
    phases::logd::run(&mut ctx)?;
    phases::ipc_kernel::run(&mut ctx)?;
    phases::mmio::run(&mut ctx)?;
    phases::vfs::run(&mut ctx)?;
    phases::net::run(&mut ctx)?;
    phases::remote::run(&mut ctx)?;
    phases::end::run(&mut ctx)
}

// NOTE: Keep this file's marker surface centralized in `crate::markers`.
