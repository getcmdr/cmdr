//! What the NFS place list owes the Servers list: one entry per export, found again the
//! way the volume id finds it, a uid that's a setting rather than an identity,
//! and a pin a reconnect can't undo.
//!
//! In-memory only: these run before `load_known_nfs_places` names a file, so
//! `save()` is a no-op. Each cell uses a host of its own, since the store is
//! process-global.

use super::*;

fn place(host: &str, export: &str) -> KnownNfsPlace {
    KnownNfsPlace {
        host: host.to_string(),
        port: None,
        export: export.to_string(),
        uid: 1000,
        gid: 100,
        other_gids: Vec::new(),
        start_folder: None,
        auto_reconnect: true,
        pinned: false,
        last_connected_at: "2026-10-20T10:00:00Z".to_string(),
    }
}

#[test]
fn every_export_on_one_server_is_its_own_entry() {
    let public = place("exports.test", "/Public");
    let media = place("exports.test", "/Media");
    remember(public.clone());
    remember(media.clone());
    assert!(find(&public.volume_id()).is_some());
    assert!(find(&media.volume_id()).is_some());
    assert_ne!(public.volume_id(), media.volume_id());
    assert_eq!(public.server_id(), media.server_id(), "one server, two places");
}

#[test]
fn a_changed_uid_is_the_same_place() {
    // ❗ A setting, not the identity: fixing a uid typo keeps every tab and favorite.
    let before = place("uid.test", "/Public");
    let after = KnownNfsPlace {
        uid: 1026,
        gid: 1026,
        other_gids: Vec::new(),
        ..before.clone()
    };
    assert_eq!(before.volume_id(), after.volume_id());
    remember(before.clone());
    remember(after.clone());
    let stored = find(&before.volume_id()).expect("still one entry");
    assert_eq!((stored.uid, stored.gid), (1026, 1026), "the edit replaced the entry");
    assert_eq!(
        all().iter().filter(|entry| entry.host == "uid.test").count(),
        1,
        "a uid change never adds a second entry"
    );
}

#[test]
fn an_export_spelled_two_ways_is_one_entry() {
    let typed = place("spelling.test", "/volume1/data/");
    let canonical = place("spelling.test", "/volume1//data");
    remember(typed.clone());
    remember(canonical.clone());
    assert_eq!(typed.volume_id(), canonical.volume_id());
    let stored = find(&typed.volume_id()).expect("saved");
    assert_eq!(stored.export, "/volume1/data", "stored canonical");
    assert_eq!(all().iter().filter(|e| e.host == "spelling.test").count(), 1);
}

#[test]
fn no_port_and_2049_are_one_place() {
    // The id carries the canonical port, so a typed `:2049` names the place a
    // portless address saved.
    let portless = place("ports.test", "/Public");
    let explicit = KnownNfsPlace {
        port: Some(2049),
        ..portless.clone()
    };
    assert_eq!(portless.volume_id(), explicit.volume_id());
    let other_port = KnownNfsPlace {
        port: Some(2050),
        ..portless.clone()
    };
    assert_ne!(portless.volume_id(), other_port.volume_id());
}

#[test]
fn a_reconnect_cant_repin_a_place_the_user_unpinned() {
    let public = place("pins.test", "/Public");
    remember(KnownNfsPlace {
        pinned: true,
        ..public.clone()
    });
    assert!(set_pinned(&public.volume_id(), false));
    remember(KnownNfsPlace {
        pinned: true,
        auto_reconnect: false,
        ..public.clone()
    });
    let stored = find(&public.volume_id()).expect("still saved");
    assert!(!stored.pinned, "the stored pin wins on a replace");
    assert!(!stored.auto_reconnect, "everything else is the new entry");
}

#[test]
fn the_name_is_the_servers_and_a_blank_one_leaves_it_alone() {
    let public = place("names.test", "/Public");
    let media = place("names.test", "/Media");
    remember(public.clone());
    remember(media.clone());
    adopt_typed_name(&public, "Home NAS");
    assert_eq!(server_name(&media), "Home NAS", "every export reads its server's name");
    adopt_typed_name(&media, "  ");
    assert_eq!(server_name(&public), "Home NAS", "a blank name unnames nothing");
    adopt_typed_name(&media, "Attic NAS");
    assert_eq!(server_label(&public), "Attic NAS", "the newest name typed wins");
}

