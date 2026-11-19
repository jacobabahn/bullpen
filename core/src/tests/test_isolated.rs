use super::*;

#[test]
fn core_s_tests_read_no_settings_of_this_machine_s() {
    let _held = crate::testenv::held(); // not while a test has its own (testenv's Env)
    assert_isolated();
}
