use nexus_alloc::BumpAllocator;

#[test]
fn reset_allows_reuse() {
    let mut bump = BumpAllocator::new(100, 32);
    let first = bump.alloc(8, 8).unwrap();
    assert!(first >= 100);
    bump.reset();
    let second = bump.alloc(8, 8).unwrap();
    assert_eq!(first, second);
}
