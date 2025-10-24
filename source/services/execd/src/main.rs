//! CONTEXT: Execd daemon entrypoint wiring default transport to shared service logic
fn main() -> ! {
    execd::touch_schemas();
    if let Err(err) = execd::service_main_loop(execd::ReadyNotifier::new(|| ())) {
        eprintln!("execd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
