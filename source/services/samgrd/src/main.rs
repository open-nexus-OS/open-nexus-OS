//! CONTEXT: SAMGR daemon entrypoint wiring default transport to shared server logic

fn main() -> ! {
    samgrd::touch_schemas();
    if let Err(err) = samgrd::service_main_loop(samgrd::ReadyNotifier::new(|| ())) {
        eprintln!("samgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
