use super::*;

fn v(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|i| i.to_string()).collect()
}

#[test]
fn the_accepts_are_named_as_a_list_in_words() {
    assert_eq!(and_list(&v(&["BP-3"])), "BP-3");
    assert_eq!(and_list(&v(&["BP-3", "BP-5"])), "BP-3 and BP-5");
    assert_eq!(and_list(&v(&["BP-3", "BP-5", "BP-6"])), "BP-3, BP-5 and BP-6");
}

#[test]
fn take_returns_the_accepts_and_clears_them() {
    let mut t = Obj::new();
    t.insert(KEY.into(), json!(["BP-1", "BP-2"]));
    assert_eq!(take(&mut t), v(&["BP-1", "BP-2"]));
    assert!(t.get(KEY).is_none());
    assert!(take(&mut t).is_empty());
}
