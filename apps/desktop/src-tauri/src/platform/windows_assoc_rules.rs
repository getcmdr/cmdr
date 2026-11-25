//! The pure rules behind Windows' "Open with" list (`windows/assoc.rs`): which type key a file's
//! extension asks about, and which listed handler is the type's default. Pure text, so the tests
//! run on every OS.

/// The Shell's type key for an extension given without its dot (`txt` → `.txt`), or `None` for
/// no extension at all, or text that isn't one (a separator, a NUL): there's nothing to ask.
pub(crate) fn assoc_extension(extension: &str) -> Option<String> {
    let clean = !extension.is_empty() && !extension.contains(['.', '\\', '/', '\0']);
    clean.then(|| format!(".{extension}"))
}

/// Which of the listed handlers, each `(id, name)`, is the type's default: the one whose id is the
/// default's program (`ASSOCSTR_EXECUTABLE`, a classic app's id is its program's path), else the
/// one whose name is the default's (`ASSOCSTR_FRIENDLYAPPNAME`: a packaged app's id is no path).
/// Both compare ignoring case, as Windows paths and names do. `None` when neither matches.
pub(crate) fn default_index(
    handlers: &[(&str, &str)],
    executable: Option<&str>,
    friendly: Option<&str>,
) -> Option<usize> {
    let by_program =
        executable.and_then(|program| handlers.iter().position(|(id, _)| id.eq_ignore_ascii_case(program)));
    by_program.or_else(|| {
        friendly.and_then(|wanted| {
            handlers
                .iter()
                .position(|(_, name)| name.to_lowercase() == wanted.to_lowercase())
        })
    })
}

/// The types Windows runs, rather than opens, when one is opened: the default `PATHEXT` (`.com`,
/// `.exe`, `.bat`, `.cmd`, `.vbs`, `.vbe`, `.js`, `.jse`, `.wsf`, `.wsh`, `.msc`), plus PowerShell
/// scripts, installers and patches, shortcuts and their kin, screen savers, Control Panel items,
/// registry files, HTML applications, and ClickOnce apps. Lowercase, without the dot.
const RUNS_WHEN_OPENED: &[&str] = &[
    "appref-ms",
    "application",
    "bat",
    "cmd",
    "com",
    "cpl",
    "exe",
    "hta",
    "js",
    "jse",
    "lnk",
    "msc",
    "msi",
    "msp",
    "pif",
    "ps1",
    "reg",
    "scr",
    "url",
    "vbe",
    "vbs",
    "ws",
    "wsf",
    "wsh",
];

/// Whether a file with this extension (no dot, any case) runs when opened. "Choose another app"
/// isn't offered for one: its "Always use this app" would re-associate a program type.
pub(crate) fn runs_when_opened(extension: &str) -> bool {
    RUNS_WHEN_OPENED
        .iter()
        .any(|known| known.eq_ignore_ascii_case(extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_scripts_and_shortcuts_run_when_opened() {
        for extension in [
            "exe", "EXE", "bat", "cmd", "com", "msi", "lnk", "scr", "ps1", "vbs", "reg",
        ] {
            assert!(runs_when_opened(extension), "{extension}");
        }
        for extension in ["txt", "jpg", "json", "pdf", ""] {
            assert!(!runs_when_opened(extension), "{extension}");
        }
    }

    #[test]
    fn an_extension_asks_by_its_dotted_key() {
        assert_eq!(assoc_extension("txt").as_deref(), Some(".txt"));
        assert_eq!(assoc_extension("JSON").as_deref(), Some(".JSON"));
    }

    #[test]
    fn no_extension_or_a_broken_one_asks_nothing() {
        assert_eq!(assoc_extension(""), None);
        assert_eq!(assoc_extension("tar.gz"), None);
        assert_eq!(assoc_extension("a\\b"), None);
        assert_eq!(assoc_extension("x\0"), None);
    }

    const LISTED: &[(&str, &str)] = &[
        (r"C:\Program Files\Notepad++\notepad++.exe", "Notepad++"),
        ("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad"),
        (
            r"C:\Users\test_user_1\AppData\Local\Programs\Microsoft VS Code\Code.exe",
            "Visual Studio Code",
        ),
    ];

    #[test]
    fn a_classic_default_is_found_by_its_program_ignoring_case() {
        let program = r"c:\program files\notepad++\NOTEPAD++.EXE";
        assert_eq!(default_index(LISTED, Some(program), Some("Something else")), Some(0));
    }

    /// A packaged app's id is its app id, never the program `ASSOCSTR_EXECUTABLE` names, so the
    /// friendly name decides.
    #[test]
    fn a_packaged_default_is_found_by_its_name() {
        let program = r"C:\Program Files\WindowsApps\Microsoft.WindowsNotepad_11.2410.21.0_x64__8wekyb3d8bbwe\Notepad\Notepad.exe";
        assert_eq!(default_index(LISTED, Some(program), Some("notepad")), Some(1));
    }

    #[test]
    fn no_match_marks_no_default() {
        assert_eq!(default_index(LISTED, Some(r"C:\x.exe"), Some("Word")), None);
        assert_eq!(default_index(LISTED, None, None), None);
        assert_eq!(default_index(&[], Some(r"C:\x.exe"), Some("x")), None);
    }
}
