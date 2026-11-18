//! Which Linux desktop this session runs, for the conventions that split by desktop (dialog button
//! order first). Read from `XDG_CURRENT_DESKTOP` once per process.

use std::sync::OnceLock;

use crate::platform::conventions::LinuxDesktop;

/// The desktop of this session, read once: a desktop doesn't change under a running app.
pub(crate) fn linux_desktop() -> LinuxDesktop {
    static DESKTOP: OnceLock<LinuxDesktop> = OnceLock::new();
    *DESKTOP.get_or_init(|| desktop_from(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref()))
}

/// `XDG_CURRENT_DESKTOP` is a colon-separated list, most specific first, of names from the
/// freedesktop.org registry (`GNOME`, `KDE`, `XFCE`, `X-Cinnamon`, …), so a derivative names its
/// base after itself: Ubuntu's session is `ubuntu:GNOME`, Budgie's `Budgie:GNOME`. The first entry
/// that is `GNOME` or `KDE` decides, case-insensitively; nothing named, or neither, is `Other`.
fn desktop_from(xdg_current_desktop: Option<&str>) -> LinuxDesktop {
    xdg_current_desktop
        .into_iter()
        .flat_map(|value| value.split(':'))
        .find_map(|name| {
            let name = name.trim();
            if name.eq_ignore_ascii_case("GNOME") {
                Some(LinuxDesktop::Gnome)
            } else if name.eq_ignore_ascii_case("KDE") {
                Some(LinuxDesktop::Kde)
            } else {
                None
            }
        })
        .unwrap_or(LinuxDesktop::Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_and_its_derivatives_read_as_gnome() {
        assert_eq!(desktop_from(Some("GNOME")), LinuxDesktop::Gnome);
        assert_eq!(desktop_from(Some("ubuntu:GNOME")), LinuxDesktop::Gnome);
        assert_eq!(desktop_from(Some("Budgie:GNOME")), LinuxDesktop::Gnome);
        assert_eq!(desktop_from(Some("GNOME-Flashback:GNOME")), LinuxDesktop::Gnome);
        assert_eq!(desktop_from(Some("gnome")), LinuxDesktop::Gnome);
    }

    #[test]
    fn plasma_reads_as_kde() {
        assert_eq!(desktop_from(Some("KDE")), LinuxDesktop::Kde);
    }

    #[test]
    fn everything_else_is_other() {
        assert_eq!(desktop_from(None), LinuxDesktop::Other);
        assert_eq!(desktop_from(Some("")), LinuxDesktop::Other);
        assert_eq!(desktop_from(Some("XFCE")), LinuxDesktop::Other);
        assert_eq!(desktop_from(Some("X-Cinnamon")), LinuxDesktop::Other);
        assert_eq!(desktop_from(Some("COSMIC")), LinuxDesktop::Other);
        // Whole names only: `GNOME-Flashback` alone doesn't claim GNOME's conventions.
        assert_eq!(desktop_from(Some("GNOME-Flashback")), LinuxDesktop::Other);
    }
}
