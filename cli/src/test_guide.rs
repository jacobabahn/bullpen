use super::*;

/// (name, old, new, opcodes without autojunk, unified diff), from Python's difflib.
type Case = (
    &'static str,
    &'static str,
    &'static str,
    &'static [Op],
    &'static [&'static str],
);

const CASES: &[Case] = &[
    ("insert", "a\nb\nc", "a\nb\nx\nc", &[("equal", 0, 2, 0, 2), ("insert", 2, 2, 2, 3), ("equal", 2, 3, 3, 4)], &["--- guide (yours)", "+++ guide (now)", "@@ -1,3 +1,4 @@", " a", " b", "+x", " c"]),
    ("delete", "a\nb\nc\nd", "a\nd", &[("equal", 0, 1, 0, 1), ("delete", 1, 3, 1, 1), ("equal", 3, 4, 1, 2)], &["--- guide (yours)", "+++ guide (now)", "@@ -1,4 +1,2 @@", " a", "-b", "-c", " d"]),
    ("replace", "a\nb\nc\nd\ne", "a\nB\nC\nd\ne", &[("equal", 0, 1, 0, 1), ("replace", 1, 3, 1, 3), ("equal", 3, 5, 3, 5)], &["--- guide (yours)", "+++ guide (now)", "@@ -1,5 +1,5 @@", " a", "-b", "-c", "+B", "+C", " d", " e"]),
    ("repeated", "x\ny\nx\ny\nx", "y\nx\ny\nx\ny\nz", &[("insert", 0, 0, 0, 1), ("equal", 0, 4, 1, 5), ("replace", 4, 5, 5, 6)], &["--- guide (yours)", "+++ guide (now)", "@@ -1,5 +1,6 @@", "+y", " x", " y", " x", " y", "-x", "+z"]),
    ("empty_old", "", "one\ntwo", &[("insert", 0, 0, 0, 2)], &["--- guide (yours)", "+++ guide (now)", "@@ -0,0 +1,2 @@", "+one", "+two"]),
    ("empty_new", "one\ntwo", "", &[("delete", 0, 2, 0, 0)], &["--- guide (yours)", "+++ guide (now)", "@@ -1,2 +0,0 @@", "-one", "-two"]),
    ("both_empty", "", "", &[], &[]),
    ("same", "a\nb", "a\nb", &[("equal", 0, 2, 0, 2)], &[]),
    ("one_line_each", "a", "b", &[("replace", 0, 1, 0, 1)], &["--- guide (yours)", "+++ guide (now)", "@@ -1 +1 @@", "-a", "+b"]),
    ("delete_at_end", "a\nb\nc\nd\ne\nf\ng", "a\nb\nc\nd\ne", &[("equal", 0, 5, 0, 5), ("delete", 5, 7, 5, 5)], &["--- guide (yours)", "+++ guide (now)", "@@ -3,5 +3,3 @@", " c", " d", " e", "-f", "-g"]),
    ("several_hunks", "l0\nl1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nl20\nl21\nl22\nl23\nl24\nl25\nl26\nl27\nl28\nl29\nl30\nl31\nl32\nl33\nl34\nl35\nl36\nl37\nl38\nl39", "l0\nl1\nL2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nL20\nl21\nl22\nl23\nl24\nl25\nl26\nl27\nl28\nl29\nl31\nl32\nl33\nl34\nl35\nl36\nL37\nl38\nl39", &[("equal", 0, 2, 0, 2), ("replace", 2, 3, 2, 3), ("equal", 3, 20, 3, 20), ("replace", 20, 21, 20, 21), ("equal", 21, 30, 21, 30), ("delete", 30, 31, 30, 30), ("equal", 31, 37, 30, 36), ("replace", 37, 38, 36, 37), ("equal", 38, 40, 37, 39)], &["--- guide (yours)", "+++ guide (now)", "@@ -1,6 +1,6 @@", " l0", " l1", "-l2", "+L2", " l3", " l4", " l5", "@@ -18,7 +18,7 @@", " l17", " l18", " l19", "-l20", "+L20", " l21", " l22", " l23", "@@ -28,13 +28,12 @@", " l27", " l28", " l29", "-l30", " l31", " l32", " l33", " l34", " l35", " l36", "-l37", "+L37", " l38", " l39"]),
    ("near_hunks", "l0\nl1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19", "l0\nl1\nl2\nl3\nl4\nL5\nl6\nl7\nl8\nl9\nl10\nL11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19", &[("equal", 0, 5, 0, 5), ("replace", 5, 6, 5, 6), ("equal", 6, 11, 6, 11), ("replace", 11, 12, 11, 12), ("equal", 12, 20, 12, 20)], &["--- guide (yours)", "+++ guide (now)", "@@ -3,13 +3,13 @@", " l2", " l3", " l4", "-l5", "+L5", " l6", " l7", " l8", " l9", " l10", "-l11", "+L11", " l12", " l13", " l14"]),
];
const POPULAR: &[Case] = &[
    ("popular 0", "...xHx..X.Z.6MK...x.96IQE..xYD5xDH.x3..Rx6.xRPxxFYx.x.WM.xx..XL..Z....2.U.40Rx7H.x.X.9x0HKHZV.WJx..0.P.xxIxP.YIYR6.Tx.x.U.Ix.xC...7..x..x.x..xxxD8...x3x..x.MMxSBB.x8.UTx..3V...U.7x..M7xK1.x...x.V3xA...2x2DKxP44HY.OxOC.C.", "...xHx..X.Z.6MK...x.96IQE..xYD5xDH.x3..Rx6.xRPxxFYx.x.WM.xx..XL..Z....2.U.40Rx7H.x.X.9x0HKHZV.WJx..0.P.xxIxP.YIYR6.Tx.x.U.Ix.xC...7..x..x.x..xxD8...x3x..x.MMxSBB.x8.UTx..3V...U.7x..M7xK1.x...x.V3xA...2x2DKxP44HY.OxOC.C.", &[("equal", 0, 141, 0, 141), ("delete", 141, 142, 141, 141), ("equal", 142, 220, 141, 219)], &["--- guide (yours)", "+++ guide (now)", "@@ -139,7 +139,6 @@", " x", " .", " .", "-x", " x", " x", " D"]),
    ("popular 1", "..GxxxYA..x8.Sxx.3xNHF.FG..x.A.xN.NFDBI1D.x.B..4U.x..V.F...x.xx..7FN.1LKSP..JETx...6R2..xx3...V.EKTL..xx..IWx.Q.K.B.YP4.x.......1......Y..xN..I.GxE..x64C.IJ.XLBx.S.TMZ...x.N....xXPA.4x....I..x0xJR0.GS.0.6.xx7", "..GxxxYA..x8.Sxx.3xNHF.FG..x.A.xN.NFDBI1D.x.B..4U.x..V.F...x.xx.7FN.1LKSP..JETx...6R2..xx3...V.EKTL..xx..IWx.Q..K.B.YP4.x.......1......Y..xN..I.GxE..x64C.IJ.XLBx.S.TMZ...x.N....xXPA.4x....I..x0xJR0.GS.0.6.xx7", &[("equal", 0, 63, 0, 63), ("delete", 63, 64, 63, 63), ("equal", 64, 112, 63, 111), ("insert", 112, 112, 111, 112), ("equal", 112, 208, 112, 208)], &["--- guide (yours)", "+++ guide (now)", "@@ -61,7 +61,6 @@", " .", " x", " x", "-.", " .", " 7", " F", "@@ -110,6 +109,7 @@", " .", " Q", " .", "+.", " K", " .", " B"]),
];

