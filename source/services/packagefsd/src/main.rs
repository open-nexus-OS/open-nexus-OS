//! CONTEXT: Packagefsd entrypoint wiring default transport to shared service logic
//! Package file system daemon entrypoint.

fn main() -> ! {
    #[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
    packagefsd::touch_schemas();
    if let Err(err) = packagefsd::service_main_loop(packagefsd::ReadyNotifier::new(|| ())) {
        eprintln!("packagefsd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
