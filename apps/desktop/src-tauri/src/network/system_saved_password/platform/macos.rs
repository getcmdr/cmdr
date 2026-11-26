//! macOS: the login keychain, where Finder saves the SMB passwords it signs in with, read through
//! `crate::secrets::system_keychain_smb`.

use crate::network::NetworkHost;
use crate::network::keychain::SmbCredentials;
use crate::secrets::system_keychain_smb;

pub(crate) fn candidates(host: &NetworkHost) -> Vec<String> {
    let hostname = host
        .hostname
        .clone()
        .unwrap_or_else(|| crate::network::service_name_to_hostname(&host.name));
    let dialed = host.ip_address.as_deref().unwrap_or(&hostname);
    let mut aliases = vec![host.name.clone(), format!("{}._smb._tcp.local", host.name)];
    aliases.extend(crate::network::smb_server_address::system_keychain_aliases(dialed));
    system_keychain_smb::server_query_candidates(dialed, Some(&hostname), &aliases)
}

pub(crate) fn saved_account(candidates: &[String]) -> Option<String> {
    system_keychain_smb::account_for_any(candidates).map(|(_, account)| account)
}

pub(crate) fn saved_password(candidates: &[String]) -> Option<SmbCredentials> {
    system_keychain_smb::read_password(candidates).map(|saved| SmbCredentials {
        username: saved.username,
        password: saved.password,
    })
}
