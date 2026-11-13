use super::*;

#[test]
fn widths_as_python() {
    assert_eq!(width("幅の広い"), 8);
    assert_eq!(width("ünïcödé"), 7);
    assert_eq!(width("e\u{301}"), 1);
}

#[test]
fn splitlines_as_python() {
    assert_eq!(
        splitlines("a\r\nb\rc\n\nd\u{2028}e\n"),
        vec!["a", "b", "c", "", "d", "e"]
    );
    assert_eq!(splitlines(""), Vec::<&str>::new());
}

#[test]
fn textwrap_as_python() {
    assert_eq!(textwrap("one two three four", 9), vec!["one two", "three", "four"]);
    assert_eq!(textwrap("abcdefghijkl xy", 5), vec!["abcde", "fghij", "kl xy"]);
    assert_eq!(textwrap("  lead\tand  more", 6), vec!["  lead", "and", "more"]);
    assert!(textwrap("   ", 4).is_empty());
}

#[test]
fn textwrap_breaks_long_words_and_folds_whitespace() {
    assert_eq!(
        textwrap("a-very-long-hyphenated-word here", 10),
        vec!["a-very-lon", "g-hyphenat", "ed-word", "here"]
    );
    assert_eq!(textwrap("x   y", 2), vec!["x", "y"]);
    assert_eq!(textwrap("word\n\nnext  line", 20), vec!["word  next  line"]);
}

#[test]
fn fit_cuts_by_columns_with_an_ellipsis() {
    assert_eq!(fit("abc", 3), "abc");
    assert_eq!(fit("abcdef", 3), "ab…");
    assert_eq!(fit("幅の広い", 5), "幅の…");
    assert_eq!(fit("abc", 0), "");
    assert_eq!(wrap("one two three four", 9, 2), vec!["one two", "three …"]);
    assert_eq!(wrap("", 9, 2), vec![""]);
}

#[test]
fn strip_and_split_use_pythons_whitespace() {
    assert_eq!(strip("\x1c a\u{3000}\u{85} "), "a");
    assert_eq!(split("\x1fa b\u{2028}c"), vec!["a", "b", "c"]);
    assert_eq!(squash("  a \n\t b "), "a b");
    assert_eq!(lstrip(" a "), "a ");
    assert_eq!(rstrip(" a "), " a");
    assert_eq!(upper_first("éa"), "Éa");
    assert_eq!(ljust("é", 3), "é  ");
    assert_eq!(rjust("é", 3), "  é");
}

#[test]
fn quote_is_shlexs() {
    assert_eq!(quote(""), "''");
    assert_eq!(quote("a-b/c.d_e@f%g+h=i:j,k"), "a-b/c.d_e@f%g+h=i:j,k");
    assert_eq!(quote("it's here"), "'it'\"'\"'s here'");
    assert_eq!(quote("é"), "'é'");
    assert_eq!(shell_join(&["env", "A=b c", "x"]), "env 'A=b c' x");
}
