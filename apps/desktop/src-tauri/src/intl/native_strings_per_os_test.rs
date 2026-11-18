//! How a per-OS native string resolves, and the menu bar rows whose words differ per OS. The
//! table carries every system's text, so every seam arm's answer is checked here whichever host
//! runs the test.

use super::*;

/// Every system a native string can be read on.
pub(super) const EVERY_OS: [NativeOs; 5] = [
    NativeOs::MacOs,
    NativeOs::Windows,
    NativeOs::Linux(LinuxDesktop::Gnome),
    NativeOs::Linux(LinuxDesktop::Kde),
    NativeOs::Linux(LinuxDesktop::Other),
];

#[test]
fn each_os_reads_its_own_text_and_a_plain_value_reads_the_same_everywhere() {
    let per_os = NativeText::PerOs {
        macos: "Show in Finder",
        windows: "Show in File Explorer",
        linux_gnome: "Show in Files",
        linux_kde: "Show in Dolphin",
        other: "Show in file manager",
    };
    assert_eq!(pick(per_os, NativeOs::MacOs), "Show in Finder");
    assert_eq!(pick(per_os, NativeOs::Windows), "Show in File Explorer");
    assert_eq!(pick(per_os, NativeOs::Linux(LinuxDesktop::Gnome)), "Show in Files");
    assert_eq!(pick(per_os, NativeOs::Linux(LinuxDesktop::Kde)), "Show in Dolphin");
    assert_eq!(
        pick(per_os, NativeOs::Linux(LinuxDesktop::Other)),
        "Show in file manager"
    );
    for os in EVERY_OS {
        assert_eq!(pick(NativeText::All("File"), os), "File");
    }
}

/// The shared resolution table the frontend's resolvers pass too (`platform-select-cases.json`):
/// each (platform, desktop) is the reader it names, and that reader picks the field holding its
/// text. The generator's folding (a missing branch reads `other`) is pinned on its side.
#[test]
fn native_text_agrees_with_the_shared_platform_select_table() {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct System {
        platform: String,
        linux_desktop: Option<String>,
        reader: String,
    }
    #[derive(serde::Deserialize)]
    struct Block {
        name: String,
        reads: std::collections::HashMap<String, String>,
    }
    #[derive(serde::Deserialize)]
    struct Cases {
        systems: Vec<System>,
        blocks: Vec<Block>,
    }
    let cases: Cases = serde_json::from_str(include_str!("../../../src/lib/intl/platform-select-cases.json"))
        .expect("the shared platform-select table parses");

    let os_of = |platform: &str, desktop: Option<&str>| match platform {
        "macos" => NativeOs::MacOs,
        "windows" => NativeOs::Windows,
        "linux" => NativeOs::Linux(match desktop {
            Some("gnome") => LinuxDesktop::Gnome,
            Some("kde") => LinuxDesktop::Kde,
            _ => LinuxDesktop::Other,
        }),
        other => panic!("unknown platform {other}"),
    };
    let reader_of = |os: NativeOs| match os {
        NativeOs::MacOs => "macos",
        NativeOs::Windows => "windows",
        NativeOs::Linux(LinuxDesktop::Gnome) => "linuxGnome",
        NativeOs::Linux(LinuxDesktop::Kde) => "linuxKde",
        NativeOs::Linux(LinuxDesktop::Other) => "linux",
    };
    for system in &cases.systems {
        let os = os_of(&system.platform, system.linux_desktop.as_deref());
        assert_eq!(
            reader_of(os),
            system.reader,
            "{} on {:?}",
            system.platform,
            system.linux_desktop
        );
    }
    for block in cases.blocks {
        // `NativeText` holds `'static` text, like the generated table; a test-only leak is fine.
        let read = |reader: &str| -> &'static str { block.reads[reader].clone().leak() };
        let text = NativeText::PerOs {
            macos: read("macos"),
            windows: read("windows"),
            linux_gnome: read("linuxGnome"),
            linux_kde: read("linuxKde"),
            other: read("linux"),
        };
        for os in EVERY_OS {
            assert_eq!(pick(text, os), block.reads[reader_of(os)], "{}", block.name);
        }
    }
}

