//! Why a server's share list didn't list, as data rather than as a sentence.
//!
//! A share list (`smb://<server>` on the `network` volume) is a listing like any
//! other, so its refusal is filesystem vocabulary: [`ShareListError`] says what the
//! server (or the client) answered, and [`ShareListRefusal`] is what a listing
//! refuses with (`VolumeError::ShareListRefused`), naming the server it asked. The
//! SMB protocol layer fills the error (`cmdr_smb::classify_error`) and re-exports it,
//! so its callers write `cmdr_smb::ShareListError`.
//!
//! Every `message` is a log diagnostic, ❌ never words a person reads: the frontend
//! words each variant.

use serde::{Deserialize, Serialize};

/// Error types for share listing operations.
///
/// Uses internally tagged representation so each variant can carry different fields
/// while keeping a flat JSON shape (`{ "type": "...", "message": "..." }`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ShareListError {
    /// The server is offline, or nothing is listening on the SMB port.
    HostUnreachable {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// The server accepted the connection and then went quiet.
    Timeout {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// The server turned an anonymous session away; credentials are needed.
    AuthRequired {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// Guest access won't work.
    SigningRequired {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// The credentials the caller supplied were rejected.
    AuthFailed {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// The server's reply on a signed session didn't carry a valid signature, so Cmdr refused
    /// it. A possible sign of tampering, so ❌ never a fallback to another client.
    SignatureInvalid {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// The server didn't confirm the negotiation the connection ran on (SMB 3.0 and 3.0.2 check it
    /// after sign-in), so smb2 closed the connection. A possible sign of tampering, like
    /// [`Self::SignatureInvalid`], so ❌ never a fallback to another client.
    NegotiationInvalid {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// The server only accepts encrypted connections, and this one agreed on no way to encrypt.
    /// ❌ Never a fallback to another client: the CLI fallback lists as a guest, unencrypted.
    EncryptionUnavailable {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// Cmdr requires encryption (`SmbEncryption`, in `cmdr-smb`) and this connection can't have it:
    /// the server agreed on no cipher, or it answered with a guest session, which has no keys.
    /// ❌ Never a fallback to another client: the CLI fallback ignores the requirement.
    EncryptionRequired {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// The exchange broke down in a way none of the other variants describes.
    ProtocolError {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// The hostname never resolved to an address.
    ResolutionFailed {
        /// Diagnostic detail, for logs.
        message: String,
    },
    /// A required CLI tool is not installed.
    MissingDependency {
        /// Diagnostic detail, for logs.
        message: String,
        /// What the user would run to install it, when there's a one-liner.
        #[serde(rename = "installCommand")]
        install_command: Option<String>,
    },
}

impl std::fmt::Display for ShareListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HostUnreachable { message } => write!(f, "Host unreachable: {}", message),
            Self::Timeout { message } => write!(f, "Timeout: {}", message),
            Self::AuthRequired { message } => write!(f, "Authentication required: {}", message),
            Self::SigningRequired { message } => write!(f, "SMB signing required: {}", message),
            Self::AuthFailed { message } => write!(f, "Authentication failed: {}", message),
            Self::SignatureInvalid { message } => write!(f, "Reply refused for its signature: {}", message),
            Self::NegotiationInvalid { message } => write!(f, "Negotiation not confirmed: {}", message),
            Self::EncryptionUnavailable { message } => write!(f, "Encryption unavailable: {}", message),
            Self::EncryptionRequired { message } => write!(f, "Encryption required by Cmdr: {}", message),
            Self::ProtocolError { message } => write!(f, "Protocol error: {}", message),
            Self::ResolutionFailed { message } => write!(f, "Resolution failed: {}", message),
            Self::MissingDependency { message, .. } => write!(f, "Missing dependency: {}", message),
        }
    }
}

impl ShareListError {
    /// Whether an account is what this listing lacks: the server wants one, wants a
    /// signed session no guest can have, or turned the offered one away. The pane
    /// offers its sign-in for exactly these, and ❌ no retry, which without an
    /// account gets the same answer (cmdr-reports#10).
    pub fn wants_sign_in(&self) -> bool {
        matches!(
            self,
            Self::AuthRequired { .. } | Self::SigningRequired { .. } | Self::AuthFailed { .. }
        )
    }

    /// The variant as a stable log token (its wire `type`), for a line that keeps the
    /// diagnostic `message` out.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::HostUnreachable { .. } => "host_unreachable",
            Self::Timeout { .. } => "timeout",
            Self::AuthRequired { .. } => "auth_required",
            Self::SigningRequired { .. } => "signing_required",
            Self::AuthFailed { .. } => "auth_failed",
            Self::SignatureInvalid { .. } => "signature_invalid",
            Self::NegotiationInvalid { .. } => "negotiation_invalid",
            Self::EncryptionUnavailable { .. } => "encryption_unavailable",
            Self::EncryptionRequired { .. } => "encryption_required",
            Self::ProtocolError { .. } => "protocol_error",
            Self::ResolutionFailed { .. } => "resolution_failed",
            Self::MissingDependency { .. } => "missing_dependency",
        }
    }
}

/// A share-list listing the server (or the client) refused: what it answered, and
/// the server it was asked, by the name the Servers list knows it by.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareListRefusal {
    /// The server's name as the Servers list knows it (its Bonjour name, or the
    /// address a person typed), for the words.
    pub server: String,
    /// What the listing came to.
    pub error: ShareListError,
}
