//! The resolver over a made-up host: a link table instead of a disk, so every cell of
//! `docs/specs/app-path-plan.md` § "Typed and pasted `..`" runs on every OS. The real
//! disks' answers are `cmdr-local`'s and `cmdr-sftp`'s cells, and the app's go-to cells.

use std::collections::HashMap;
use std::sync::Mutex;

use super::*;
use crate::ignore_poison::IgnorePoison;

/// `links` maps a link's app path to what `<link>/..` is on that volume; a path under
/// `/hung` never answers in time; every other `..` is lexical.
struct Fake {
    flavor: PathFlavor,
    links: HashMap<&'static str, &'static str>,
    asked: Mutex<Vec<String>>,
}

impl Fake {
    fn posix() -> Self {
        Self::new(PathFlavor::Posix)
    }

    fn new(flavor: PathFlavor) -> Self {
        Self {
            flavor,
            links: HashMap::from([("/x/link", "/real"), ("sftp://h:22/a/link", "sftp://h:22/srv")]),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock_ignore_poison().clone()
    }
}

impl TypedPathHost for Fake {
    fn flavor(&self) -> PathFlavor {
        self.flavor
    }

    fn expand_tilde(&self, text: &str) -> String {
        crate::tilde::expand_tilde_with(text, Some("/home/test_user_1"), self.flavor)
    }

    async fn parent_for_dot_dot(&self, path: &AppPath) -> Result<AppPathBuf, VolumeError> {
        self.asked.lock_ignore_poison().push(path.to_string());
        if path.as_str().starts_with("/hung") {
            return Err(VolumeError::ConnectionTimeout("the mount didn't answer".into()));
        }
        Ok(match self.links.get(path.as_str()) {
            Some(parent) => AppPathBuf::parse(parent, PathFlavor::Posix).expect("a test path"),
            None => lexical_parent(path),
        })
    }
}

async fn typed(host: &Fake, text: &str, base: Option<&str>, by: TypedBy) -> Result<TypedPath, TypedPathRefusal> {
    let base = base.map(|base| AppPathBuf::parse(base, host.flavor).expect("a test base"));
    resolve_typed_with(text, base.as_deref(), by, host).await
}

async fn person(host: &Fake, text: &str, base: Option<&str>) -> (String, bool) {
    let path = typed(host, text, base, TypedBy::Person).await.expect("resolves");
    (path.path.into_string(), path.crossed_link)
}

#[tokio::test]
async fn a_dot_dot_after_a_link_lands_beside_the_target() {
    let host = Fake::posix();
    assert_eq!(person(&host, "/x/link/../y", None).await, ("/real/y".to_string(), true));
    assert_eq!(host.asked(), ["/x/link"]);
}

#[tokio::test]
async fn a_relative_inputs_leading_dot_dots_go_up_by_text() {
    // From a pane inside `/x/link`, `../y` is `/x/y`: "up from where I am", never asked.
    let host = Fake::posix();
    assert_eq!(
        person(&host, "../y", Some("/x/link")).await,
        ("/x/y".to_string(), false)
    );
    assert_eq!(
        person(&host, "./../../a", Some("/x/link")).await,
        ("/a".to_string(), false)
    );
    assert!(host.asked().is_empty(), "asked {:?}", host.asked());
}

#[tokio::test]
async fn a_dot_dot_after_a_name_in_a_relative_input_asks() {
    let host = Fake::posix();
    assert_eq!(
        person(&host, "link/../y", Some("/x")).await,
        ("/real/y".to_string(), true)
    );
}

#[tokio::test]
async fn windows_reads_dot_dot_by_text_through_a_junction() {
    // The Windows volume keeps the lexical default: Win32 resolves `..` before the disk sees it.
    let host = Fake::new(PathFlavor::Windows);
    assert_eq!(
        person(&host, "C:/x/link/../y", None).await,
        ("C:/x/y".to_string(), false)
    );
    assert_eq!(
        person(&host, r"C:\x\link\..\y", None).await,
        ("C:/x/y".to_string(), false)
    );
}

#[tokio::test]
async fn an_sftp_dot_dot_asks_the_server() {
    let host = Fake::posix();
    assert_eq!(
        person(&host, "sftp://h:22/a/link/../b", None).await,
        ("sftp://h:22/srv/b".to_string(), true)
    );
    assert_eq!(host.asked(), ["sftp://h:22/a/link"]);
}

#[tokio::test]
async fn a_missing_prefix_is_no_link() {
    let host = Fake::posix();
    assert_eq!(person(&host, "/x/nope/../y", None).await, ("/x/y".to_string(), false));
}

#[tokio::test]
async fn above_the_root_clamps_for_a_person_and_is_refused_for_an_agent() {
    let host = Fake::posix();
    assert_eq!(person(&host, "/../x", None).await, ("/x".to_string(), false));
    assert_eq!(person(&host, "../../../x", Some("/a")).await, ("/x".to_string(), false));
    let on_the_command_line = typed(&host, "/../x", None, TypedBy::PersonViaCommandLine).await;
    assert_eq!(on_the_command_line.expect("clamps").path.as_str(), "/x");
    assert!(matches!(
        typed(&host, "/../x", None, TypedBy::Agent).await,
        Err(TypedPathRefusal::OffRoot)
    ));
    assert!(matches!(
        typed(&host, "../../x", Some("/a"), TypedBy::Agent).await,
        Err(TypedPathRefusal::OffRoot)
    ));
}

#[tokio::test]
async fn a_volume_that_cant_answer_refuses_never_guesses() {
    let host = Fake::posix();
    assert!(matches!(
        typed(&host, "/hung/share/../x", None, TypedBy::Person).await,
        Err(TypedPathRefusal::Unanswered(VolumeError::ConnectionTimeout(_)))
    ));
}

#[tokio::test]
async fn a_path_with_no_dot_dot_reads_as_it_always_did() {
    let host = Fake::posix();
    assert_eq!(person(&host, "/a/./b//c/", None).await, ("/a/b/c".to_string(), false));
    assert_eq!(person(&host, "b", Some("/a")).await, ("/a/b".to_string(), false));
    assert_eq!(person(&host, "/b", Some("/a")).await, ("/b".to_string(), false));
    assert_eq!(
        person(&host, "~/Downloads", Some("/")).await,
        ("/home/test_user_1/Downloads".to_string(), false)
    );
    assert!(host.asked().is_empty());
}

#[tokio::test]
async fn the_command_line_keeps_a_tilde_literal() {
    // The shell already expanded the ones it meant.
    let host = Fake::posix();
    let path = typed(&host, "~", Some("/a"), TypedBy::PersonViaCommandLine).await;
    assert_eq!(path.expect("resolves").path.as_str(), "/a/~");
}

#[tokio::test]
async fn nothing_typed_and_no_base_are_refused() {
    let host = Fake::posix();
    assert!(matches!(
        typed(&host, "", Some("/"), TypedBy::Person).await,
        Err(TypedPathRefusal::Empty)
    ));
    assert!(matches!(
        typed(&host, "a/b", None, TypedBy::Person).await,
        Err(TypedPathRefusal::NoBase)
    ));
}

#[tokio::test]
async fn a_person_on_windows_loses_trailing_dots_and_an_agent_keeps_them() {
    let host = Fake::new(PathFlavor::Windows);
    assert_eq!(person(&host, r"C:\x\dup.", None).await, ("C:/x/dup".to_string(), false));
    let agent = typed(&host, r"C:\x\dup.", None, TypedBy::Agent).await;
    assert_eq!(agent.expect("resolves").path.as_str(), "C:/x/dup.");
}
