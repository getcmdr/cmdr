//! When a full re-read started reading (`DETAILS.md` § "Read generations"). A
//! leaf, so the listing, the re-read, and the patch log can all name it
//! without depending on each other.

/// When a full re-read started reading, from its engine's one counter: a later
/// start has a higher generation. A listing remembers the newest one it
/// published, and a re-read older than that is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReadGeneration(u64);

impl ReadGeneration {
    /// What the streaming read's cache insert publishes, so any later re-read wins.
    pub(crate) const INSERT: Self = Self(0);

    /// The generation the engine's counter reads `count`.
    pub(crate) const fn of_count(count: u64) -> Self {
        Self(count)
    }
}
