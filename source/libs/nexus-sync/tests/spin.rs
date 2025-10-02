use nexus_sync::SpinLock;

#[test]
fn concurrent_access_simulated() {
    let lock = SpinLock::new(0_u32);
    {
        let mut guard = lock.lock();
        *guard = guard.wrapping_add(1);
    }
    assert_eq!(*lock.lock(), 1);
}
