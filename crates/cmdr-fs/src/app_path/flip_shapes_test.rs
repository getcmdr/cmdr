//! Milestone 4's two codemod rules, proven here before the flip exists
//! (`docs/specs/app-path-plan.md` § "4. Flip the trait", review 3's 3a and 3b).
//!
//! - **3a, a borrowed return**: `fn root(&self) -> &AppPath` can't return
//!   `&AppPath::legacy_unchecked(&self.root)` (a temporary `Cow`, E0515). The rule retypes the
//!   field to [`BridgedPath`] and returns `.app()`; every body use of the field keeps reading
//!   `std::path` through `Deref`. Below, a trait shaped as the flipped one and four
//!   implementors shaped as today's real ones compile and answer the same paths.
//! - **3b, a `std::path` method on a typed receiver**: the flip retypes some receivers (a
//!   `let r = volume.root();`), and `r.starts_with(&pathbuf)` would then pick
//!   `AppPath::starts_with`, which answers differently on bridge values. The rule rewrites the
//!   RECEIVER to `.as_std_legacy()`, never the argument to `legacy_unchecked`. The pins hold
//!   each same-named method's answer through that rewrite to `Path`'s, and record where the
//!   typed method differs, so a rule that "lifted" the `PathBuf` side fails here.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use super::*;

/// The three borrowed returns, as the flipped trait will declare them.
trait FlippedRoots {
    fn root(&self) -> &AppPath;
    fn stands_in_for_own_disk(&self) -> Option<&AppPath> {
        None
    }
    fn local_backing_file(&self) -> Option<&AppPath> {
        None
    }
}

/// `cmdr-archive`'s shape: the root is a field, a body does `std::path` arithmetic on it,
/// and `local_backing_file` answers the same field behind a condition.
struct ArchiveShaped {
    archive_path: BridgedPath,
    parent_is_local: bool,
}

impl ArchiveShaped {
    fn new(archive_path: PathBuf, parent_is_local: bool) -> Self {
        // The codemod's constructor rewrite: `archive_path` -> `BridgedPath::new(archive_path)`.
        ArchiveShaped {
            archive_path: BridgedPath::new(archive_path),
            parent_is_local,
        }
    }

    /// A body, unchanged by the flip: it reads the field as a `Path`.
    fn inner(&self, name: &str) -> PathBuf {
        self.archive_path.join(name)
    }
}

impl FlippedRoots for ArchiveShaped {
    fn root(&self) -> &AppPath {
        self.archive_path.app()
    }
    fn local_backing_file(&self) -> Option<&AppPath> {
        self.parent_is_local.then_some(self.archive_path.app())
    }
}

/// `InMemoryVolume`'s shape: a root field plus an optional stand-in home.
struct InMemoryShaped {
    root: BridgedPath,
    own_disk_home: Option<BridgedPath>,
}

impl FlippedRoots for InMemoryShaped {
    fn root(&self) -> &AppPath {
        self.root.app()
    }
    fn stands_in_for_own_disk(&self) -> Option<&AppPath> {
        self.own_disk_home.as_ref().map(BridgedPath::app)
    }
}

/// A test volume's shape: a literal root, no field. A `'static` literal through
/// `from_normalized` is a `&'static AppPath`, production code included (`app_path!` is
/// test-only).
struct LiteralShaped;

impl FlippedRoots for LiteralShaped {
    fn root(&self) -> &AppPath {
        AppPath::from_normalized("/").expect("a canonical literal")
    }
}

/// `cmdr-sftp`'s shape: the root delegates to a helper on another struct (`RemoteRoot`), whose
/// own field becomes the `BridgedPath`; the helper returns it, and `root` takes `.app()`.
struct RemoteRootShaped {
    app: BridgedPath,
}

impl RemoteRootShaped {
    fn app_root(&self) -> &BridgedPath {
        &self.app
    }
}

struct SftpShaped {
    root: RemoteRootShaped,
}

impl FlippedRoots for SftpShaped {
    fn root(&self) -> &AppPath {
        self.root.app_root().app()
    }
}

#[test]
fn every_borrowed_return_shape_compiles_and_answers_the_same_path() {
    let archive = ArchiveShaped::new(PathBuf::from("/x/a.zip"), true);
    assert_eq!(archive.root().as_str(), "/x/a.zip");
    assert_eq!(archive.local_backing_file().map(AppPath::as_str), Some("/x/a.zip"));
    assert_eq!(archive.inner("b"), Path::new("/x/a.zip").join("b"));
    let memory = InMemoryShaped {
        root: BridgedPath::new(PathBuf::from("/")),
        own_disk_home: Some(BridgedPath::new(PathBuf::from("/Users/test_user_1"))),
    };
    assert_eq!(memory.root().as_str(), "/");
    assert_eq!(
        memory.stands_in_for_own_disk().map(AppPath::as_str),
        Some("/Users/test_user_1")
    );
    assert_eq!(LiteralShaped.root().as_str(), "/");
    let sftp = SftpShaped {
        root: RemoteRootShaped {
            app: BridgedPath::new(PathBuf::from("sftp://test_user_1@testnas.local:22/")),
        },
    };
    assert_eq!(sftp.root().as_str(), "sftp://test_user_1@testnas.local:22/");
    // Lossless: the native side is the field exactly, the app side the bridge's.
    let sloppy = BridgedPath::new(PathBuf::from("/x//y/"));
    assert_eq!(sloppy.as_path().as_os_str(), "/x//y/");
    assert_eq!(sloppy.app().as_str(), "/x//y/");
}

