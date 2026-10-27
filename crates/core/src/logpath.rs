use std::path::{Path, PathBuf};

const PREFIX_TAIL: &str =
    "steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log";

pub fn find() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let override_path = std::env::var_os("CORUFRAME_LOG").map(PathBuf::from);
    find_in(&home, xdg.as_deref(), override_path.as_deref())
}

// Check the explicit override, the XDG root, the two classic Steam roots, and
// every library listed by any of their `libraryfolders.vdf`. A Steam install on
// another library is otherwise invisible.
fn find_in(home: &Path, xdg: Option<&Path>, override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = override_path {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    let mut roots = Vec::new();
    if let Some(xdg) = xdg {
        roots.push(xdg.join("Steam"));
    }
    roots.push(home.join(".local/share/Steam"));
    roots.push(home.join(".steam/steam"));
    roots.push(home.join(".steam/root"));

    let mut candidates = roots.clone();
    for root in &roots {
        let vdf = root.join("steamapps/libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(vdf) {
            candidates.extend(library_paths(&text));
        }
    }
    candidates
        .into_iter()
        .map(|root| root.join(PREFIX_TAIL))
        .find(|path| path.is_file())
}

// Steam's VDF is quoted key/value; the only key we need is "path".
fn library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter(|l| l.trim_start().starts_with("\"path\""))
        .filter_map(|l| {
            let mut parts = l.split('"').skip(1).step_by(2);
            parts.next()?;
            Some(PathBuf::from(parts.next()?.replace("\\\\", "\\")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("coruframe-logpath-{}-{name}", std::process::id()))
    }

    fn write_log(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn reads_every_library_path() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/user/.local/share/Steam"
	}
	"1"
	{
		"path"		"/mnt/ssd2/Games/Steam"
	}
}
"#;
        let paths = library_paths(vdf);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[1], PathBuf::from("/mnt/ssd2/Games/Steam"));
    }

    #[test]
    fn finds_a_moved_library_through_the_steam_root() {
        let base = temp_root("moved");
        let home = base.join("home");
        let library = base.join("moved-library");
        let vdf = home.join(".steam/root/steamapps/libraryfolders.vdf");
        std::fs::create_dir_all(vdf.parent().unwrap()).unwrap();
        std::fs::write(
            &vdf,
            format!(
                "\"libraryfolders\"\n{{\n\"1\"\n{{\n\"path\" \"{}\"\n}}\n}}\n",
                library.display()
            ),
        )
        .unwrap();
        let log = library.join(PREFIX_TAIL);
        write_log(&log);
        assert_eq!(find_in(&home, None, None), Some(log));

        let override_log = base.join("override.log");
        write_log(&override_log);
        assert_eq!(
            find_in(&home, None, Some(&override_log)),
            Some(override_log)
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn uses_the_xdg_steam_root() {
        let base = temp_root("xdg");
        let home = base.join("home");
        let xdg = base.join("xdg");
        std::fs::create_dir_all(&home).unwrap();
        let log = xdg.join("Steam").join(PREFIX_TAIL);
        write_log(&log);
        assert_eq!(find_in(&home, Some(&xdg), None), Some(log));
        let _ = std::fs::remove_dir_all(&base);
    }
}
