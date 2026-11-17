use super::*;
use std::os::unix::fs::symlink;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bullpen-claude-tmp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_name_is_the_path_with_each_other_character_a_dash() {
    assert_eq!(
        folder_name("/home/me/dev/bullpen-bullpen/BP-12"),
        "-home-me-dev-bullpen-bullpen-BP-12"
    );
    assert_eq!(folder_name("/home/me/.cache/x_y"), "-home-me--cache-x-y");
}

#[test]
fn forget_removes_the_exact_folder_and_leaves_a_near_miss_and_a_link() {
    let base = temp("forget");
    let root = base.join("claude-1");
    let outside = base.join("outside");
    std::fs::create_dir_all(root.join("-wt-BP-1").join("session")).unwrap();
    std::fs::write(root.join("-wt-BP-1").join("session").join("f"), "x").unwrap();
    std::fs::create_dir_all(root.join("-wt-BP-12")).unwrap(); // a near miss
    std::fs::create_dir_all(outside.join("keep")).unwrap();
    symlink(&outside, root.join("-wt-BP-2")).unwrap(); // a link out of the root
    symlink(&outside, root.join("-wt-BP-1").join("link")).unwrap(); // a link inside the folder
    let names = ["-wt-BP-1", "-wt-BP-2", "../outside", ""].map(String::from);
    let gone = forget_in(&[root.clone()], &names);
    assert_eq!(gone, vec![root.join("-wt-BP-1")]);
    assert!(!root.join("-wt-BP-1").exists());
    assert!(root.join("-wt-BP-12").is_dir());
    assert!(root
        .join("-wt-BP-2")
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(outside.join("keep").is_dir()); // nothing followed out
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_root_that_is_a_link_is_never_entered() {
    let base = temp("linked-root");
    let real = base.join("real");
    std::fs::create_dir_all(real.join("-wt-BP-1")).unwrap();
    symlink(&real, base.join("claude-1")).unwrap();
    assert!(forget_in(&[base.join("claude-1")], &["-wt-BP-1".into()]).is_empty());
    assert!(real.join("-wt-BP-1").is_dir());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn sizes_as_people_read_them() {
    assert_eq!(human(0), "0 bytes");
    assert_eq!(human(4096), "4.1 KB");
    assert_eq!(human(120_000_000), "120 MB");
    assert_eq!(human(9_800_000_000), "9.8 GB");
}

#[test]
fn fullness_reads_the_filesystem() {
    let (pct, _) = fullness(Path::new("/")).unwrap();
    assert!(pct <= 100);
    assert!(fullness(Path::new("/no/such/folder")).is_none());
}
