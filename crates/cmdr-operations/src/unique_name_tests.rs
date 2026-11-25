//! Unit tests for `unique_name.rs`, split out as a `#[path]` child so the module
//! itself stays readable. `super::` here is `unique_name`, exactly as when these
//! lived inline.

use super::*;
use tempfile::TempDir;

// ============================================================================
// find_unique_name
// ============================================================================
//
// Regression for the low-severity audit finding: pre-fix `find_unique_name`
// picked a name by exists()-checking each candidate and returning the first
// miss. Between the check and the caller's write, a concurrent process (backup
// tool, cloud-sync agent, second Cmdr op) could land a file at the same name and
// the next copy / rename would silently clobber it. The fix atomically reserves
// the chosen name via O_EXCL.

/// A POSIX volume's folder keeps its `/` on every host: on Windows `Path::join` made the
/// rescue `/dest\notes (recovered).txt` and the next pick `/dest\notes (1).txt`, names no
/// remote or in-memory volume holds. A native `C:\x` folder keeps its own `\`.
#[test]
fn a_sibling_joins_with_its_folders_own_separator() {
    assert_eq!(
        recovered_sibling(Path::new("/dest/notes.txt")).to_str(),
        Some("/dest/notes (recovered).txt")
    );
    let picks = NameCandidates::for_file(Path::new("sftp://h/srv/a.jpg"));
    assert_eq!(picks.current().to_str(), Some("sftp://h/srv/a (1).jpg"));
    assert_eq!(
        recovered_sibling(Path::new("C:\\x\\notes.txt")).to_str(),
        Some("C:\\x\\notes (recovered).txt")
    );
}

/// A volume rescue splits by the app path: a POSIX server's `a\b.txt` is one name on every
/// host, where Windows' `Path` rescued it as `b (recovered).txt` inside `/dest/a`.
#[test]
fn a_volume_rescue_keeps_a_backslash_name_whole_in_its_folder() {
    use cmdr_fs::app_path;
    for (orig, rescued) in [
        (app_path!("/dest/a\\b.txt"), "/dest/a\\b (recovered).txt"),
        (app_path!("/dest/foo\\.."), "/dest/foo\\. (recovered)."),
        (app_path!("/dest/..\\bar"), "/dest/. (recovered).\\bar"),
        (app_path!("/dest/a:b"), "/dest/a:b (recovered)"),
        (app_path!("/dest/trail."), "/dest/trail (recovered)."),
        (app_path!("/dest/C:\\x"), "/dest/C:\\x (recovered)"),
        (app_path!("/dest/notes.txt"), "/dest/notes (recovered).txt"),
    ] {
        let recovered = recovered_sibling_app(orig).expect("a rescue name");
        assert_eq!(recovered.as_str(), rescued, "{orig:?}");
        let next = AppNameCandidates::for_file(&recovered).current().expect("a pick");
        assert_eq!(next.parent(), Some(app_path!("/dest")), "{orig:?}: {next:?}");
    }
}

#[test]
fn reserves_the_chosen_name_on_disk() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("notes.txt");
    fs::write(&target, b"original").unwrap();

    let unique = find_unique_name(&target, &EngineHost::detached());

    assert_eq!(unique.file_name().unwrap().to_string_lossy(), "notes (1).txt");
    // O_EXCL placeholder must already exist after the call.
    assert!(unique.exists(), "reservation must create the placeholder");
    // Second call goes to (2), proving the first reservation persisted.
    let next = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(next.file_name().unwrap().to_string_lossy(), "notes (2).txt");
}

#[test]
fn keeps_extension_in_the_right_place() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("report.pdf");
    fs::write(&target, b"x").unwrap();
    let unique = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(unique.file_name().unwrap().to_string_lossy(), "report (1).pdf");
}

#[test]
fn handles_extensionless_filenames() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("README");
    fs::write(&target, b"x").unwrap();
    let unique = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(unique.file_name().unwrap().to_string_lossy(), "README (1)");
}

#[test]
fn continues_a_trailing_sequence_instead_of_nesting() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("photo (1).jpg");
    fs::write(&target, b"x").unwrap();
    let unique = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(unique.file_name().unwrap().to_string_lossy(), "photo (2).jpg");
}

#[test]
fn a_sequence_skips_past_every_taken_number() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("photo (1).jpg");
    fs::write(&target, b"x").unwrap();
    fs::write(temp.path().join("photo (2).jpg"), b"x").unwrap();
    fs::write(temp.path().join("photo (3).jpg"), b"x").unwrap();
    let unique = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(unique.file_name().unwrap().to_string_lossy(), "photo (4).jpg");
}

#[test]
fn a_non_numeric_parenthetical_is_not_a_sequence() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("Report (final).pdf");
    fs::write(&target, b"x").unwrap();
    let unique = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(unique.file_name().unwrap().to_string_lossy(), "Report (final) (1).pdf");
}

