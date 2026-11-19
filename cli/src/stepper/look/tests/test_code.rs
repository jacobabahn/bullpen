use super::*;
use bullpen_draw::theme::Theme;

const PATCH: &str = "diff --git a/cli/src/a.rs b/cli/src/a.rs
index 1..2 100644
--- a/cli/src/a.rs
+++ b/cli/src/a.rs
@@ -10,3 +10,4 @@ fn main() {
     let a = 1;
-    let b = 2;
+    let b = 3;
+    let c = 4;
 }
\\ No newline at end of file
@@ -40 +41,2 @@
-x
+y
+z
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
diff --git a/old.md b/new.md
similarity index 100%
rename from old.md
rename to new.md
";

#[test]
fn a_patch_is_files_hunks_and_numbered_lines() {
    let files = parse(PATCH);
    assert_eq!(
        files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["cli/src/a.rs", "img.png", "new.md"]
    );
    let a = &files[0];
    assert_eq!((a.added, a.removed), (4, 2));
    assert_eq!(
        (a.hunks[0].from, a.hunks[0].to, a.hunks[0].within.as_str()),
        (10, 13, "fn main() {")
    );
    let nums: Vec<(Kind, u32)> = a.hunks[0].lines.iter().map(|l| (l.kind, l.n)).collect();
    assert_eq!(
        nums,
        [
            (Kind::Context, 10),
            (Kind::Removed, 11),
            (Kind::Added, 11),
            (Kind::Added, 12),
            (Kind::Context, 13)
        ]
    );
    assert_eq!(a.hunks[0].lines[1].text, "    let b = 2;");
    assert_eq!((a.hunks[1].from, a.hunks[1].to), (41, 42));
    assert!(files[1].note.as_deref().unwrap().contains("binary"));
    assert!(files[2].note.as_deref().unwrap().contains("rename"));
    assert!(parse("").is_empty());
}

#[test]
fn code_is_told_apart_by_its_language() {
    let t = tokens("a.rs", "pub fn x(s: String) -> u8 { \"hi\"; 42 } // why");
    let kind = |text: &str| t.iter().find(|(s, _)| s.contains(text)).map(|(_, k)| *k);
    assert_eq!(kind("pub"), Some(Tok::Keyword));
    assert_eq!(kind("String"), Some(Tok::Name));
    assert_eq!(kind("\"hi\""), Some(Tok::Str));
    assert_eq!(kind("42"), Some(Tok::Number));
    assert_eq!(kind("// why"), Some(Tok::Comment));
    // a lifetime isn't a string in Rust; Python's single quotes are
    assert!(tokens("a.rs", "fn f<'a>(x: &'a str)")
        .iter()
        .all(|(_, k)| *k != Tok::Str));
    assert_eq!(tokens("a.py", "x = 'yes'  # no")[1], ("'yes'".to_string(), Tok::Str));
    assert_eq!(
        tokens("docs/a.md", "## Heading"),
        [("## Heading".to_string(), Tok::Keyword)]
    );
    assert_eq!(tokens("a.weird", "if x"), [("if x".to_string(), Tok::Plain)]);
    let all: String = t.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(all, "pub fn x(s: String) -> u8 { \"hi\"; 42 } // why");
}

fn lum(c: [u8; 3]) -> f64 {
    let ch = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])
}

fn over(c: [u8; 4], bg: Rgb) -> [u8; 3] {
    let a = c[3] as f64 / 255.0;
    let m = |f: u8, b: u8| (f as f64 * a + b as f64 * (1.0 - a)).round() as u8;
    [m(c[0], bg.0), m(c[1], bg.1), m(c[2], bg.2)]
}

fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (x, y) = (lum(a) + 0.05, lum(b) + 0.05);
    x.max(y) / x.min(y)
}

#[test]
fn every_colour_of_code_reads_at_4_5_to_1_on_the_pane_and_on_its_washes() {
    for th in [Theme::default(), Theme::light()] {
        let grounds = [
            [th.bg.0, th.bg.1, th.bg.2],
            over(wash(&th, Kind::Added).unwrap(), th.bg),
            over(wash(&th, Kind::Removed).unwrap(), th.bg),
        ];
        for tok in [Tok::Plain, Tok::Keyword, Tok::Str, Tok::Comment, Tok::Number, Tok::Name] {
            for g in grounds {
                let c = over(colour(&th, tok), Rgb(g[0], g[1], g[2]));
                let r = contrast(c, g);
                assert!(r >= 4.5, "{tok:?} on {g:?} ({:?}): {r:.2}", th.bg);
            }
        }
    }
}

#[test]
fn the_code_is_as_tall_as_its_strips_hunks_and_lines() {
    let files = parse(PATCH);
    let lines: usize = files.iter().flat_map(|f| &f.hunks).map(|h| h.lines.len()).sum();
    let h = height(&files, 1.0);
    let expected = 3.0 * (STRIP + 6.0) + 2.0 * FILE_GAP + 2.0 * LINE + 2.0 * HUNK + LINE * lines as f32;
    assert_eq!(h, expected.round());
    assert_eq!(height(&files, 0.5), (expected * 0.5).round());
}