/// A mounted network share's detach item says each OS's verb: Finder's Eject, File Explorer's
/// Disconnect, the Linux file manager's Unmount (#1117). A drive's item says Eject everywhere.
#[test]
fn the_share_detach_rows_read_each_os_s_verb() {
    let rows: [(&str, [&str; 3]); 2] = [
        (
            "menu.volume.detachShare",
            ["Eject ({name})", "Disconnect ({name})", "Unmount ({name})"],
        ),
        (
            "menu.volume.detachShareBusy",
            [
                "Eject ({name}) (busy)",
                "Disconnect ({name}) (busy)",
                "Unmount ({name}) (busy)",
            ],
        ),
    ];
    for (key, [macos, windows, linux]) in rows {
        let text = lookup_text(BASE_LOCALE, key).unwrap_or_else(|| panic!("{key} is in the English table"));
        assert_eq!(pick(text, NativeOs::MacOs), macos, "{key}");
        assert_eq!(pick(text, NativeOs::Windows), windows, "{key}");
        assert_eq!(pick(text, NativeOs::Linux(LinuxDesktop::Other)), linux, "{key}");
    }
}

/// Windows labels the trash undo "Undo delete" (the F8 action's name there) and the settings row
/// "Settings" with no ellipsis; Linux, where both rows sit under Edit, reads the macOS words.
#[test]
fn the_trash_undo_and_settings_rows_read_each_os_s_words() {
    let rows: [(&str, [&str; 3]); 4] = [
        ("menu.app.settings", ["Settings…", "Settings", "Settings…"]),
        (
            "menu.edit.undoTrash",
            ["Undo move to trash", "Undo delete", "Undo move to trash"],
        ),
        (
            "menu.edit.undoTrashOne",
            [
                "Undo move to trash of “{name}”",
                "Undo delete of “{name}”",
                "Undo move to trash of “{name}”",
            ],
        ),
        (
            "menu.edit.undoTrashMany",
            [
                // allowed-pluralize-noun: drawn only for two or more (`cmdr-menu`'s `labels.rs` picks `undoTrashOne` for one)
                "Undo move to trash of {countText} items",
                // allowed-pluralize-noun: drawn only for two or more, as above
                "Undo delete of {countText} items",
                // allowed-pluralize-noun: drawn only for two or more, as above
                "Undo move to trash of {countText} items",
            ],
        ),
    ];
    for (key, [macos, windows, linux]) in rows {
        let text = lookup_text(BASE_LOCALE, key).unwrap_or_else(|| panic!("{key} is in the English table"));
        assert_eq!(pick(text, NativeOs::MacOs), macos, "{key}");
        assert_eq!(pick(text, NativeOs::Windows), windows, "{key}");
        assert_eq!(pick(text, NativeOs::Linux(LinuxDesktop::Other)), linux, "{key}");
        // This host reads its own arm's text through the same lookup the menu uses.
        assert_eq!(
            lookup(BASE_LOCALE, key),
            Some(pick(text, super::super::platform::native_text_os())),
            "{key}"
        );
    }
}

/// GNOME and KDE name the settings row and the file manager their own way (#1231): GNOME's
/// "Preferences" and Files, KDE's "Configure Cmdr…" and Dolphin. Any other desktop keeps the
/// desktop-neutral words.
#[test]
fn gnome_and_kde_read_their_own_settings_and_reveal_rows() {
    let rows: [(&str, [&str; 3]); 2] = [
        ("menu.app.settings", ["Preferences", "Configure Cmdr…", "Settings…"]),
        (
            "menu.file.showInFileManager",
            ["Show in Files", "Show in Dolphin", "Show in file manager"],
        ),
    ];
    for (key, [gnome, kde, other]) in rows {
        let text = lookup_text(BASE_LOCALE, key).unwrap_or_else(|| panic!("{key} is in the English table"));
        assert_eq!(pick(text, NativeOs::Linux(LinuxDesktop::Gnome)), gnome, "{key}");
        assert_eq!(pick(text, NativeOs::Linux(LinuxDesktop::Kde)), kde, "{key}");
        assert_eq!(pick(text, NativeOs::Linux(LinuxDesktop::Other)), other, "{key}");
    }
}
