//! Every install-at-startup seam has a real caller in the file that runs it, for each OS it's
//! built for. #1481: Windows' drive watcher shipped with nothing starting it, and no lint said
//! so (a `const _: fn(..) = f;` pin counts as a use, and the Windows build `expect`s dead code).
//!
//! It reads the sources, so it runs on every OS and checks all three arms at once. What it
//! can't catch: a call that's there but never reached (an early `return` above it, a `cfg`
//! that compiles it out), a seam missing from the table below, or a call in another file. The
//! watcher's own cells (`volumes/platform/windows/watcher/tests.rs`) prove it works once started.

/// The source with comment lines dropped, so a commented-out call doesn't count.
fn code(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `(file, its source, the calls it must make)`.
fn seams() -> Vec<(&'static str, &'static str, &'static [&'static str])> {
    const WATCHER: &str = "crate::volumes::start_volume_watcher(app)";
    const LOOK: &str = "crate::system_look::observe_changes(app.clone())";
    vec![
        ("platform/startup/macos.rs", include_str!("macos.rs"), &[WATCHER, LOOK]),
        ("platform/startup/linux.rs", include_str!("linux.rs"), &[WATCHER, LOOK]),
        (
            "platform/startup/windows.rs",
            include_str!("windows.rs"),
            &[WATCHER, LOOK],
        ),
        (
            "lib.rs",
            include_str!("../../lib.rs"),
            &[
                "platform::startup::install(app.handle())",
                "crash_reporter::init(app.handle())",
            ],
        ),
        (
            "crash_reporter/mod.rs",
            include_str!("../../crash_reporter/mod.rs"),
            &["platform::install_native_crash_hooks("],
        ),
        (
            "tauri_builder.rs",
            include_str!("../../tauri_builder.rs"),
            &["crate::commands::file_system::install_drop_target(&webview)"],
        ),
        (
            "updater/mod.rs",
            include_str!("../../updater/mod.rs"),
            &["install_kind::current()"],
        ),
    ]
}

#[test]
fn every_startup_seam_has_a_caller_on_every_os() {
    for (file, source, calls) in seams() {
        let code = code(source);
        for call in calls {
            assert!(code.contains(call), "{file} no longer calls `{call}`");
        }
    }
}

/// The source filter itself: a commented-out call is no call.
#[test]
fn a_commented_out_call_doesnt_count() {
    let source = "fn install(app: &AppHandle) {\n    // crate::volumes::start_volume_watcher(app);\n}";
    assert!(!code(source).contains("crate::volumes::start_volume_watcher(app)"));
}
