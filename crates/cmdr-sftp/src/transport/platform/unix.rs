//! Both Unixes: the agent's socket is `$SSH_AUTH_SOCK`.

use russh::keys::agent::client::AgentClient;

use super::Connecting;

pub(crate) fn connect_agent() -> Connecting {
    Box::pin(async { AgentClient::connect_env().await.ok().map(AgentClient::dynamic) })
}
