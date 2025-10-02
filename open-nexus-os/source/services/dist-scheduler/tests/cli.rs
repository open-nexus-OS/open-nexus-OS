#[test]
fn deadline_prints_ticks() {
    assert!(dist_scheduler::execute(&["3"]).contains("3000"));
}
