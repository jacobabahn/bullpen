/// Counts the writes made to it: a line written whole is one.
#[derive(Default)]
struct Writes(Vec<Vec<u8>>);

impl std::io::Write for Writes {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.push(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_line_to_a_log_is_one_write() {
    let (name, rest) = ("_missing:_push", "building it first");
    let mut w = Writes::default();
    line_to!(w, "bullpen: `{name}` is newer than this build: {}", rest).unwrap();
    assert_eq!(
        w.0,
        vec![b"bullpen: `_missing:_push` is newer than this build: building it first\n".to_vec()]
    );
    // what writeln! does with the same line: a write a piece
    let mut w = Writes::default();
    std::io::Write::write_fmt(
        &mut w,
        format_args!("bullpen: `{name}` is newer than this build: {}\n", rest),
    )
    .unwrap();
    assert!(w.0.len() > 1);
}

#[test]
fn eprintln_and_println_print_what_stds_do() {
    // what they print, the test harness takes: here, that each form builds
    let name = "x";
    eprintln!();
    eprintln!("bullpen: `{name}` and {}", 1);
    println!();
    println!("{name} {}", 2);
}
