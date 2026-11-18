//! Picks how this OS reaches the user's SSH agent, the first rung of the auth ladder.

use std::future::Future;
use std::pin::Pin;

use russh::keys::agent::client::{AgentClient, AgentStream};

#[cfg(unix)]
mod unix;

#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;

/// A connected agent, one type on every OS.
pub(crate) type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

/// The connect, boxed so its signature can be pinned.
pub(crate) type Connecting = Pin<Box<dyn Future<Output = Option<Agent>> + Send>>;

const _: fn() -> Connecting = connect_agent;

/// The user's running agent, or `None` when there isn't one to ask (no agent is not a
/// failure; it's a rung that isn't there).
pub(super) use imp::connect_agent;
