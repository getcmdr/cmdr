//! Which destinations a group may be built with.
//!
//! The executor fences an archive-changeset route against a bound transfer, which is the
//! right backstop and the wrong layer on its own: it fires AFTER the user approved, so a
//! user-started copy-into-zip would work while an approved one refused. The constraint
//! belongs here, where a refusal costs the agent a retry and costs the user nothing.

use super::super::*;
use crate::location::Location;

fn at(path: &str) -> Location {
    Location {
        volume_id: "root".to_string(),
        path: path.to_string(),
    }
}

#[test]
fn an_ordinary_folder_is_a_writable_destination() {
    assert!(WritableDestination::new(at("/Users/someone/Documents/Invoices")).is_some());
}

/// The compress target IS an archive, and that is the point of compress. Only a path that
/// continues INSIDE one is refused.
#[test]
fn the_archive_file_itself_is_a_writable_destination() {
    assert!(WritableDestination::new(at("/Users/someone/Desktop/backup.zip")).is_some());
}

#[test]
fn a_path_inside_an_archive_cannot_be_a_destination() {
    for inside in [
        "/Users/someone/Desktop/backup.zip/invoices",
        "/Users/someone/Desktop/backup.zip/nested/deeper",
        "/Users/someone/Desktop/archive.tar.gz/inside",
    ] {
        assert!(
            WritableDestination::new(at(inside)).is_none(),
            // allowed-pluralize-noun: "continues" is the verb here, not a plural noun.
            "{inside} continues inside an archive, so no group may write there"
        );
    }
}

/// A folder that merely has an archive-ish name somewhere ABOVE it is still refused, because
/// the routing splits on the first archive component whatever follows it. The check and the
/// routing must agree, or the fence fires on a group the proposal layer allowed.
#[test]
fn the_constraint_matches_where_the_routing_splits() {
    assert!(WritableDestination::new(at("/Users/someone/backup.zip/a/b/c")).is_none());
}

/// A repo's virtual `.git` trees take no writes either, and for the same reason:
/// there is nothing on disk to write to. Refusing here costs the agent a retry;
/// refusing after approval costs the user a decision they made for nothing.
#[test]
fn a_path_inside_a_repos_history_cannot_be_a_destination() {
    crate::file_system::git::wiring::set_virtual_portal_enabled(true);
    for inside in [
        "/Users/someone/code/cmdr/.git/branches",
        "/Users/someone/code/cmdr/.git/branches/main/src",
        "/Users/someone/code/cmdr/.git/stash/0",
    ] {
        assert!(
            WritableDestination::new(at(inside)).is_none(),
            "{inside} is the portal's, so no group may write there"
        );
    }

    // The real entries under `.git/` are ordinary local files and stay writable.
    assert!(WritableDestination::new(at("/Users/someone/code/cmdr/.git")).is_some());
    assert!(WritableDestination::new(at("/Users/someone/code/cmdr/.git/hooks")).is_some());
}

/// A stored destination is its app path's own text, and reads back typed: a trailing `/`
/// goes, so the folder a review names is the one the write lands in.
#[test]
fn a_destination_is_stored_as_its_app_path() {
    let destination = WritableDestination::new(at("/Users/someone/Documents/Invoices/")).expect("writable");
    assert_eq!(destination.location().path, "/Users/someone/Documents/Invoices");
    assert!(
        WritableDestination::new(at("Documents/Invoices")).is_none(),
        "a relative path names no place"
    );
    assert!(
        WritableDestination::new(at("/Users/someone/x/../Invoices")).is_none(),
        "a `..` is the MCP path door's to answer, before a group is built"
    );
}

/// A destination in this host's native spelling (`C:\Users\x` on Windows, a `Path`'s text,
/// which a rename group's parent is) reads as its app path, so its approval runs. Before,
/// every such Windows approval refused as `TargetMissing`.
#[test]
fn a_destination_in_the_hosts_native_spelling_is_its_app_path() {
    let dir = crate::test_support::TestDir::new("proposal_native_destination");
    let native = dir.join("paid");
    let app = AppPathBuf::from_native_unicode(&native).expect("a Unicode temp path");
    let destination = WritableDestination::new(at(&native.display().to_string())).expect("writable");
    assert_eq!(destination.location().path, app.as_str());
    let dot_dot = native.join("..").join("x");
    assert!(
        WritableDestination::new(at(&dot_dot.display().to_string())).is_none(),
        "a `..` stays the MCP path door's to answer, in the native spelling too"
    );
}

/// A row an older build stored with a `..` names no place without its volume's answer: it
/// reads as no destination, so its approval refuses (`TargetMissing`), never a lexical guess.
#[test]
fn a_legacy_destination_holding_a_dot_dot_reads_as_none() {
    let conn = super::migrated_conn();
    let group_id = super::group_with_ops(&conn, 1);
    conn.execute(
        "UPDATE proposals SET destination = '/Users/someone/x/../Invoices' WHERE id = ?1",
        [group_id],
    )
    .expect("an older build's row");
    let group = get_group(&conn, group_id).expect("read").expect("exists");
    assert_eq!(group.destination, None);
}
