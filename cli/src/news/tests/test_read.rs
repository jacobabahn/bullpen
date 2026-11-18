use super::*;

#[test]
fn cursors_as_python_reads_them() {
    assert_eq!(py_int(" 12\n"), Some(12));
    assert_eq!(py_int("+1_000"), Some(1000));
    assert_eq!(py_int("-5"), Some(-5));
    for bad in ["junk", "", "1__0", "_1", "1.5", "1 2"] {
        assert_eq!(py_int(bad), None, "{bad}");
    }
}
