use super::*;

#[test]
fn a_summarys_first_line_drops_a_markdown_titles_marks() {
    assert_eq!(first_line("# Recorded the demo\n\nMore."), "Recorded the demo");
    assert_eq!(first_line("  ## Notes  \nx"), "Notes");
    assert_eq!(first_line("Plain line\nsecond"), "Plain line");
    assert_eq!(first_line("#"), "");
}
