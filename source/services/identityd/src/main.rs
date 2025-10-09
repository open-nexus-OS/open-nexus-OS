//! Identity daemon entry point wiring transports to the shared server logic.

fn main() -> ! {
    identityd::touch_schemas();
    if let Err(err) = identityd::service_main_loop(identityd::ReadyNotifier::new(|| ())) {
        eprintln!("identityd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
