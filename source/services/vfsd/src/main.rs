//! CONTEXT: Vfsd entrypoint wiring default transport to shared service logic
fn main() {
    if let Err(err) = vfsd::service_main_loop(vfsd::ReadyNotifier::new(|| {})) {
        eprintln!("vfsd: exited with error: {err}");
        std::process::exit(1);
    }
}
