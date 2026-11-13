use super::*;
use bullpen_core::Fail;
use std::cell::RefCell;

#[test]
fn a_step_that_fails_is_said_and_the_others_still_run() {
    // BP-100: the merge is done, so every step after it runs whatever another did
    let ran = RefCell::new(Vec::new());
    let mut said = Vec::new();
    let steps: Vec<Step> = vec![
        ("the accepted news", Box::new(|| Ok(ran.borrow_mut().push("news")))),
        (
            "the merge's follow-up",
            Box::new(|| Err(Fail("cannot import\n  name 'placed'".into()))),
        ),
        ("the run queue", Box::new(|| Ok(ran.borrow_mut().push("queue")))),
        (
            "telling the coordinators about guide changes",
            Box::new(|| Ok(ran.borrow_mut().push("guide"))),
        ),
        ("the keys-changed news", Box::new(|| Err(Fail(String::new())))),
    ];
    run_steps("BP-1", steps, &mut |line| {
        said.push(line.to_string());
        Ok(())
    });
    assert_eq!(*ran.borrow(), ["news", "queue", "guide"]);
    assert_eq!(
        said,
        [
            "BP-1 was accepted, but the merge's follow-up failed: cannot import name 'placed'",
            "BP-1 was accepted, but the keys-changed news failed: Fail",
        ]
    );
}

#[test]
fn news_of_a_failed_step_that_fails_too_stops_nothing() {
    let ran = RefCell::new(0);
    let steps: Vec<Step> = vec![
        ("one", Box::new(|| Err(Fail("no".into())))),
        ("two", Box::new(|| Ok(*ran.borrow_mut() += 1))),
    ];
    run_steps("BP-1", steps, &mut |_| Err(Fail("no news either".into())));
    assert_eq!(*ran.borrow(), 1);
}
