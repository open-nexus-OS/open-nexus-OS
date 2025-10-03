//! Settings daemon entry point: delegates to the userspace library.

fn main() {
    settings::run();
    println!("settingsd: ready");
}
