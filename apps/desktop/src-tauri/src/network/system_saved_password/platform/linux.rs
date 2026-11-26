//! Linux: Cmdr reads no other app's saved SMB passwords here, so there's nothing to offer.

use crate::network::NetworkHost;
use crate::network::keychain::SmbCredentials;

pub(crate) fn candidates(_host: &NetworkHost) -> Vec<String> {
    Vec::new()
}

pub(crate) fn saved_account(_candidates: &[String]) -> Option<String> {
    None
}

pub(crate) fn saved_password(_candidates: &[String]) -> Option<SmbCredentials> {
    None
}
