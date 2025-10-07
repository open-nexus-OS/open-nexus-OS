//! Thin SAMGR daemon entrypoint: wires transports to the shared server logic.

fn main() -> ! {
    samgrd::touch_schemas();
    if let Err(err) = samgrd::service_main_loop(samgrd::ReadyNotifier::new(|| ())) {
        eprintln!("samgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
