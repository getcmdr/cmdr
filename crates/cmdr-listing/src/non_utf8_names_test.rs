//! ❗ Today's wrong behavior (#1236), pinned so the lossless-name change flips
//! it on purpose: the listing protocols address rows BY NAME, and a local
//! listing on Linux gives two files whose names differ only in their invalid
//! bytes (`caf\xe9.txt`, `caf\xe8.txt`) one row name, `caf\u{FFFD}.txt`, and one
//! row path (`cmdr-local`'s `reading_parity_test` pins that row shape). So the
//! name can't say which twin a pane meant: a resort moves the selection and the
//! cursor to the other twin, and a restored cursor lands on whichever comes
//! first. The rows here are that shape, built in memory, so the cells run on
//! every OS.

use cmdr_fs::entry::FileEntry;
use cmdr_sort::{DirectorySortMode, SortColumn, SortOrder};

use super::testing::{TestEngine, TestListing, TestListingGuard};

const DIR: &str = "/test/non-utf8";
/// The one name both twins list under.
const LOSSY: &str = "caf\u{FFFD}.txt";

/// A file row with `size` bytes, every date set to `date`.
fn file(name: &str, size: u64, date: u64) -> FileEntry {
    FileEntry {
        size: Some(size),
        modified_at: Some(date),
        created_at: Some(date),
        added_at: Some(date),
        permissions: 0o644,
        ..FileEntry::new(name.to_string(), format!("{DIR}/{name}"), false, false)
    }
}

/// The twins (sizes 1 and 22) between two UTF-8 neighbors.
fn rows() -> Vec<FileEntry> {
    vec![
        file("cafe.txt", 5, 300),
        file(LOSSY, 1, 100),
        file(LOSSY, 22, 200),
        file("cafz.txt", 4, 400),
    ]
}

fn pane(tag: &str) -> TestListingGuard {
    TestListing::new()
        .path(DIR)
        .entries(rows())
        .insert(&TestEngine::new(), tag)
}

/// Resorts `listing` with the cursor on `cursor` and `selected` rows marked,
/// returning the new cursor row and selection.
fn resort(
    listing: &TestListingGuard,
    by: SortColumn,
    order: SortOrder,
    cursor: Option<&str>,
    selected: &[usize],
) -> (Option<usize>, Vec<usize>) {
    let result = listing
        .engine()
        .inner
        .resort_listing(
            listing.id(),
            by,
            order,
            DirectorySortMode::LikeFiles,
            cursor,
            true,
            Some(selected),
            false,
            None,
        )
        .expect("the listing is cached");
    (
        result.new_cursor_index,
        result.new_selected_indices.expect("a selection was passed"),
    )
}

/// `(name, size)` per row, in row order.
fn shown(listing: &TestListingGuard) -> Vec<(String, Option<u64>)> {
    listing
        .engine()
        .inner
        .get_file_range(listing.id(), super::RowSpan { start: 0, count: 100 }, true)
        .expect("the listing is cached")
        .into_iter()
        .map(|entry| (entry.name, entry.size))
        .collect()
}

fn size_at(listing: &TestListingGuard, row: usize) -> Option<u64> {
    shown(listing)[row].1
}

fn row_of_size(listing: &TestListingGuard, size: u64) -> usize {
    shown(listing)
        .iter()
        .position(|(_, s)| *s == Some(size))
        .expect("a row of that size")
}

/// ❗ A resort re-maps the selection by name, and the last row holding a name
/// wins, so with the 22-byte twin selected the selection lands on the 1-byte
/// one: a Delete after a sort acts on a file the person didn't pick. The cursor
/// re-maps by name too and lands on the FIRST twin, whichever it was on.
#[test]
fn a_resort_moves_the_selection_and_the_cursor_to_the_other_twin() {
    let listing = pane("non-utf8-resort");
    resort(&listing, SortColumn::Name, SortOrder::Ascending, None, &[]);
    let picked = row_of_size(&listing, 22);
    assert_eq!(
        size_at(&listing, picked),
        Some(22),
        "the person picked the 22-byte twin"
    );

    let (cursor, selected) = resort(
        &listing,
        SortColumn::Size,
        SortOrder::Descending,
        Some(LOSSY),
        &[picked],
    );

    assert_eq!(selected.len(), 1, "one row stays selected");
    assert_eq!(
        size_at(&listing, selected[0]),
        Some(1),
        "the selection moved to the 1-byte twin"
    );
    assert_eq!(cursor, Some(0), "the cursor lands on the first twin");
    assert_eq!(size_at(&listing, 0), Some(22));
}

/// ❗ A cursor restored by name (`cursorFilename` in a tab, a reveal, MCP) can't
/// say which twin it was on: it lands on the first one in the current order.
#[test]
fn a_restored_cursor_lands_on_the_first_twin() {
    let listing = pane("non-utf8-cursor");
    for (order, first_twin_size) in [(SortOrder::Ascending, 1), (SortOrder::Descending, 22)] {
        resort(&listing, SortColumn::Size, order, None, &[]);
        let row = listing
            .engine()
            .inner
            .find_file_index(listing.id(), LOSSY, true)
            .expect("the listing is cached")
            .expect("the name is listed");
        assert_eq!(size_at(&listing, row), Some(first_twin_size), "{order:?}");
        assert_eq!(row, row_of_size(&listing, first_twin_size), "{order:?}");
    }
}

/// Where the twins sort under every column, both ways: by their shared name
/// (after both UTF-8 neighbors, a tie between them kept in listing order) and
/// otherwise by their own size or dates. The lossless spelling changes only
/// the Name and Extension rows, and only between the twins.
#[test]
fn the_twins_sort_by_their_shared_lossy_name_and_their_own_size_and_dates() {
    let listing = pane("non-utf8-sort");
    let order = |by, order| {
        resort(&listing, by, order, None, &[]);
        shown(&listing)
            .into_iter()
            .map(|(name, size)| format!("{name}:{}", size.expect("a file size")))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let (asc, desc) = (SortOrder::Ascending, SortOrder::Descending);
    let by_name_asc = format!("cafe.txt:5 cafz.txt:4 {LOSSY}:1 {LOSSY}:22");
    let by_name_desc = format!("{LOSSY}:1 {LOSSY}:22 cafz.txt:4 cafe.txt:5");
    let by_date_asc = format!("{LOSSY}:1 {LOSSY}:22 cafe.txt:5 cafz.txt:4");
    let by_date_desc = format!("cafz.txt:4 cafe.txt:5 {LOSSY}:22 {LOSSY}:1");
    assert_eq!(order(SortColumn::Name, asc), by_name_asc);
    assert_eq!(order(SortColumn::Name, desc), by_name_desc);
    assert_eq!(order(SortColumn::Extension, asc), by_name_asc);
    assert_eq!(order(SortColumn::Extension, desc), by_name_desc);
    assert_eq!(
        order(SortColumn::Size, asc),
        format!("{LOSSY}:1 cafz.txt:4 cafe.txt:5 {LOSSY}:22")
    );
    assert_eq!(
        order(SortColumn::Size, desc),
        format!("{LOSSY}:22 cafe.txt:5 cafz.txt:4 {LOSSY}:1")
    );
    for by in [SortColumn::Modified, SortColumn::Created, SortColumn::Added] {
        assert_eq!(order(by, asc), by_date_asc, "{by:?}");
        assert_eq!(order(by, desc), by_date_desc, "{by:?}");
    }
}
