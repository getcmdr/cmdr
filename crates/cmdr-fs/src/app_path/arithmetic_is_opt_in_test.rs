//! The app path's `Path`-shaped arithmetic is a trait, ❌ never inherent: a value a
//! conversion typed must not silently answer `x.file_name()` the typed way where
//! `Path` answered before. Without `PathArithmetic` in scope, the call doesn't
//! compile; a crate adopts the typed answers by importing it, on purpose.
//!
//! This module deliberately doesn't import `PathArithmetic`.

use super::AppPath;

/// Each call below resolves to the probe trait's method, which proves the app
/// path's own one is out of reach: an inherent method would win over the probe
/// (and its argument or result type wouldn't match), and an imported
/// `PathArithmetic` would make the call ambiguous (E0034), so either one fails
/// this file's build.
#[test]
fn path_shaped_arithmetic_is_out_of_reach_without_the_trait() {
    struct NotTheTypedAnswer;
    trait Probe {
        fn parent(&self) -> NotTheTypedAnswer {
            NotTheTypedAnswer
        }
        fn file_name(&self) -> NotTheTypedAnswer {
            NotTheTypedAnswer
        }
        fn join(&self, _name: &str) -> NotTheTypedAnswer {
            NotTheTypedAnswer
        }
        fn starts_with(&self, _ancestor: &str) -> NotTheTypedAnswer {
            NotTheTypedAnswer
        }
        fn strip_prefix(&self, _ancestor: &str) -> NotTheTypedAnswer {
            NotTheTypedAnswer
        }
    }
    impl Probe for AppPath {}
    let path = crate::app_path!("/x/a.zip");
    let NotTheTypedAnswer = path.parent();
    let NotTheTypedAnswer = path.file_name();
    let NotTheTypedAnswer = path.join("b");
    let NotTheTypedAnswer = path.starts_with("/x");
    let NotTheTypedAnswer = path.strip_prefix("/x");
}
