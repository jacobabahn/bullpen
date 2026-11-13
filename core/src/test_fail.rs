use super::*;

#[test]
fn a_fail_says_its_message() {
    let r: Res<()> = fail("no task BP-9");
    assert_eq!(r.unwrap_err().to_string(), "no task BP-9");
}

#[test]
fn an_io_error_is_a_fail() {
    let e = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
    assert_eq!(Fail::from(e), Fail("gone".into()));
}
