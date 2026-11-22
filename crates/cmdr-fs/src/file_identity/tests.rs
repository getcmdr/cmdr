#[cfg(unix)]
use std::fs;

use super::*;

#[test]
fn unknown_never_reads_as_different() {
    assert_eq!(Sameness::of(Some(1), Some(1)), Sameness::Same);
    assert_eq!(Sameness::of(Some(1), Some(2)), Sameness::Different);
    assert_eq!(Sameness::of(None, Some(1)), Sameness::Unknown);
    assert_eq!(Sameness::of(Some(1), None), Sameness::Unknown);
    assert_eq!(Sameness::of::<i32>(None, None), Sameness::Unknown);
    assert!(Sameness::Same.may_be_same());
    assert!(Sameness::Unknown.may_be_same(), "a guard must take the cautious branch");
    assert!(!Sameness::Different.may_be_same());
}

#[test]
fn a_node_id_too_wide_for_64_bits_has_no_64_bit_form() {
    let device = DeviceId::from_raw(7);
    assert_eq!(FileId::from_parts(device, 42).node_u64(), Some(42));
    assert_eq!(FileId::from_parts(device, u128::from(u64::MAX) + 1).node_u64(), None);
    assert_eq!(FileId::from_parts(device, 42).device(), device);
}

#[cfg(unix)]
mod on_a_real_disk {
    use super::*;

    #[test]
    fn a_hard_link_is_the_same_node_and_counts_two_names() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("a.txt");
        let link = dir.path().join("b.txt");
        fs::write(&original, b"x").unwrap();
        fs::hard_link(&original, &link).unwrap();
        let (a, b) = (fs::metadata(&original).unwrap(), fs::metadata(&link).unwrap());

        assert_eq!(same_node(&a, &b), Sameness::Same);
        assert_eq!(link_count(&a), Some(2));
    }

    #[test]
    fn two_files_side_by_side_differ_but_share_a_device() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), b"1").unwrap();
        fs::write(dir.path().join("b"), b"2").unwrap();
        let a = fs::metadata(dir.path().join("a")).unwrap();
        let b = fs::metadata(dir.path().join("b")).unwrap();

        assert_eq!(same_node(&a, &b), Sameness::Different);
        assert_eq!(same_device(&a, &b), Sameness::Same);
        assert_eq!(link_count(&a), Some(1));
        assert_eq!(FileId::of(&a).map(FileId::device), device_of(&a));
    }

    #[test]
    fn a_followed_symlink_is_its_target_and_an_unfollowed_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("link");
        fs::write(&target, b"x").unwrap();
        crate::testing::symlink(&target, &link).unwrap();
        let target_meta = fs::metadata(&target).unwrap();

        assert_eq!(same_node(&fs::metadata(&link).unwrap(), &target_meta), Sameness::Same);
        assert_eq!(
            same_node(&fs::symlink_metadata(&link).unwrap(), &target_meta),
            Sameness::Different
        );
    }
}

/// The path reader on every host OS: the one that answers on Windows (#1291), where no
/// `Metadata` carries an id.
mod by_path {
    use std::fs;

    use super::*;

    #[test]
    fn a_hard_link_is_the_same_node_and_a_sibling_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let (original, link, sibling) = (dir.path().join("a.txt"), dir.path().join("b.txt"), dir.path().join("c"));
        fs::write(&original, b"x").unwrap();
        fs::hard_link(&original, &link).unwrap();
        fs::write(&sibling, b"x").unwrap();

