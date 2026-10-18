//! Compile-fail snapshots for every Z01xx macro error.

#[test]
fn ui() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
