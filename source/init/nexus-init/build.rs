// RFC-0068: the subject-keyed `NEXUS_LOG_EXPAND` debug-expand (orchestrator `subject_expanded`) is
// read at compile time via `option_env!` — declare the dependency so changing it rebuilds init.
fn main() {
    println!("cargo:rerun-if-env-changed=NEXUS_LOG_EXPAND");
}
