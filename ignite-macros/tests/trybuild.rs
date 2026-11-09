//! Compile tests for `#[derive(IgniteResource)]`: valid models compile, and bad
//! attribute use fails with a clear message.

#[test]
fn derive_compile_tests() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/pass/*.rs");
    cases.compile_fail("tests/fail/*.rs");
}
