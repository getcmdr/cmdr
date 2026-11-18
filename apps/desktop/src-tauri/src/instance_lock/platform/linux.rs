//! Linux: no native alert surface we can rely on at this point in startup, so the stderr line and
//! the non-zero exit are the whole story.

use std::time::Duration;

pub(crate) fn show_alert(title: &str, body: &str, _timeout: Duration) {
    super::super::to_stderr(format_args!("{title}. {body}"));
}
