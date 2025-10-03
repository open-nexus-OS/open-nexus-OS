//! Time synchronization daemon entry point.

fn main() {
    time_sync::run();
    println!("time-syncd: ready");
}
