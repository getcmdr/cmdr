//! macOS: launchd hands every login session an agent socket in `$SSH_AUTH_SOCK`.

pub(crate) use super::unix::connect_agent;
