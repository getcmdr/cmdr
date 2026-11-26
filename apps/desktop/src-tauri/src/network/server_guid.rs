//! Which SMB server a share lives on: the GUID the server answers NEGOTIATE with.
//!
//! One NAS reached by two addresses (a LAN IP and a Tailscale one) mounts as two
//! volumes with two ids. A GUID the server minted ([`GuidOrigin::Minted`]) says
//! they're one box, and the share name pairs with it to prove two saved shares are
//! one share (`location::same_share`); the GUID alone names the BOX, never the share.
//! ❗ A Samba server's GUID is its NetBIOS name ([`GuidOrigin::NameDerived`]), so it
//! proves only a name and never pairs two shares (#1518).
//!
//! ❗ An all-zero GUID is no GUID: smb2 fills `Guid::ZERO` before a negotiate
//! lands, and a server may answer it. [`ServerGuid`] can't hold one, so two rows
//! that never learned their server can't match on it.

use serde::{Deserialize, Serialize};
use smb2::pack::Guid;

/// A server's non-zero GUID, in its canonical lowercase text form
/// (`8-4-4-4-12` hex digits, no braces): what the saved-share store keeps.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerGuid(String);

impl ServerGuid {
    /// The GUID a NEGOTIATE answered with, or `None` for the all-zero one.
    pub fn from_negotiated(guid: Guid) -> Option<Self> {
        if guid == Guid::ZERO {
            return None;
        }
        let tail: String = guid.data4.iter().map(|b| format!("{b:02x}")).collect();
        Some(Self(format!(
            "{:08x}-{:04x}-{:04x}-{}-{}",
            guid.data1,
            guid.data2,
            guid.data3,
            &tail[..4],
            &tail[4..]
        )))
    }

    /// A stored GUID read back, or `None` when it isn't one (malformed, or all
    /// zeros: a hand-edited store must not make two shares match).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        let groups: Vec<&str> = text.split('-').collect();
        let shape_ok = groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
            && groups.iter().all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()));
        let all_zero = text.bytes().all(|b| b == b'0' || b == b'-');
        (shape_ok && !all_zero).then_some(Self(text))
    }

    /// The canonical text form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the server minted this GUID or derived it from its name ([`GuidOrigin`]).
    pub fn origin(&self) -> GuidOrigin {
        let wire = self.wire_bytes();
        let name_len = wire.iter().take_while(|b| (0x20..=0x7e).contains(*b)).count();
        let nul_padded = wire[name_len..].iter().all(|b| *b == 0);
        if name_len > 0 && nul_padded {
            GuidOrigin::NameDerived
        } else {
            GuidOrigin::Minted
        }
    }

    /// The 16 bytes as NEGOTIATE carried them: the first three fields little-endian,
    /// the last eight in order.
    fn wire_bytes(&self) -> [u8; 16] {
        let hex: Vec<u8> = self
            .0
            .bytes()
            .filter(|b| *b != b'-')
            .map(|b| match b {
                b'0'..=b'9' => b - b'0',
                _ => b - b'a' + 10,
            })
            .collect();
        let mut text_order = [0u8; 16];
        for (byte, pair) in text_order.iter_mut().zip(hex.as_chunks::<2>().0) {
            *byte = (pair[0] << 4) | pair[1];
        }
        let mut wire = text_order;
        wire[0..4].reverse();
        wire[4..6].reverse();
        wire[6..8].reverse();
        wire
    }
}

/// Where a server's GUID came from, read off its bytes.
///
/// ❗ Samba (QNAP, Synology, a Raspberry Pi) doesn't mint a GUID: it copies its
/// lowercased NetBIOS name into the 16 bytes, NUL-padded (`naspolya` negotiates
/// `7073616e-6c6f-6179-0000-000000000000`; verified on a QNAP, dev log, 2026-11-26).
/// So such a GUID proves only a NAME: two Pis named `raspberrypi` answer the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidOrigin {
    /// A GUID the server minted for itself (Windows, macOS): it names one box.
    Minted,
    /// Printable ASCII, then only NULs: a Samba server's NetBIOS name. ❌ Never proof
    /// of one box (`location::share_identity`).
    NameDerived,
}

