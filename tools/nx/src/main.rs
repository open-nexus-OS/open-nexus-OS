//! CONTEXT: Binary entrypoint for the canonical `nx` host CLI.
//! INTENT: Delegate process exit behavior to `nx::run()` contract mapping.
//! TESTS: Covered transitively by `cargo test -p nx -- --nocapture`.

fn main() {
    std::process::exit(nx::run());
}
