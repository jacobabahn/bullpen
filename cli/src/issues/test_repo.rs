use super::*;

#[test]
fn a_github_urls_repository() {
    for url in [
        "git@github.com:me/proj.git",
        "https://github.com/me/proj",
        "https://github.com/me/proj.git/",
        "https://token@github.com/me/proj.git",
        "ssh://git@github.com/me/proj.git",
        "ssh://git@ssh.github.com:443/me/proj.git",
        "git://GitHub.com/me/proj\n",
    ] {
        assert_eq!(slug(url).as_deref(), Some("me/proj"), "{url}");
    }
    assert_eq!(slug("git@github.com:me/my.proj-2.git").as_deref(), Some("me/my.proj-2"));
}

#[test]
fn not_a_github_url() {
    for url in [
        "",
        "/tmp/origin.git",
        "../origin.git",
        "git@gitlab.com:me/proj.git",
        "https://example.com/me/proj",
        "https://github.com/me",
        "https://github.com/me/proj/extra",
        "git@github.com:me/-R.git",
        "git@github.com:me/pr oj.git",
    ] {
        assert_eq!(slug(url), None, "{url}");
    }
}

#[test]
fn what_dash_r_takes() {
    assert!(well_formed("me/proj") && well_formed("ghe.example.com/me/proj"));
    for bad in ["proj", "me/", "-x/proj", "me/proj;rm", "a/b/c/d", "me/pr oj"] {
        assert!(!well_formed(bad), "{bad}");
    }
}
