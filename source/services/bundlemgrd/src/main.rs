//! Bundle manager daemon entrypoint wiring default transport to the shared service logic.

fn main() -> ! {
    bundlemgrd::touch_schemas();
    println!("bundlemgrd: ready");
    if let Err(err) = bundlemgrd::run_default() {
        eprintln!("bundlemgrd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
