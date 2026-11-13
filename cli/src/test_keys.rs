use super::*;

#[test]
fn the_config_line_is_found_however_it_is_spaced() {
    let text = "-- mine\ndofile(\"/a/lua/keys.lua\").setup{ bin = \"x\" }\ndofile ( '~/b/keys.lua' ) . setup{}\n";
    assert_eq!(keys_in_config(text), ["/a/lua/keys.lua", "~/b/keys.lua"]);
}

#[test]
fn other_files_and_other_calls_are_not_the_keys_line() {
    assert!(keys_in_config("dofile(\"/a/lua/watch.lua\").setup{}").is_empty());
    assert!(keys_in_config("dofile(\"/a/keys.lua\")").is_empty()); // no setup
    assert!(keys_in_config("dofile(\"/a/keys.lua\").start{}").is_empty());
    assert!(keys_in_config("dofile(\"/a/\"keys.lua\").setup{}").is_empty()); // a quote inside
                                                                             // a match that fails doesn't hide the next one
    assert_eq!(keys_in_config("dofile(dofile(\"/k/keys.lua\").setup)"), ["/k/keys.lua"]);
}

#[test]
fn a_tilde_is_the_home_folder() {
    let home = user_home();
    assert_eq!(expanduser("~/x/keys.lua"), PathBuf::from(format!("{home}/x/keys.lua")));
    assert_eq!(expanduser("/x/keys.lua"), PathBuf::from("/x/keys.lua"));
    assert_eq!(expanduser("~other/x"), PathBuf::from("~other/x"));
}

#[test]
fn a_path_that_isnt_there_resolves_what_is() {
    let tmp = std::env::temp_dir().canonicalize().unwrap();
    assert_eq!(resolve(&tmp.join("no-such-dir/../keys.lua")), tmp.join("keys.lua"));
    assert_eq!(resolve(&tmp.join("nothing-here.lua")), tmp.join("nothing-here.lua"));
}