#[test]
fn an_unnamed_server_reads_as_its_host_and_an_export_as_its_path() {
    let nested = place("labels.test", "/volume1/data");
    let root = place("labels.test", "/");
    assert_eq!(server_label(&nested), "labels.test");
    assert_eq!(place_label(&nested), "volume1/data");
    assert_eq!(place_label(&root), "labels.test", "the root export reads as the server");
    assert_eq!(place_label_named("Home NAS", &root), "Home NAS");
}

#[test]
fn forgetting_the_last_place_forgets_the_servers_name() {
    let public = place("forget.test", "/Public");
    let media = place("forget.test", "/Media");
    remember(public.clone());
    remember(media.clone());
    adopt_typed_name(&public, "Basement");
    assert!(forget(&public.volume_id()));
    assert!(find(&media.volume_id()).is_some(), "a sibling stays");
    assert_eq!(server_name(&media), "Basement", "the name stays while a place does");
    assert!(forget(&media.volume_id()));
    assert_eq!(server_name(&media), "", "the last place takes the name along");
    assert!(!forget(&media.volume_id()), "nothing left to forget");
}

#[test]
fn resolved_params_save_the_split_and_the_settings() {
    let mut params = NfsConnectionParams::new(
        NfsServer::new("split.test", Some(2050)),
        "/share/Public/",
        NfsIdentity::new(7, 8),
    );
    params.start_folder = Some("/photos".to_string());
    params.auto_reconnect = false;
    let entry = KnownNfsPlace::from_params(&params, true, "now".to_string());
    assert_eq!(entry.export, "/share/Public");
    assert_eq!(entry.port, Some(2050));
    assert_eq!((entry.uid, entry.gid), (7, 8));
    assert_eq!(entry.start_folder.as_deref(), Some("/photos"));
    assert!(!entry.auto_reconnect);
    assert_eq!(
        entry.volume_id(),
        params.volume_id(),
        "the entry names the place it was built from"
    );
}

#[test]
fn a_place_saved_before_the_other_groups_keeps_working_unchanged() {
    let json = r#"{ "knownNfsPlaces": [{ "host": "old.test", "export": "/Public", "uid": 1000, "gid": 100 }] }"#;
    let store: KnownNfsPlacesStore = serde_json::from_str(json).expect("reads");
    let entry = &store.known_nfs_places[0];
    assert_eq!(entry.other_gids, Vec::<u32>::new());
    assert_eq!(entry.identity(), NfsIdentity::new(1000, 100));
    assert_eq!(entry.volume_id(), place("old.test", "/Public").volume_id());
}

#[test]
fn the_other_groups_survive_the_store_and_the_params_but_not_the_id() {
    let grouped = KnownNfsPlace {
        other_gids: vec![2001, 2002],
        ..place("groups.test", "/Public")
    };
    assert_eq!(grouped.volume_id(), place("groups.test", "/Public").volume_id());

    let json = serde_json::to_string(&grouped).expect("writes");
    let read: KnownNfsPlace = serde_json::from_str(&json).expect("reads");
    assert_eq!(read.other_gids, vec![2001, 2002]);

    assert_eq!(grouped.params().identity.other_gids, vec![2001, 2002]);
    let back = KnownNfsPlace::from_params(&grouped.params(), false, String::new());
    assert_eq!(back.other_gids, vec![2001, 2002]);
}

#[test]
fn a_store_with_a_broken_entry_keeps_the_rest() {
    let json = r#"{
      "knownNfsPlaces": [
        { "host": "lenient.test", "export": "/Public", "uid": 1000, "gid": 100 },
        { "host": "lenient.test", "export": 42 }
      ],
      "knownNfsServers": [{ "host": "lenient.test", "displayName": "NAS" }]
    }"#;
    let store: KnownNfsPlacesStore = serde_json::from_str(json).expect("lenient");
    assert_eq!(store.known_nfs_places.len(), 1);
    let entry = &store.known_nfs_places[0];
    assert!(entry.auto_reconnect, "on unless the person switched it off");
    assert!(!entry.pinned);
    assert_eq!(store.known_nfs_servers.len(), 1);
}
