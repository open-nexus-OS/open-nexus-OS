#[test]
fn indexing_ready() {
    assert!(searchd::execute(&[]).contains("indexing"));
}
