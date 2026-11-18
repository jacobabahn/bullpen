use super::*;

#[test]
fn numstat_reads_each_name_as_it_is() {
    let out = "1\t0\tprompts/café \"x\".md\0-\t-\timg.png\u{0}2\t3\ta\tb\nc.rs\0";
    assert_eq!(
        numstat_of(out),
        vec![
            Changed {
                lines: Some(1),
                path: "prompts/café \"x\".md".into()
            },
            Changed {
                lines: None,
                path: "img.png".into()
            },
            Changed {
                lines: Some(5),
                path: "a\tb\nc.rs".into()
            },
        ]
    );
    assert_eq!(numstat_of(""), vec![]);
}

#[test]
fn names_are_split_at_nul() {
    assert_eq!(names_of("a b.md\0c\nd\0\0"), vec!["a b.md".to_string(), "c\nd".into()]);
    assert_eq!(names_of(""), Vec::<String>::new());
}

#[test]
fn a_name_shown_escapes_what_isnt_printable() {
    assert_eq!(shown("docs/café \"x\" \\.md"), "docs/café \"x\" \\.md");
    assert_eq!(shown("a\tb\nc\u{1b}d"), "a\\tb\\nc\\u{1b}d");
    assert_eq!(listed(&["a\nb".into(), "c d".into()]), "a\\nb, c d");
}

#[test]
fn unquote_decodes_gits_c_style_quoting() {
    assert_eq!(unquote("a/plain b/plain"), ("a/plain b/plain".to_string(), ""));
    assert_eq!(
        unquote("\"a/caf\\303\\251 \\\"q\\\"\\\\\\t\\n.md\" \"b/x\""),
        ("a/café \"q\"\\\t\n.md".to_string(), " \"b/x\"")
    );
}