/// How a server GUID was learned. It decides whether the GUID may replace one a
/// row already holds, and whether it proves enough to let one member of a share
/// answer for another (`location::same_share`). Stored beside the GUID as
/// `serverGuidProof`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum GuidProof {
    /// From a session that signed in as the share's account: the latest truth
    /// about the box, so it overwrites (a NAS restored or replaced mints a new GUID).
    Authenticated,
    /// From a credential-free NEGOTIATE probe of a saved address, or a guest
    /// session: whatever answers there may not be the share's server (a hotel
    /// network can hand out the saved LAN IP), so it only FILLS a missing GUID,
    /// and only ever makes a `sameShareAs` hint. The default: a row saved before
    /// Cmdr kept the proof can't show a sign-in recorded its GUID.
    #[default]
    Unauthenticated,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Guid {
        Guid {
            data1: 0x6ba7_b810,
            data2: 0x9dad,
            data3: 0x11d1,
            data4: [0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8],
        }
    }

    #[test]
    fn an_all_zero_guid_is_no_guid() {
        assert_eq!(ServerGuid::from_negotiated(Guid::ZERO), None);
        assert_eq!(ServerGuid::parse("00000000-0000-0000-0000-000000000000"), None);
    }

    #[test]
    fn a_negotiated_guid_reads_back_as_itself() {
        let guid = ServerGuid::from_negotiated(sample()).expect("non-zero");
        assert_eq!(guid.as_str(), "6ba7b810-9dad-11d1-80b4-00c04fd430c8");
        assert_eq!(ServerGuid::parse(guid.as_str()), Some(guid.clone()));
        assert_eq!(
            ServerGuid::parse("6BA7B810-9DAD-11D1-80B4-00C04FD430C8"),
            Some(guid),
            "case doesn't make a second server"
        );
    }

    /// What a QNAP named `naspolya` negotiates (Samba, dev log 2026-11-26): its NetBIOS
    /// name in the GUID's wire bytes, NUL-padded.
    fn samba(name: &str) -> Guid {
        let mut bytes = [0u8; 16];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        Guid {
            data1: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            data2: u16::from_le_bytes([bytes[4], bytes[5]]),
            data3: u16::from_le_bytes([bytes[6], bytes[7]]),
            data4: [
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
            ],
        }
    }

    #[test]
    fn a_samba_guid_is_derived_from_the_netbios_name() {
        let guid = ServerGuid::from_negotiated(samba("naspolya")).expect("non-zero");
        assert_eq!(guid.as_str(), "7073616e-6c6f-6179-0000-000000000000");
        assert_eq!(guid.origin(), GuidOrigin::NameDerived);
        assert_eq!(
            ServerGuid::parse(guid.as_str()).map(|g| g.origin()),
            Some(GuidOrigin::NameDerived),
            "a stored one reads back with its origin"
        );
        for name in ["raspberrypi", "a", "testnas-15chars"] {
            let guid = ServerGuid::from_negotiated(samba(name)).expect("non-zero");
            assert_eq!(guid.origin(), GuidOrigin::NameDerived, "{name}");
        }
    }

    #[test]
    fn a_random_guid_is_minted_by_its_server() {
        assert_eq!(
            ServerGuid::from_negotiated(sample()).map(|g| g.origin()),
            Some(GuidOrigin::Minted)
        );
        // Printable, then NUL, then a non-NUL byte: not a padded name.
        let mut gapped = samba("nas");
        gapped.data4[7] = 0x01;
        assert_eq!(
            ServerGuid::from_negotiated(gapped).map(|g| g.origin()),
            Some(GuidOrigin::Minted)
        );
        // A control byte in the leading run: not a name.
        let mut control = samba("nas");
        control.data1 = u32::from_le_bytes([b'n', 0x07, b's', 0]);
        assert_eq!(
            ServerGuid::from_negotiated(control).map(|g| g.origin()),
            Some(GuidOrigin::Minted)
        );
    }

    #[test]
    fn a_malformed_stored_guid_is_no_guid() {
        for text in [
            "",
            "testnas",
            "{6ba7b810-9dad-11d1-80b4-00c04fd430c8}",
            "6ba7b810-9dad-11d1-80b4",
        ] {
            assert_eq!(ServerGuid::parse(text), None, "{text:?}");
        }
    }
}
