//! The trait flip's guard, on the typed values this crate holds: while bodies still do
//! `std::path` arithmetic through the bridge, `x.file_name().is_some()` on a typed `x` must not
//! compile, or it would answer the app path's way, which differs from `Path`'s on bridge values
//! (`cmdr_fs::app_path::PathArithmetic`, `docs/specs/app-path-plan.md` § "4. Flip the trait").

use super::*;
use cmdr_fs::volume::InMemoryVolume;

/// `root().file_name()` resolves to the probe trait's method, which proves the app path's own
/// `file_name` is out of reach: an inherent one would win over the probe (and its result type
/// wouldn't match), and an imported `PathArithmetic` would make the call ambiguous (E0034), so
/// either one fails this file's build.
#[test]
fn a_typed_root_has_no_path_shaped_arithmetic_in_reach() {
    struct NotTheTypedAnswer;
    trait Probe {
        fn file_name(&self) -> NotTheTypedAnswer {
            NotTheTypedAnswer
        }
    }
    impl Probe for AppPath {}
    let parent: Arc<dyn Volume> = Arc::new(InMemoryVolume::new("parent"));
    let volume = ArchiveVolume::new(
        parent,
        PathBuf::from("/x/a.zip"),
        ArchiveFormat::Zip,
        VolumeHost::detached(),
    );
    let NotTheTypedAnswer = volume.root().file_name();
}
