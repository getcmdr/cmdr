//! Linux: the session's agent (ssh-agent, GNOME Keyring, gpg-agent) in `$SSH_AUTH_SOCK`.

pub(crate) use super::unix::connect_agent;
