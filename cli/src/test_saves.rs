use super::*;
use std::rc::Rc;

#[test]
fn later_runs_once_per_key_in_order_after_the_command() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    for (key, what) in [("queue", "first"), ("other", "other"), ("queue", "second")] {
        let s = seen.clone();
        later(key, move || s.borrow_mut().push(what));
    }
    assert!(seen.borrow().is_empty());
    run_later();
    assert_eq!(*seen.borrow(), ["other", "second"]);
}
