//! Building the arena: from a scan's recorder, from the local walk's lists, and
//! from a compress plan.

use std::path::{Path, PathBuf};

use cmdr_fs::volume::{CopyScanResult, ScanRecorder};

use super::super::model::{NodeId, NodeKind};
use super::super::test_support::{english, node_at, outline};
use super::{TreeRecorder, from_archive_plan, from_local_scan, sort_sources};

fn paths(raw: &[&str]) -> Vec<PathBuf> {
    raw.iter().map(PathBuf::from).collect()
}

#[test]
fn a_recorded_scan_builds_the_selection_in_tree_order_with_folder_totals() {
    let recorder = TreeRecorder::new(&paths(&["/src/photos", "/src/notes.txt"]), english());
    recorder.dir(Path::new("/src/photos"));
    recorder.file(Path::new("/src/photos/b.jpg"), 20);
    recorder.dir(Path::new("/src/photos/2024"));
    recorder.file(Path::new("/src/photos/2024/img_10.jpg"), 5);
    recorder.file(Path::new("/src/photos/2024/img_9.jpg"), 4);
    recorder.file(Path::new("/src/photos/a.jpg"), 10);
    recorder.file(Path::new("/src/notes.txt"), 3);

    let model = recorder.finish().expect("the tree builds");

    assert_eq!(
        outline(&model),
        vec![
            "photos/ 4 files 39 B",
            "  2024/ 2 files 9 B",
            "    img_9.jpg 4 B",
            "    img_10.jpg 5 B",
            "  a.jpg 10 B",
            "  b.jpg 20 B",
            "notes.txt 3 B",
        ]
    );
    let root = model.folder(NodeId::ROOT).expect("the root is a folder");
    assert_eq!((root.files_total, root.bytes_total), (5, 42));
}

#[test]
fn children_are_one_contiguous_id_range_in_tree_order() {
    let recorder = TreeRecorder::new(&paths(&["/src/top"]), english());
    recorder.dir(Path::new("/src/top"));
    for name in ["c", "a", "b"] {
        recorder.file(&Path::new("/src/top").join(name), 1);
    }

    let model = recorder.finish().expect("the tree builds");

    let top = node_at(&model, &["top"]);
    let ids: Vec<u32> = (0..3).map(|index| model.child_at(top, index).get()).collect();
    assert_eq!(ids[1], ids[0] + 1);
    assert_eq!(ids[2], ids[0] + 2);
    for index in 0..3 {
        assert_eq!(model.position_in_parent(model.child_at(top, index)), index);
    }
}

#[test]
fn entries_arriving_before_their_folder_still_land_under_it() {
    let recorder = TreeRecorder::new(&paths(&["/src/top"]), english());
    recorder.file(Path::new("/src/top/deep/er/x.bin"), 7);
    recorder.dir(Path::new("/src/top/deep"));
    recorder.dir(Path::new("/src/top"));
    recorder.dir(Path::new("/src/top/deep/er"));

    let model = recorder.finish().expect("the tree builds");

    assert_eq!(
        outline(&model),
        vec![
            "top/ 1 files 7 B",
            "  deep/ 1 files 7 B",
            "    er/ 1 files 7 B",
            "      x.bin 7 B"
        ]
    );
}

#[test]
fn sources_under_different_parents_hang_off_their_common_ancestor_as_relative_labels() {
    let recorder = TreeRecorder::new(&paths(&["/a/x/one.txt", "/a/y/z/two"]), english());
    recorder.file(Path::new("/a/x/one.txt"), 1);
    recorder.dir(Path::new("/a/y/z/two"));
    recorder.file(Path::new("/a/y/z/two/inner.txt"), 2);

    let model = recorder.finish().expect("the tree builds");

    assert_eq!(
        outline(&model),
        vec!["y/z/two/ 1 files 2 B", "  inner.txt 2 B", "x/one.txt 1 B"],
        "no synthesized folder rows: `y` and `z` weren't selected"
    );
}

/// ❗ Regression anchor (#523): a twin's row path carries a pick before its
/// name (`cmdr_fs::twin_path`), and `Path::parent` answered the pick, so the
/// tree hung off it and a row read `␀2/IMG_1.JPG`. The root is the real folder
/// and every label reads as the plain name.
#[test]
fn twin_sources_hang_off_their_real_folder_under_their_plain_names() {
    let twin = |id| {
        PathBuf::from("/DCIM")
            .join(cmdr_fs::twin_path::pick_segment(id))
            .join("IMG_1.JPG")
    };
    let recorder = TreeRecorder::new(&[twin(1), twin(2), PathBuf::from("/DCIM/notes.txt")], english());
    recorder.file(&twin(1), 1);
    recorder.file(&twin(2), 2);
    recorder.file(Path::new("/DCIM/notes.txt"), 3);

    let model = recorder.finish().expect("the tree builds");

    assert_eq!(outline(&model), vec!["IMG_1.JPG 1 B", "IMG_1.JPG 2 B", "notes.txt 3 B"]);
}

