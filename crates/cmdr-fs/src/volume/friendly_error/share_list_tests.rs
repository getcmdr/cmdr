//! A share list's refusal (`VolumeError::ShareListRefused`) → its `ListingError`, one row
//! per `ShareListError` variant, so a new variant has to decide its classification here.

use std::path::Path;

use super::*;
use crate::share_list::{ShareListError, ShareListRefusal};
use crate::volume::VolumeError;

fn refused(error: ShareListError) -> ListingError {
    let err = VolumeError::ShareListRefused(Box::new(ShareListRefusal {
        server: "Test_user_1's NAS".to_string(),
        error,
    }));
    listing_error_from_volume_error(&err, Path::new("smb://test_user_1's nas"))
}

fn message() -> String {
    "diagnostic".to_string()
}

/// Every variant, with the classification the pane acts on: the sign-in reason for an
/// account it lacks (no retry: the same answer comes back without one), the server's
/// answer for the rest.
#[test]
fn every_share_list_refusal_maps_to_its_listing_error() {
    use ErrorActionKind::{OpenSmbEncryptionSetting, SignInToServer};
    use ErrorCategory::{NeedsAction, Serious, Transient};

    let cases: Vec<(ShareListError, bool, ErrorCategory, bool, Option<ErrorActionKind>)> = vec![
        // (error, is the sign-in reason, category, retry, action)
        (
            ShareListError::AuthRequired { message: message() },
            true,
            NeedsAction,
            false,
            Some(SignInToServer),
        ),
        (
            ShareListError::SigningRequired { message: message() },
            true,
            NeedsAction,
            false,
            Some(SignInToServer),
        ),
        (
            ShareListError::AuthFailed { message: message() },
            true,
            NeedsAction,
            false,
            Some(SignInToServer),
        ),
        (
            ShareListError::HostUnreachable { message: message() },
            false,
            Transient,
            true,
            None,
        ),
        (
            ShareListError::Timeout { message: message() },
            false,
            Transient,
            true,
            None,
        ),
        (
            ShareListError::ResolutionFailed { message: message() },
            false,
            Transient,
            true,
            None,
        ),
        (
            ShareListError::ProtocolError { message: message() },
            false,
            Transient,
            true,
            None,
        ),
        (
            ShareListError::SignatureInvalid { message: message() },
            false,
            Serious,
            true,
            None,
        ),
        (
            ShareListError::NegotiationInvalid { message: message() },
            false,
            Serious,
            true,
            None,
        ),
        (
            ShareListError::EncryptionUnavailable { message: message() },
            false,
            NeedsAction,
            false,
            None,
        ),
        (
            ShareListError::EncryptionRequired { message: message() },
            false,
            NeedsAction,
            true,
            Some(OpenSmbEncryptionSetting),
        ),
        (
            ShareListError::MissingDependency {
                message: message(),
                install_command: Some("sudo apt install smbclient".to_string()),
            },
            false,
            NeedsAction,
            true,
            None,
        ),
    ];

    for (error, sign_in, category, retry, action) in cases {
        let kind = error.kind();
        let listing = refused(error);
        match &listing.reason {
            ListingErrorReason::ShareListSignIn { refusal, server } => {
                assert!(sign_in, "{kind}: answered the sign-in reason");
                assert_eq!(refusal.kind(), kind, "{kind}: carries its own refusal");
                assert_eq!(server, "Test_user_1's NAS", "{kind}: names the server");
            }
            ListingErrorReason::ShareListUnavailable { refusal, server } => {
                assert!(!sign_in, "{kind}: answered the server's reason");
                assert_eq!(refusal.kind(), kind, "{kind}: carries its own refusal");
                assert_eq!(server, "Test_user_1's NAS", "{kind}: names the server");
            }
            other => panic!("{kind}: a share list's refusal read as {other:?}"),
        }
        assert_eq!(listing.category, category, "{kind}: category");
        assert_eq!(listing.retry_hint, retry, "{kind}: retry hint");
        assert_eq!(listing.action_kind, action, "{kind}: action");
        assert!(listing.provider.is_none(), "{kind}: no provider overlay");
    }
}

/// The install command reaches the frontend, which shows it in a copy box.
#[test]
fn a_missing_dependency_keeps_its_install_command() {
    let listing = refused(ShareListError::MissingDependency {
        message: message(),
        install_command: Some("sudo apt install smbclient".to_string()),
    });
    let ListingErrorReason::ShareListUnavailable {
        refusal: ShareListError::MissingDependency { install_command, .. },
        ..
    } = listing.reason
    else {
        panic!("not the missing dependency: {:?}", listing.reason);
    };
    assert_eq!(install_command.as_deref(), Some("sudo apt install smbclient"));
}

/// The technical details name what the server answered by its kind token.
#[test]
fn the_raw_detail_names_the_kind() {
    let listing = refused(ShareListError::AuthRequired { message: message() });
    assert!(
        listing.raw_detail.contains("auth_required"),
        "raw_detail {:?} names the kind",
        listing.raw_detail
    );
}
