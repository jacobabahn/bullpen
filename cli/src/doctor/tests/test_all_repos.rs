use super::*;

fn labels_of(paths: &[&str]) -> Vec<String> {
    labels(&paths.iter().map(PathBuf::from).collect::<Vec<_>>())
}

#[test]
fn a_repo_is_its_folder_unless_another_has_the_same() {
    assert_eq!(
        labels_of(&["/h/a/shop", "/h/b/shop", "/h/proj", "broken"]),
        ["a/shop", "b/shop", "proj", "broken"]
    );
}

#[test]
fn as_many_folders_as_tell_them_apart() {
    assert_eq!(
        labels_of(&["/h/x/a/shop", "/h/y/a/shop", "/h/b/shop"]),
        ["x/a/shop", "y/a/shop", "b/shop"]
    );
    // a state folder named as a repo: the repo gets its folder above
    assert_eq!(labels_of(&["shop", "/h/a/shop"]), ["shop", "a/shop"]);
}