#[test]
fn test_opcodes_and_unified_diff_are_difflibs() {
    for &(name, old, new, ops, diff) in CASES {
        let (a, b) = (splitlines(old), splitlines(new));
        assert_eq!(Matcher::new(&a, &b, false).opcodes(), ops, "{name}");
        assert_eq!(unified_diff(&a, &b, "guide (yours)", "guide (now)", 3), diff, "{name}");
    }
}

#[test]
fn test_unified_diff_junks_popular_lines_in_a_long_new_file() {
    // each character a line: '.' and 'x' are in more than 1% of 200+ lines, so start no match
    for &(name, old, new, ops, diff) in POPULAR {
        let a: Vec<String> = old.chars().map(String::from).collect();
        let b: Vec<String> = new.chars().map(String::from).collect();
        let (a, b): (Vec<&str>, Vec<&str>) = (
            a.iter().map(|l| l.as_str()).collect(),
            b.iter().map(|l| l.as_str()).collect(),
        );
        assert_eq!(Matcher::new(&a, &b, true).opcodes(), ops, "{name}");
        assert_ne!(Matcher::new(&a, &b, false).opcodes(), ops, "{name}");
        assert_eq!(unified_diff(&a, &b, "guide (yours)", "guide (now)", 3), diff, "{name}");
    }
}

