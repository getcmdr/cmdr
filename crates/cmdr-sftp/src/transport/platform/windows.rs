//! Windows: the OpenSSH agent service listens on a named pipe, `\\.\pipe\openssh-ssh-agent`
//! unless `SSH_AUTH_SOCK` names another (Git for Windows' and 1Password's agents set it).
//! Pageant, PuTTY's agent, isn't asked yet.

use russh::keys::agent::client::AgentClient;

use super::Connecting;

/// The pipe Windows' bundled OpenSSH agent service listens on.
const OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

pub(crate) fn connect_agent() -> Connecting {
    Box::pin(async {
        let pipe = std::env::var_os("SSH_AUTH_SOCK").unwrap_or_else(|| OPENSSH_AGENT_PIPE.into());
        AgentClient::connect_named_pipe(pipe)
            .await
            .ok()
            .map(AgentClient::dynamic)
    })
}
