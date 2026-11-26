//! Where each OS keeps the SMB passwords another app saved, for a share list's sign-in. A platform
//! seam (`docs/guides/platform-seams.md`).

use crate::network::NetworkHost;
use crate::network::keychain::SmbCredentials;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

const _: fn(&NetworkHost) -> Vec<String> = candidates;
const _: fn(&[String]) -> Option<String> = saved_account;
const _: fn(&[String]) -> Option<SmbCredentials> = saved_password;

/// Every name the saving app may have keyed `host`'s item by: where it's dialed, its `.local`
/// hostname, its discovery name, and that name's `_smb._tcp.local` service form, plus what
/// discovery pairs with them. Empty where the OS keeps none Cmdr reads.
pub(crate) use imp::candidates;

/// The account saved under the first of `candidates` that has one, attributes only: ❌ never a
/// consent dialog. Blocking FFI, so call it off the async worker.
pub(crate) use imp::saved_account;

/// The password saved under the first of `candidates` that has one. ❗ Raises the macOS consent
/// dialog and blocks on the person: only `read_once` calls it, behind its memo.
pub(crate) use imp::saved_password;
