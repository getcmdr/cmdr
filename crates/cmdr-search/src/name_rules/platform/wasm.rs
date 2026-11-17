//! The browser build has no local filesystem; the demo passes its own rules
//! (`cmdr-listing-wasm`'s `DEMO_NAME_RULES`), and `host()` keeps Linux's exact answer.

use super::super::NameRules;

pub(crate) const HOST: NameRules = NameRules::LINUX;
