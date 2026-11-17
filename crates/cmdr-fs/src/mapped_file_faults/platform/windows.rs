//! Windows: nothing is recovered yet, so [`generation`](super::super::generation) never moves and
//! a read of a mapping whose disk went away still crashes, as it did before this module existed.
//! Windows raises `EXCEPTION_IN_PAGE_ERROR` there, which a vectored exception handler could
//! answer the same way; that handler belongs with the crash reporter's Windows arm (windows-port
//! plan, task 47).

pub(crate) fn prepare() {}

/// No handler to install: a test process on Windows has nothing that recovers.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn install_test_handler() {}
