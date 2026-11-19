//! For every test program (core's, the command line's, the board's): settings and state
//! of its own, never the machine's. `isolate_tests!()` in a crate's root runs `isolate`
//! as the test program starts, before any test: the global config.json
//! ($XDG_CONFIG_HOME/bullpen) and the state folder ($BULLPEN_STATE, with every repo's
//! own config.json) are folders that don't exist, so a test reads the defaults and a
//! write to either fails. A test that wants a config makes its own (core's testenv,
//! the board's Scratch). tests/support.py sets the same two for the Python suite.

/// The config folder ($XDG_CONFIG_HOME) a test program has.
pub const CONFIG: &str = "/nonexistent/bullpen-tests/config";
/// The state folder ($BULLPEN_STATE) a test program has.
pub const STATE: &str = "/nonexistent/bullpen-tests/state";

/// Point the settings and state at the test folders. Run once, as the program starts
/// (`isolate_tests!`), while it has no other thread.
pub fn isolate() {
    std::env::set_var("XDG_CONFIG_HOME", CONFIG);
    std::env::set_var("BULLPEN_STATE", STATE);
}

/// In a crate's root: its test program runs `isolate` as it starts (a constructor, as
/// C++'s statics are run: Linux's .init_array, macOS's __mod_init_func). Nothing in a
/// build that isn't a test's.
#[macro_export]
macro_rules! isolate_tests {
    () => {
        #[cfg(test)]
        #[used]
        #[cfg_attr(target_os = "linux", link_section = ".init_array")]
        #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__mod_init_func")]
        static ISOLATE_TESTS: extern "C" fn() = {
            extern "C" fn isolate() {
                $crate::isolated::isolate()
            }
            isolate
        };
    };
}

/// For each test program's check that `isolate_tests!` ran: the settings and state it
/// reads are the test folders, and no config.json is there.
pub fn assert_isolated() {
    let config = crate::config::global_config_path();
    assert_eq!(
        config,
        std::path::Path::new(CONFIG).join("bullpen/config.json"),
        "isolate_tests!() didn't run"
    );
    assert_eq!(
        crate::paths::state(),
        std::path::PathBuf::from(STATE),
        "isolate_tests!() didn't run"
    );
    assert!(
        !config.exists() && !std::path::Path::new(STATE).exists(),
        "a test wrote to {CONFIG} or {STATE}"
    );
}

#[cfg(test)]
#[path = "tests/test_isolated.rs"]
mod tests;
