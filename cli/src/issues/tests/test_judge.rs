use super::*;

fn score(w: &str) -> Score {
    parse_score(w).unwrap()
}

#[test]
fn a_score_is_a_word_from_each_list() {
    assert_eq!(
        score(" Clear, small ,LOW"),
        Score {
            clarity: "clear".into(),
            size: "small".into(),
            risk: "low".into()
        }
    );
    let lists = "--score is three words, clarity,size,risk: clarity clear or vague; size small, medium or large; \
                 risk low or high (`--score clear,small,low`)";
    for bad in [
        "",
        "clear,small",
        "clear,small,low,x",
        "clear,tiny,low",
        "low,small,clear",
        "clear,small,medium",
    ] {
        assert_eq!(parse_score(bad), Err(lists.to_string()), "{bad}");
    }
}

#[test]
fn the_threshold() {
    let yes = Trust {
        yes: true,
        why: "me".into(),
    };
    let no = Trust {
        yes: false,
        why: "ann isn't trusted".into(),
    };
    let small = score("clear,small,low");
    let medium = score("clear,medium,low");
    assert_eq!(why_it_asks(&small, "small", &yes), None);
    assert_eq!(why_it_asks(&medium, "medium", &yes), None);
    assert_eq!(why_it_asks(&small, "medium", &yes), None);
    assert!(why_it_asks(&medium, "small", &yes)
        .unwrap()
        .starts_with("issues_start is small"));
    assert!(why_it_asks(&small, "off", &yes)
        .unwrap()
        .starts_with("issues_start is off"));
    for w in ["vague,small,low", "clear,small,high", "clear,large,low"] {
        assert!(why_it_asks(&score(w), "medium", &yes).is_some(), "{w}");
    }
    assert_eq!(why_it_asks(&small, "small", &no).as_deref(), Some("ann isn't trusted"));
}