#[test]
fn a_number_too_big_for_u32_is_literal_text() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("photo (99999999999).jpg");
    fs::write(&target, b"x").unwrap();
    let unique = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(
        unique.file_name().unwrap().to_string_lossy(),
        "photo (99999999999) (1).jpg"
    );
}

#[test]
fn zero_padding_is_not_preserved() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("photo (007).jpg");
    fs::write(&target, b"x").unwrap();
    let unique = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(unique.file_name().unwrap().to_string_lossy(), "photo (8).jpg");
}

#[test]
fn a_zero_sequence_advances_to_one() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("photo (0).jpg");
    fs::write(&target, b"x").unwrap();
    let unique = find_unique_name(&target, &EngineHost::detached());
    assert_eq!(unique.file_name().unwrap().to_string_lossy(), "photo (1).jpg");
}

// ============================================================================
// split_sequence
// ============================================================================
//
// The pure half of the ` (N)` convention: what counts as a trailing sequence and
// what is ordinary text that happens to hold parentheses.

#[test]
fn a_bare_stem_starts_at_one() {
    assert_eq!(split_sequence("photo"), ("photo", 1));
}

#[test]
fn a_trailing_number_continues_from_itself() {
    assert_eq!(split_sequence("photo (4)"), ("photo", 5));
    assert_eq!(split_sequence("photo (0)"), ("photo", 1));
    assert_eq!(split_sequence("photo (007)"), ("photo", 8));
}

#[test]
fn only_the_last_parenthetical_counts() {
    assert_eq!(split_sequence("photo (2) (3)"), ("photo (2)", 4));
    assert_eq!(split_sequence("photo (2) (draft)"), ("photo (2) (draft)", 1));
}

#[test]
fn text_in_parentheses_is_not_a_sequence() {
    assert_eq!(split_sequence("Report (final)"), ("Report (final)", 1));
    assert_eq!(split_sequence("photo ()"), ("photo ()", 1));
    assert_eq!(split_sequence("photo (12a)"), ("photo (12a)", 1));
    // `u32::from_str` accepts a leading `+`; the convention never writes one.
    assert_eq!(split_sequence("photo (+1)"), ("photo (+1)", 1));
    // Non-ASCII digits parse in no locale we generate.
    assert_eq!(split_sequence("photo (١)"), ("photo (١)", 1));
}

#[test]
fn the_separating_space_is_required() {
    assert_eq!(split_sequence("photo(1)"), ("photo(1)", 1));
    assert_eq!(split_sequence("(1)"), ("(1)", 1));
}

#[test]
fn a_number_that_cannot_advance_is_literal_text() {
    // Doesn't fit `u32` at all.
    assert_eq!(split_sequence("photo (99999999999)"), ("photo (99999999999)", 1));
    // Fits, but has no successor, so continuing the sequence is impossible.
    let at_max = format!("photo ({})", u32::MAX);
    assert_eq!(split_sequence(&at_max), (at_max.as_str(), 1));
}

// ============================================================================
// next_available_name
// ============================================================================
//
// The non-reserving sibling of `find_unique_name`: same convention, same
// sequence rule, but it only probes and never creates. Callers that reserve the
// name themselves (a directory claiming its own name with `create_dir`) need the
// probe without the placeholder a file reservation leaves behind.

#[test]
fn picks_the_next_free_name_without_creating_anything() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("notes.txt");
    fs::write(&target, b"original").unwrap();

    let picked = next_available_name(&target, &ClaimedNames::default());

    assert_eq!(picked.file_name().unwrap().to_string_lossy(), "notes (1).txt");
    assert!(!picked.exists(), "the probe must not reserve the name");
    // Nothing was reserved on disk, so another operation answers the same.
    assert_eq!(next_available_name(&target, &ClaimedNames::default()), picked);
}

/// The ledger is what a probe can't have: nothing lands on disk between the two
/// picks, so only the record of the first keeps the second off it.
#[test]
fn a_second_pick_in_one_operation_walks_past_the_first() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("notes.txt");
    fs::write(&target, b"original").unwrap();
    let claimed = ClaimedNames::default();

    let first = next_available_name(&target, &claimed);
    let second = next_available_name(&target, &claimed);

    assert_eq!(first.file_name().unwrap().to_string_lossy(), "notes (1).txt");
    assert_eq!(second.file_name().unwrap().to_string_lossy(), "notes (2).txt");
}

/// Two sources of one ` (N)` family, the shape that made this a bug: the second
/// source's sequence starts exactly where the first source's pick landed.
#[test]
fn two_sources_of_one_family_never_pick_the_same_name() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("photo.jpg"), b"x").unwrap();
    fs::write(temp.path().join("photo (1).jpg"), b"x").unwrap();
    let claimed = ClaimedNames::default();

    let from_plain = next_available_name(&temp.path().join("photo.jpg"), &claimed);
    let from_first = next_available_name(&temp.path().join("photo (1).jpg"), &claimed);

    assert_eq!(from_plain.file_name().unwrap().to_string_lossy(), "photo (2).jpg");
    assert_eq!(from_first.file_name().unwrap().to_string_lossy(), "photo (3).jpg");
}

