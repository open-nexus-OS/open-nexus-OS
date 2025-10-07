//! Bundle manager daemon entrypoint wiring default transport to the shared service logic.

fn main() -> ! {
    bundlemgrd::touch_schemas();
    let artifacts = bundlemgrd::ArtifactStore::new();
    if let Err(err) =
        bundlemgrd::service_main_loop(bundlemgrd::ReadyNotifier::new(|| ()), artifacts)
    {
        eprintln!("bundlemgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
