#[test]
fn invalid_calls_fail_to_compile() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/*.rs");
    tests.pass("tests/pass/*.rs");
}