/// The order a driver takes its sources in is the order their rows list in:
/// folders first, then by the label each row shows, which for sources under
/// different parents is the path below their common ancestor.
#[test]
fn sources_sort_into_the_order_their_rows_list_in() {
    let mut sources = paths(&["/a/x/one.txt", "/a/y/z/two", "/a/b.txt", "/a/x/A"]);
    let folders = paths(&["/a/y/z/two", "/a/x/A"]);

    sort_sources(
        &mut sources,
        |source| std::borrow::Cow::Borrowed(source.as_path()),
        |source| folders.iter().any(|folder| folder == source),
        &english(),
    );

    assert_eq!(sources, paths(&["/a/x/A", "/a/y/z/two", "/a/b.txt", "/a/x/one.txt"]));
}

#[test]
fn a_subtree_counted_without_a_walk_is_an_opaque_folder_with_its_totals() {
    let recorder = TreeRecorder::new(&paths(&["/src/opaque", "/src/single"]), english());
    let folder = CopyScanResult {
        file_count: 12,
        dir_count: 3,
        total_bytes: 1_200,
        dedup_bytes: 1_200,
        top_level_is_directory: true,
        top_level_modified_at: None,
        top_level_created_at: None,
    };
    let file = CopyScanResult {
        file_count: 1,
        dir_count: 0,
        total_bytes: 9,
        dedup_bytes: 9,
        top_level_is_directory: false,
        top_level_modified_at: None,
        top_level_created_at: None,
    };
    recorder.subtree(Path::new("/src/opaque"), &folder);
    recorder.subtree(Path::new("/src/single"), &file);

    let model = recorder.finish().expect("the tree builds");

    assert_eq!(outline(&model), vec!["opaque/? 12 files 1200 B", "single 9 B"]);
    assert_eq!(model.kind(node_at(&model, &["opaque"])), NodeKind::Opaque);
}

#[test]
fn an_entry_outside_every_source_is_dropped() {
    let recorder = TreeRecorder::new(&paths(&["/src/top"]), english());
    recorder.dir(Path::new("/src/top"));
    recorder.file(Path::new("/elsewhere/stray.txt"), 5);
    recorder.file(Path::new("/src/sibling.txt"), 5);

    let model = recorder.finish().expect("the tree builds");

    assert_eq!(outline(&model), vec!["top/ 0 files 0 B"]);
}

#[test]
fn the_local_walk_builds_the_same_tree_and_maps_every_file_to_its_node() {
    let sources = paths(&["/src/top", "/src/loose.txt"]);
    let dirs = paths(&["/src/top", "/src/top/empty", "/src/top/sub"]);
    let files = [
        (PathBuf::from("/src/top/sub/b.txt"), 2u64),
        (PathBuf::from("/src/loose.txt"), 3),
        (PathBuf::from("/src/top/a.txt"), 1),
    ];

    let (model, nodes) = from_local_scan(
        &sources,
        &dirs,
        files.iter().map(|(path, size)| (path.as_path(), *size)),
        &english(),
    )
    .expect("the tree builds");

    assert_eq!(
        outline(&model),
        vec![
            "top/ 2 files 3 B",
            "  empty/ 0 files 0 B",
            "  sub/ 1 files 2 B",
            "    b.txt 2 B",
            "  a.txt 1 B",
            "loose.txt 3 B",
        ]
    );
    let names: Vec<&str> = nodes
        .iter()
        .map(|node| model.name(node.expect("every walked file is under a source")))
        .collect();
    assert_eq!(
        names,
        vec!["b.txt", "loose.txt", "a.txt"],
        "one node per file, in input order"
    );
}

#[test]
fn a_compress_plan_builds_from_its_root_relative_entry_names() {
    let entries = [
        ("top/", true, 0u64),
        ("top/sub/", true, 0),
        ("top/sub/b.txt", false, 2),
        ("top/a.txt", false, 1),
        ("loose.txt", false, 3),
    ];

    let (model, nodes) = from_archive_plan(entries.iter().copied(), &english()).expect("the tree builds");

    assert_eq!(
        outline(&model),
        vec![
            "top/ 2 files 3 B",
            "  sub/ 1 files 2 B",
            "    b.txt 2 B",
            "  a.txt 1 B",
            "loose.txt 3 B"
        ]
    );
    let names: Vec<&str> = nodes.iter().map(|node| model.name(*node)).collect();
    assert_eq!(names, vec!["top", "sub", "b.txt", "a.txt", "loose.txt"]);
}

/// A recorder that hits a bug stops recording and builds nothing, and the scan
/// feeding it never sees the panic.
#[test]
fn a_recorder_that_hits_a_bug_records_nothing_more_and_builds_no_tree() {
    use super::super::test_support::TREE_BUG_NAME;

    let recorder = TreeRecorder::new(&paths(&["/src/top"]), english());
    recorder.dir(Path::new("/src/top"));
    recorder.file(&Path::new("/src/top").join(TREE_BUG_NAME), 1);
    recorder.file(Path::new("/src/top/after.txt"), 1);

    assert!(recorder.finish().is_none());
}

#[test]
fn every_builder_answers_none_for_a_tree_bug() {
    use super::super::test_support::TREE_BUG_NAME;
    use super::{from_per_path, from_sources};

    let bug = PathBuf::from("/src").join(TREE_BUG_NAME);
    assert!(from_sources(std::slice::from_ref(&bug), |_| false, &english()).is_none());
    assert!(from_per_path(std::slice::from_ref(&bug), &[], &english()).is_none());
    assert!(from_local_scan(std::slice::from_ref(&bug), &[], [(bug.as_path(), 1)], &english()).is_none());
    assert!(from_archive_plan([(TREE_BUG_NAME, false, 1)], &english()).is_none());
}