        assert_eq!(same_node_at(&original, &link), Sameness::Same);
        assert_eq!(same_node_at(&original, &sibling), Sameness::Different);
        let (a, c) = (
            FileId::at(&original, Follow::Links),
            FileId::at(&sibling, Follow::Links),
        );
        assert_eq!(a.map(FileId::device), c.map(FileId::device), "one folder, one volume");
    }

    /// An open handle names the node it opened, even once another file took its name.
    #[test]
    fn an_open_file_names_the_node_it_opened_after_a_swap() {
        let dir = tempfile::tempdir().unwrap();
        let (path, aside) = (dir.path().join("a.txt"), dir.path().join("aside.txt"));
        fs::write(&path, b"x").unwrap();
        let file = fs::File::open(&path).unwrap();
        let opened = FileId::at(&path, Follow::Links);
        assert!(opened.is_some());
        assert_eq!(FileId::of_open(&file), opened);

        fs::rename(&path, &aside).unwrap();
        fs::write(&path, b"y").unwrap();
        assert_ne!(FileId::at(&path, Follow::Links), opened, "a new file at the name");
        assert_eq!(FileId::of_open(&file), opened, "the handle still names the first");
    }

    #[test]
    fn a_folder_has_an_id_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("inner");
        fs::create_dir(&inner).unwrap();

        assert_eq!(same_node_at(dir.path(), dir.path()), Sameness::Same);
        assert_eq!(same_node_at(dir.path(), &inner), Sameness::Different);
    }

    #[test]
    fn a_file_replaced_under_its_name_is_a_different_node() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invoice.pdf");
        fs::write(&path, b"aaaa").unwrap();
        let before = FileId::at(&path, Follow::NotLinks).expect("a local file has an id");
        // A second name keeps the old node alive, so no filesystem can hand its number on.
        fs::hard_link(&path, dir.path().join("kept")).unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"bbbb").unwrap();

        assert_ne!(FileId::at(&path, Follow::NotLinks), Some(before));
    }

    #[test]
    fn a_path_that_isnt_there_has_no_id() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");

        assert_eq!(FileId::at(&missing, Follow::Links), None);
        assert_eq!(same_node_at(&missing, dir.path()), Sameness::Unknown);
    }

    #[test]
    fn a_followed_link_is_its_target_and_an_unfollowed_one_is_not() {
        if crate::testing::skip_without_symlinks() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (target, link) = (dir.path().join("target"), dir.path().join("link"));
        fs::write(&target, b"x").unwrap();
        crate::testing::symlink(&target, &link).unwrap();
        let target_id = FileId::at(&target, Follow::Links);

        assert_eq!(FileId::at(&link, Follow::Links), target_id);
        assert_ne!(FileId::at(&link, Follow::NotLinks), target_id);
    }

    #[test]
    fn two_items_in_one_folder_share_a_device_and_a_missing_one_cant_say() {
        let dir = tempfile::tempdir().unwrap();
        let (file, folder) = (dir.path().join("a.txt"), dir.path().join("sub"));
        fs::write(&file, b"x").unwrap();
        fs::create_dir(&folder).unwrap();

        assert!(device_at(&file).is_some(), "a local file has a device");
        assert_eq!(same_device_at(&file, &folder), Sameness::Same);
        // Windows keys `device_at` by the volume's GUID or share, not the serial a `FileId` holds.
        #[cfg(unix)]
        assert_eq!(device_at(&file), FileId::at(&file, Follow::Links).map(FileId::device));
        assert_eq!(same_device_at(&file, &dir.path().join("missing")), Sameness::Unknown);
    }

    #[test]
    fn a_hidden_identity_hides_its_device_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        {
            let _hidden = hide_identity_of(&[&a]);
            assert_eq!(same_device_at(&a, &b), Sameness::Unknown);
        }
        assert_eq!(same_device_at(&a, &b), Sameness::Same);
    }

    #[test]
    fn a_hidden_identity_hides_from_the_path_reader_too() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        {
            let _hidden = hide_identity_of(&[&a]);
            assert_eq!(same_node_at(&a, &b), Sameness::Unknown);
        }
        assert_eq!(same_node_at(&a, &b), Sameness::Different);
    }
}

#[cfg(unix)]
#[test]
fn a_hidden_identity_reads_as_unknown_until_the_guard_drops() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    fs::write(&a, b"1").unwrap();
    fs::write(&b, b"2").unwrap();
    {
        let _hidden = hide_identity_of(&[&a]);
        let (ma, mb) = (fs::metadata(&a).unwrap(), fs::metadata(&b).unwrap());
        assert_eq!(same_node(&ma, &mb), Sameness::Unknown);
        assert_eq!(same_device(&ma, &mb), Sameness::Unknown);
        assert_eq!(link_count(&ma), None);
        assert!(FileId::of(&mb).is_some(), "only the named path hides");
    }
    let (ma, mb) = (fs::metadata(&a).unwrap(), fs::metadata(&b).unwrap());
    assert_eq!(same_node(&ma, &mb), Sameness::Different);
}
