//! CONTEXT: Tests for userland scheduler deadline helper
use nexus_sched::Deadline;

#[test]
fn deadline_from_ms_scales() {
    let d = Deadline::from_ms(2);
    assert_eq!(d.ticks, 2000);
}
