//! The double's dates: it lists only what it claims, and lies on request so the
//! shared `conformance::assert_listing_dates_are_claimed` can be seen catching a
//! backend that fills a date it never claimed.

use super::*;
use crate::app_path;
use crate::entry::FileEntry;
use std::path::Path;

/// A folder holding one file that carries every date a listing can.
fn dated_volume() -> InMemoryVolume {
    InMemoryVolume::with_entries(
        "Test",
        vec![
            FileEntry::new("dated".to_string(), "/dated".to_string(), true, false),
            FileEntry {
                modified_at: Some(1_700_000_300),
                created_at: Some(1_700_000_100),
                added_at: Some(1_700_000_200),
                ..FileEntry::new("file.txt".to_string(), "/dated/file.txt".to_string(), false, false)
            },
        ],
    )
}

#[tokio::test]
async fn the_double_claims_every_date_by_default_and_lists_them() {
    let volume = dated_volume();

    assert_eq!(volume.supported_dates(app_path!("/dated")).await, SupportedDates::ALL);
    let listed = volume.list_directory(app_path!("/dated"), None).await.unwrap();
    assert_eq!(
        (listed[0].created_at, listed[0].added_at),
        (Some(1_700_000_100), Some(1_700_000_200))
    );
    conformance::assert_listing_dates_are_claimed(&volume, Path::new("/dated")).await;
}

/// A volume without an added date (an smbfs mount) drops it from listings and
/// `get_metadata` alike, keeps the creation date, and claims exactly that.
#[tokio::test]
async fn supported_dates_drop_the_unclaimed_date_from_listings_and_stats() {
    let volume = dated_volume().with_supported_dates(SupportedDates::CREATED);

    assert_eq!(
        volume.supported_dates(app_path!("/dated")).await,
        SupportedDates::CREATED
    );
    let listed = volume.list_directory(app_path!("/dated"), None).await.unwrap();
    assert_eq!((listed[0].created_at, listed[0].added_at), (Some(1_700_000_100), None));
    let stat = volume.get_metadata(app_path!("/dated/file.txt")).await.unwrap();
    assert_eq!((stat.created_at, stat.added_at), (Some(1_700_000_100), None));
    conformance::assert_listing_dates_are_claimed(&volume, Path::new("/dated")).await;
}

/// The cell catches a backend whose claim drifted from its mapping: it lists a
/// creation date and claims none.
#[tokio::test]
#[should_panic(expected = "carries a creation date")]
async fn the_dates_contract_catches_a_listed_creation_date_left_unclaimed() {
    let volume = dated_volume()
        .with_supported_dates(SupportedDates::CREATED)
        .with_dates_claimed_as(SupportedDates::NONE);

    conformance::assert_listing_dates_are_claimed(&volume, Path::new("/dated")).await;
}

/// And an added date listed but not claimed.
#[tokio::test]
#[should_panic(expected = "carries an added date")]
async fn the_dates_contract_catches_a_listed_added_date_left_unclaimed() {
    let volume = dated_volume().with_dates_claimed_as(SupportedDates::CREATED);

    conformance::assert_listing_dates_are_claimed(&volume, Path::new("/dated")).await;
}

/// An empty fixture can't pass by accident.
#[tokio::test]
#[should_panic(expected = "must hold at least one entry")]
async fn the_dates_contract_refuses_an_empty_fixture() {
    let volume = InMemoryVolume::with_entries(
        "Test",
        vec![FileEntry::new("empty".to_string(), "/empty".to_string(), true, false)],
    );

    conformance::assert_listing_dates_are_claimed(&volume, Path::new("/empty")).await;
}
