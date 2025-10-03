//! Clipboard daemon entry point.

fn main() {
    clipboard::run();
    println!("clipboardd: ready");
}
