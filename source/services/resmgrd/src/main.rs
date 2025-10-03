//! Resource manager daemon entry point.

fn main() {
    resmgr::run();
    println!("resmgrd: ready");
}
