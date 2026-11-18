use super::*;

fn score(w: &str) -> Score {
    parse_score(w).unwrap()
}

#[test]
fn a_score_is_a_word_from_each_list() {
    assert_eq!(
        score(" Clear, small ,LOW, Unsure"),
        Score {
            clarity: "clear".into(),
            size: "small".into(),
            risk: "low".into(),
            worth: "unsure".into()
        }
    );
    let lists = "--score is four words, clarity,size,risk,worth: clarity clear or vague; size small, medium or large; \
                 risk low or high; worth (is it worth doing) yes, unsure or no (`--score clear,small,low,yes`)";
    for bad in [
        "",
        "clear,small",
        "clear,small,low",
        "clear,small,low,maybe",
        "clear,small,low,yes,x",
        "clear,tiny,low,yes",
        "low,small,clear,yes",
        "yes,clear,small,low",
        "clear,small,medium,yes",
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
    let small = score("clear,small,low,yes");
    let medium = score("clear,medium,low,yes");
    assert_eq!(why_it_asks(&small, "small", &yes), None);
    assert_eq!(why_it_asks(&medium, "medium", &yes), None);
    assert_eq!(why_it_asks(&small, "medium", &yes), None);
    assert!(why_it_asks(&medium, "small", &yes)
        .unwrap()
        .starts_with("issues_start is small"));
    assert!(why_it_asks(&small, "off", &yes)
        .unwrap()
        .starts_with("issues_start is off"));
    for w in ["vague,small,low,yes", "clear,small,high,yes", "clear,large,low,yes"] {
        assert!(why_it_asks(&score(w), "medium", &yes).is_some(), "{w}");
    }
    assert_eq!(why_it_asks(&small, "small", &no).as_deref(), Some("ann isn't trusted"));
}

#[test]
fn only_one_worth_doing_starts_by_itself() {
    let yes = Trust {
        yes: true,
        why: "me".into(),
    };
    for start in ["off", "small", "medium"] {
        for w in ["unsure", "no"] {
            let sc = score(&format!("clear,small,low,{w}"));
            assert!(!within(&sc, start), "{w} at {start}");
            assert!(why_it_asks(&sc, start, &yes).is_some(), "{w} at {start}");
        }
    }
    let unsure = score("clear,small,low,unsure");
    let no = score("clear,small,low,no");
    assert_eq!(
        why_it_asks(&unsure, "small", &yes).as_deref(),
        Some("the coordinator isn't sure it's worth doing")
    );
    assert_eq!(
        why_it_asks(&no, "medium", &yes).as_deref(),
        Some("the coordinator doesn't think it's worth doing")
    );
    assert_eq!(no.said(), "clear, small, low, worth doing: no");
}

#[test]
fn a_judgement_from_before_the_fourth_word_counts_as_unsure() {
    let judged = |sc: &str| {
        let mut j = Obj::new();
        j.insert("score".into(), json!(sc));
        j
    };
    assert_eq!(
        score_of(&judged("clear, small, low")),
        "clear, small, low, worth doing: unsure"
    );
    assert_eq!(
        score_of(&judged("clear, small, low, worth doing: no")),
        "clear, small, low, worth doing: no"
    );
}
