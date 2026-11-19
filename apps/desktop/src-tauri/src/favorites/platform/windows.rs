//! The Windows arm: the user folder first, then the Desktop, Documents, and Downloads known
//! folders, wherever Windows keeps them. `dirs` reads each through `SHGetKnownFolderPath`
//! (`FOLDERID_Desktop`, `FOLDERID_Documents`, `FOLDERID_Downloads`), so a folder the person moved
//! (its Properties > Location tab) or OneDrive's folder backup redirected
//! (`%USERPROFILE%\OneDrive\Documents`) seeds where it really is. `home\<name>` is only the
//! fallback for a known folder Windows can't name.

use std::path::{Path, PathBuf};

/// Where Windows says the three seeded folders are, or `None` for one it can't name.
struct KnownFolders {
    desktop: Option<PathBuf>,
    documents: Option<PathBuf>,
    downloads: Option<PathBuf>,
}

impl KnownFolders {
    fn of_this_user() -> Self {
        KnownFolders {
            desktop: dirs::desktop_dir(),
            documents: dirs::document_dir(),
            downloads: dirs::download_dir(),
        }
    }
}

pub(crate) fn default_seed(home: &Path) -> Vec<(PathBuf, &'static str)> {
    seed_from(home, KnownFolders::of_this_user())
}

/// The first row is "User folder", ❌ never "Home": on Windows 11 "Home" is File Explorer's
/// landing page, a different place (`docs/notes/windows-port/copy-decisions.md` § Glossary).
fn seed_from(home: &Path, known: KnownFolders) -> Vec<(PathBuf, &'static str)> {
    let or_under_home = |found: Option<PathBuf>, name: &str| found.unwrap_or_else(|| home.join(name));
    vec![
        (home.to_path_buf(), "User folder"),
        (or_under_home(known.desktop, "Desktop"), "Desktop"),
        (or_under_home(known.documents, "Documents"), "Documents"),
        (or_under_home(known.downloads, "Downloads"), "Downloads"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing_known() -> KnownFolders {
        KnownFolders {
            desktop: None,
            documents: None,
            downloads: None,
        }
    }

    #[test]
    fn names_the_user_folder_in_windows_own_words() {
        let seed = seed_from(Path::new(r"C:\Users\test_user_1"), nothing_known());
        assert_eq!(seed[0], (PathBuf::from(r"C:\Users\test_user_1"), "User folder"));
        let names: Vec<&str> = seed.iter().map(|(_, name)| *name).collect();
        assert_eq!(names, vec!["User folder", "Desktop", "Documents", "Downloads"]);
        assert_eq!(seed[3].0, Path::new(r"C:\Users\test_user_1").join("Downloads"));
    }

    /// OneDrive's folder backup moves Desktop and Documents under `OneDrive`, and a person can
    /// move Downloads to another drive: the seed follows the known folders.
    #[test]
    fn a_moved_known_folder_seeds_where_it_really_is() {
        let seed = seed_from(
            Path::new(r"C:\Users\test_user_1"),
            KnownFolders {
                desktop: Some(PathBuf::from(r"C:\Users\test_user_1\OneDrive\Desktop")),
                documents: Some(PathBuf::from(r"C:\Users\test_user_1\OneDrive\Documents")),
                downloads: Some(PathBuf::from(r"D:\Downloads")),
            },
        );
        assert_eq!(seed[1].0, PathBuf::from(r"C:\Users\test_user_1\OneDrive\Desktop"));
        assert_eq!(seed[2].0, PathBuf::from(r"C:\Users\test_user_1\OneDrive\Documents"));
        assert_eq!(seed[3].0, PathBuf::from(r"D:\Downloads"));
    }

    /// This machine's own answer: every seeded known folder is one Windows names, and it's
    /// there on disk.
    #[test]
    fn this_users_known_folders_resolve_and_exist() {
        let home = dirs::home_dir().expect("a user profile");
        let seed = default_seed(&home);
        for (path, name) in &seed[1..] {
            assert!(path.is_absolute(), "{name}: {}", path.display());
            assert!(path.is_dir(), "{name}: {} isn't a folder", path.display());
        }
        assert_eq!(Some(seed[3].0.clone()), dirs::download_dir());
    }
}