#[test]
fn test_unified_ranges() {
    assert_eq!(unified_range(0, 0), "0,0");
    assert_eq!(unified_range(4, 4), "4,0");
    assert_eq!(unified_range(4, 5), "5");
    assert_eq!(unified_range(4, 7), "5,3");
}

const G: &str = "intro\n# Setup\nline a\nline b\n## News\nnews 1\nnews 2\n### Review  \nrev 1\nrev 2\n";

#[test]
fn test_guide_sections_names_those_around_changes_and_those_gone() {
    let cases: &[(String, String, &[&str])] = &[
        (G.into(), G.replacen("intro", "intro 2", 1), &["the top"]),
        (G.into(), G.replace("news 2", "news two\nnews three"), &["News"]),
        (
            G.into(),
            G.replace("## News\nnews 1\nnews 2\n", ""),
            &["Review", "News"],
        ),
        (G.into(), format!("{G}rev 3\n"), &["Review"]),
        (G.into(), G.replace("rev 2\n", ""), &["Review"]),
        (G.into(), String::new(), &["Setup", "News", "Review"]),
        (String::new(), G.into(), &["the top", "Setup", "News", "Review"]),
        (
            G.into(),
            G.replace("line a", "LINE A").replace("rev 1", "REV 1"),
            &["Setup", "Review"],
        ),
        (G.into(), G.into(), &[]),
    ];
    for (old, new, want) in cases {
        assert_eq!(guide_sections(old, new), *want, "{new:?}");
    }
}

#[test]
fn test_guide_news_short_long_and_unknown() {
    let (line, more) = guide_news(Some(G), &G.replace("news 2", "news two\nnews three")).unwrap();
    assert_eq!(
        line,
        "Your coordinator guide changed (section: News). Read the change below and follow it from now on:"
    );
    let want = [
        "--- guide (yours)",
        "+++ guide (now)",
        "@@ -4,7 +4,8 @@",
        " line b",
        " ## News",
        " news 1",
        "-news 2",
        "+news two",
        "+news three",
        " ### Review  ",
        " rev 1",
        " rev 2",
    ];
    assert_eq!(more, want);
    let many: Vec<String> = (0..40).map(|i| i.to_string()).collect();
    let (line, more) = guide_news(Some(G), &G.replace("news 1", &many.join("\n"))).unwrap();
    assert_eq!(
        line,
        "Your coordinator guide changed (section: News), 50 lines of diff: run `bullpen guide` to read it all, \
         and follow it from now on."
    );
    assert!(more.is_empty());
    let (line, more) = guide_news(None, G).unwrap();
    assert_eq!(
        line,
        "Your coordinator guide may have changed since you were given it: run `bullpen guide` to read it all, \
         and follow it from now on."
    );
    assert!(more.is_empty());
}

