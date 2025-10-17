//! Package file system daemon entrypoint.

fn main() -> ! {
    packagefsd::touch_schemas();
    if let Err(err) = packagefsd::service_main_loop(packagefsd::ReadyNotifier::new(|| ())) {
        eprintln!("packagefsd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
