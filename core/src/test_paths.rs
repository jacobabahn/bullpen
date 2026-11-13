use super::*;
use crate::testenv::Env;

#[test]
fn safe_keeps_only_file_name_characters() {
    assert_eq!(safe("blk/1 é:x.y-z_w"), "blk_1___x.y-z_w");
}

#[test]
fn home_is_the_checkout_with_the_python_entry_or_the_source() {
    let h = home();
    assert!(h.join("core/Cargo.toml").is_file(), "{}", h.display());
    assert_eq!(self_bin(), h.join("bin/bullpen"));
    assert_eq!(python_entry(), h.join("bin/bullpen-py"));
    assert_eq!(watch_lua(), h.join("lua/watch.lua"));
}

#[test]
fn state_and_rex_come_from_the_environment() {
    let env = Env::new();
    assert_eq!(state(), env.state());
    std::env::set_var("BULLPEN_STATE", "");
    assert_eq!(state(), env.dir.join("home/.local/state/bullpen"));
    std::env::set_var("BULLPEN_REX", "/x/rex");
    assert_eq!(rex(), "/x/rex");
    std::env::remove_var("BULLPEN_REX");
    assert_eq!(rex(), "rex");
}

#[test]
fn paths_under_home_are_written_with_a_tilde() {
    let env = Env::new();
    let h = env.dir.join("home").display().to_string();
    assert_eq!(home_path(&h), "~");
    assert_eq!(home_path(format!("{h}/dev/x")), "~/dev/x");
    assert_eq!(home_path(format!("{h}x/dev")), format!("{h}x/dev"));
    assert_eq!(home_path("/elsewhere"), "/elsewhere");
    let text = format!("{h}/a, {h}, {h}x, {h}.d, {h}-e, {h}é {h}/é");
    assert_eq!(homes(&text), format!("~/a, ~, {h}x, {h}.d, {h}-e, {h}é ~/é"));
    assert_eq!(homes(&format!("{h}{h}/a")), "~~/a");
}