/// A directory claims with `mkdir(2)`, which can't see a name only spoken for,
/// so it consults the ledger as well.
#[test]
fn a_directory_claim_walks_past_a_name_a_file_pick_spoke_for() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("notes"), b"x").unwrap();
    let claimed = ClaimedNames::default();

    let spoken_for = next_available_name(&temp.path().join("notes"), &claimed);
    let created = create_unique_dir(&temp.path().join("notes"), &claimed).unwrap();

    assert_eq!(spoken_for.file_name().unwrap().to_string_lossy(), "notes (1)");
    assert_eq!(created.file_name().unwrap().to_string_lossy(), "notes (2)");
    assert!(created.is_dir(), "the directory claim creates what it returns");
}

#[test]
fn skips_names_already_taken_and_continues_a_sequence() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("photo (1).jpg");
    fs::write(&target, b"x").unwrap();
    fs::write(temp.path().join("photo (2).jpg"), b"x").unwrap();

    let picked = next_available_name(&target, &ClaimedNames::default());
    assert_eq!(picked.file_name().unwrap().to_string_lossy(), "photo (3).jpg");
}

#[test]
fn works_for_a_name_with_no_extension() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("docs");
    fs::create_dir(&target).unwrap();

    let picked = next_available_name(&target, &ClaimedNames::default());
    assert_eq!(picked.file_name().unwrap().to_string_lossy(), "docs (1)");
    assert!(!picked.exists());
}

// ============================================================================
// create_unique_dir
// ============================================================================
//
// A directory has no extension. Everything after a dot in `my.dir` or
// `backup.2024` is part of its name, so the number goes at the END.

#[test]
fn a_dot_in_a_directory_name_is_part_of_the_name() {
    let temp = TempDir::new().unwrap();
    for name in ["my.dir", "backup.2024", "v1.2.3"] {
        let target = temp.path().join(name);
        fs::create_dir(&target).unwrap();

        let created = create_unique_dir(&target, &ClaimedNames::default()).unwrap();

        assert_eq!(
            created.file_name().unwrap().to_string_lossy(),
            format!("{name} (1)"),
            "a directory numbers whole, never mid-name"
        );
    }
}

#[test]
fn a_directory_continues_its_sequence_past_the_dot() {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("my.dir")).unwrap();
    fs::create_dir(temp.path().join("my.dir (1)")).unwrap();

    let created = create_unique_dir(&temp.path().join("my.dir (1)"), &ClaimedNames::default()).unwrap();

    assert_eq!(created.file_name().unwrap().to_string_lossy(), "my.dir (2)");
}

#[test]
fn a_dangling_symlink_counts_as_taken() {
    // A link needs Developer Mode or elevation on Windows.
    if cmdr_fs::testing::skip_without_symlinks() {
        return;
    }
    // `Path::exists()` follows symlinks and reports `false` for a broken
    // one; handing that name back would let the caller's write follow the
    // symlink to wherever it points.
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("notes.txt");
    fs::write(&target, b"x").unwrap();
    cmdr_fs::testing::symlink(temp.path().join("gone"), temp.path().join("notes (1).txt")).unwrap();

    let picked = next_available_name(&target, &ClaimedNames::default());
    assert_eq!(picked.file_name().unwrap().to_string_lossy(), "notes (2).txt");
}

/// ❗ #1470: the volume namer's candidates split and join by the app path, so a POSIX
/// server's `a\b.txt` numbers beside itself on every host (Windows' `Path` split it into
/// `b (1).txt` inside `a`), and a folder keeps its dots whole.
#[test]
fn app_name_candidates_number_a_name_holding_a_backslash_beside_it() {
    let next = |path: &str, is_directory: bool| {
        let path = AppPath::from_normalized(path).expect("canonical");
        let candidates = if is_directory {
            AppNameCandidates::for_directory(path)
        } else {
            AppNameCandidates::for_file(path)
        };
        candidates.current().map(AppPathBuf::into_string)
    };
    assert_eq!(next("/inbox/a\\b.txt", false).as_deref(), Some("/inbox/a\\b (1).txt"));
    assert_eq!(
        next("/inbox/photo (1).jpg", false).as_deref(),
        Some("/inbox/photo (2).jpg")
    );
    assert_eq!(next("/inbox/my.dir", true).as_deref(), Some("/inbox/my.dir (1)"));
    assert_eq!(
        next("sftp://h/x/notes.txt", false).as_deref(),
        Some("sftp://h/x/notes (1).txt")
    );
}
