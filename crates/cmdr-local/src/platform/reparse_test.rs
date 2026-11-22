use super::{ATTRIBUTE_REPARSE_POINT, ReparseKind, TAG_MOUNT_POINT, TAG_SYMLINK, holds_no_links};

#[test]
fn no_privilege_and_no_link_support_mean_the_drive_holds_no_links() {
    for code in [1314, 50, 1] {
        assert!(holds_no_links(Some(code)), "{code}");
    }
    assert!(
        !holds_no_links(Some(5)),
        "an access refusal is the folder's, not the drive's"
    );
    assert!(!holds_no_links(Some(183)), "a taken name is no answer about links");
    assert!(!holds_no_links(None));
}

const DIRECTORY: u32 = 0x10;
/// `IO_REPARSE_TAG_CLOUD_6`, the tag a OneDrive placeholder wears.
const TAG_CLOUD_6: u32 = 0x9000_601A;
/// `IO_REPARSE_TAG_APPEXECLINK`, an app execution alias under `WindowsApps`.
const TAG_APPEXECLINK: u32 = 0x8000_001B;
/// `IO_REPARSE_TAG_LX_SYMLINK`, a symlink WSL made.
const TAG_LX_SYMLINK: u32 = 0xA000_001D;
/// `IO_REPARSE_TAG_DEDUP`, a file Data Deduplication optimized.
const TAG_DEDUP: u32 = 0x8000_0013;

#[test]
fn symlinks_and_junctions_are_the_links_the_volume_reads() {
    assert_eq!(
        ReparseKind::of(ATTRIBUTE_REPARSE_POINT, TAG_SYMLINK),
        ReparseKind::Symlink
    );
    assert_eq!(
        ReparseKind::of(ATTRIBUTE_REPARSE_POINT | DIRECTORY, TAG_SYMLINK),
        ReparseKind::Symlink
    );
    assert_eq!(
        ReparseKind::of(ATTRIBUTE_REPARSE_POINT | DIRECTORY, TAG_MOUNT_POINT),
        ReparseKind::Junction
    );
}

#[test]
fn a_placeholder_an_alias_and_a_deduplicated_file_are_the_item_itself() {
    for tag in [TAG_CLOUD_6, TAG_APPEXECLINK, TAG_DEDUP] {
        assert_eq!(
            ReparseKind::of(ATTRIBUTE_REPARSE_POINT, tag),
            ReparseKind::NotALink(tag),
            "{tag:#x}"
        );
    }
}

#[test]
fn another_name_surrogate_is_a_link_nobody_here_can_read() {
    assert_eq!(
        ReparseKind::of(ATTRIBUTE_REPARSE_POINT, TAG_LX_SYMLINK),
        ReparseKind::OtherLink(TAG_LX_SYMLINK)
    );
}

#[test]
fn without_the_attribute_the_tag_says_nothing() {
    assert_eq!(ReparseKind::of(DIRECTORY, TAG_SYMLINK), ReparseKind::None);
    assert_eq!(ReparseKind::of(0, 0), ReparseKind::None);
}