/// A non-UTF-8 mount point bridges both ways without loss (the `None` a borrowed
/// `legacy_unchecked_ref` would have had to answer).
#[cfg(unix)]
#[test]
fn a_non_utf8_root_bridges_losslessly() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let native = PathBuf::from(OsStr::from_bytes(b"/mnt/caf\xe9"));
    let bridged = BridgedPath::new(native.clone());
    assert_eq!(bridged.as_path(), native.as_path());
    assert_eq!(bridged.app().as_std_legacy().as_os_str().as_bytes(), b"/mnt/caf\xe9");
}

// ---- 3b: a same-named method on a typed receiver drops to `as_std_legacy()` ----

/// The rewrite the rule makes: a typed receiver, then `.as_std_legacy()`.
fn through_the_rule(receiver: &str) -> Cow<'_, Path> {
    match AppPath::legacy_unchecked(Path::new(receiver)) {
        Cow::Borrowed(typed) => typed.as_std_legacy(),
        Cow::Owned(_) => unreachable!("the pins' text is valid UTF-8"),
    }
}

/// The three answers review 3 found the lifting rewrite would change, each pinned to `Path`'s
/// (on Windows `//Users/d` is a UNC prefix to `Path` itself, so these are the Unix answers).
#[cfg(unix)]
#[test]
fn starts_with_through_the_rule_answers_what_path_answers() {
    for (receiver, ancestor) in [
        ("/a/../b/c", "/a"),
        ("photos/x", "photos"),
        ("//Users/d/Docs", "/Users"),
    ] {
        assert!(Path::new(receiver).starts_with(ancestor), "{receiver} under {ancestor}");
        assert!(
            through_the_rule(receiver).starts_with(ancestor),
            "{receiver} under {ancestor} through the rule"
        );
        // The rewrite the rule forbids: the typed method answers otherwise on every one.
        let typed = AppPath::legacy_unchecked(Path::new(receiver));
        let lifted = AppPath::legacy_unchecked(Path::new(ancestor));
        assert!(
            !typed.starts_with(&lifted),
            "{receiver}: if this flips, re-read the rule's pins"
        );
    }
}

/// Every method both `Path` and the app path name, through the rule, answers exactly `Path`'s;
/// the bridge values where the typed twin differs are listed, so the rule's reason stays true.
#[test]
fn every_same_named_method_through_the_rule_answers_what_path_answers() {
    let receivers = [
        "/a/../b/c",
        "photos/x",
        "//Users/d/Docs",
        "/a/b/..",
        "/a/.",
        "/a//b/",
        "/x/a.tar.gz",
    ];
    for receiver in receivers {
        let (std_path, ruled) = (Path::new(receiver), through_the_rule(receiver));
        assert_eq!(
            ruled.as_os_str(),
            std_path.as_os_str(),
            "{receiver}: the bridge keeps the bytes"
        );
        assert_eq!(ruled.parent(), std_path.parent(), "parent of {receiver}");
        assert_eq!(ruled.file_name(), std_path.file_name(), "file_name of {receiver}");
        assert_eq!(ruled.extension(), std_path.extension(), "extension of {receiver}");
        assert_eq!(ruled.file_stem(), std_path.file_stem(), "file_stem of {receiver}");
        assert!(ruled.components().eq(std_path.components()), "components of {receiver}");
        assert_eq!(ruled.join("z"), std_path.join("z"), "join of {receiver}");
        for other in ["/a", "photos", "/Users", "b/c", "Docs", "gz"] {
            assert_eq!(
                ruled.starts_with(other),
                std_path.starts_with(other),
                "{receiver} starts_with {other}"
            );
            assert_eq!(
                ruled.ends_with(other),
                std_path.ends_with(other),
                "{receiver} ends_with {other}"
            );
            assert_eq!(
                ruled.strip_prefix(other).ok(),
                std_path.strip_prefix(other).ok(),
                "{receiver} strip_prefix {other}"
            );
        }
    }
    // Where the typed twin answers otherwise on these same values: why the rule never lifts.
    let typed = |text: &'static str| AppPath::legacy_unchecked(Path::new(text));
    assert_ne!(
        typed("/a/b/..")
            .with_file_name(AppName::from_app_spelling("z").expect("a name"))
            .ok(),
        Some(AppPath::legacy_unchecked(&Path::new("/a/b/..").with_file_name("z")).into_owned()),
    );
    assert!(typed("/a/../b/c").strip_prefix(&typed("/a")).is_none());
    assert!(Path::new("/a/../b/c").strip_prefix("/a").is_ok());
    assert!(
        Path::new("/a/../b/c")
            .components()
            .any(|component| component == Component::ParentDir)
    );
}