/// A guide with sections that come and go with a repo's settings.
const S: &str =
    "intro\n\n## News\nnews 1\n\n## Auto-accept\nauto 1\nauto 2\n\n## Pickup\npick 1\n\n## Reporting\nrep 1\n";

#[test]
fn test_a_section_that_came_is_told_whole_and_one_that_only_went_is_not() {
    let without = |h: &str, body: &str| S.replace(&format!("## {h}\n{body}\n\n"), "");
    let (no_auto, no_pickup) = (without("Auto-accept", "auto 1\nauto 2"), without("Pickup", "pick 1"));
    let neither = no_auto.replace("## Pickup\npick 1\n\n", "");
    let (line, more) = guide_news(Some(&no_auto), S).unwrap();
    assert_eq!(
        line,
        "Your coordinator guide changed with this repo's settings: it has the section Auto-accept now. Read it \
         below and follow it from now on:"
    );
    assert_eq!(more, ["## Auto-accept", "auto 1", "auto 2"]);
    let (line, more) = guide_news(Some(&neither), S).unwrap();
    assert_eq!(
        line,
        "Your coordinator guide changed with this repo's settings: it has the sections Auto-accept and Pickup now. \
         Read them below and follow it from now on:"
    );
    assert_eq!(more, ["## Auto-accept", "auto 1", "auto 2", "", "## Pickup", "pick 1"]);
    let (line, more) = guide_news(Some(&no_auto), &no_pickup).unwrap();
    assert_eq!(
        line,
        "Your coordinator guide changed with this repo's settings: it has the section Auto-accept now, and its \
         section Pickup no longer applies. Read it below and follow it from now on:"
    );
    assert_eq!(more, ["## Auto-accept", "auto 1", "auto 2"]);
    assert_eq!(guide_news(Some(S), &no_auto), None); // only went: following it does no harm
    assert_eq!(guide_news(Some(S), &neither), None);
    // a section no setting brings (a merge added it, or took it out): the diff, as for any change
    let merged = S.replace("## Reporting\n", "## Tours\ntour 1\n\n## Reporting\n");
    for (old, new) in [(S, merged.as_str()), (merged.as_str(), S)] {
        let (line, more) = guide_news(Some(old), new).unwrap();
        assert!(
            line.starts_with("Your coordinator guide changed (section") && line.contains("Tours"),
            "{line}"
        );
        assert!(!line.contains("settings"), "{line}");
        assert_eq!(more[..2], ["--- guide (yours)", "+++ guide (now)"]);
    }
    // anything else changed too: the diff, as for any change
    let (line, _) = guide_news(Some(&no_auto), &S.replace("rep 1", "rep one")).unwrap();
    assert_eq!(
        line,
        "Your coordinator guide changed (sections: Auto-accept, Reporting). Read the change below and follow it from \
         now on:"
    );
}

#[test]
fn test_a_file_that_isnt_utf8_is_pythons_decode_error() {
    let err = |b: &[u8]| decode(b.to_vec()).unwrap_err().0;
    assert_eq!(
        err(b"ab\xffc"),
        "'utf-8' codec can't decode byte 0xff in position 2: invalid start byte"
    );
    assert_eq!(
        err(b"a\xe2\x28\xa1"),
        "'utf-8' codec can't decode byte 0xe2 in position 1: invalid continuation byte"
    );
    assert_eq!(
        err(b"ab\xe2\x82"),
        "'utf-8' codec can't decode bytes in position 2-3: unexpected end of data"
    );
    assert_eq!(
        err(b"ab\xe2"),
        "'utf-8' codec can't decode byte 0xe2 in position 2: unexpected end of data"
    );
    assert_eq!(
        err(b"\xf0\x90("),
        "'utf-8' codec can't decode bytes in position 0-1: invalid continuation byte"
    );
    assert_eq!(
        err(b"\xe0\x80"),
        "'utf-8' codec can't decode byte 0xe0 in position 0: invalid continuation byte"
    );
    assert_eq!(decode(b"a\r\nb\rc\n".to_vec()).unwrap(), "a\nb\nc\n");
}
