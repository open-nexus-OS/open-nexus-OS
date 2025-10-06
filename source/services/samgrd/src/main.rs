//! Thin SAMGR daemon entrypoint: wires transports to the shared server logic.

fn main() -> ! {
    samgrd::touch_schemas();
    println!("samgrd: ready");
    if let Err(err) = samgrd::run_default() {
        eprintln!("samgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
