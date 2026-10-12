//! Explicit per-user icon font installation; never invoked at startup.

use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

const FONT: &[u8] = include_bytes!("../assets/fonts/HerdrAgentIconsMax-Regular.ttf");
const NAME: &str = "herdr-lineup-HerdrAgentIconsMax-1b819949.ttf";
const START: &str = "# BEGIN herdr-lineup font";
const END: &str = "# END herdr-lineup font";

/// Paths are injected so tests never depend on the real user's home.
#[derive(Debug)]
pub struct Locations {
    font: PathBuf,
    font_root: PathBuf,
    config_root: PathBuf,
    config: PathBuf,
    home: PathBuf,
    state: PathBuf,
}

impl Locations {
    pub fn new(
        home: &Path,
        data: Option<&Path>,
        config: Option<&Path>,
        state: &Path,
        macos: bool,
    ) -> Self {
        let font_root = if macos {
            home.to_path_buf()
        } else {
            data.filter(|p| p.is_absolute())
                .unwrap_or(home)
                .to_path_buf()
        };
        let config_root = config
            .filter(|p| p.is_absolute())
            .unwrap_or(home)
            .to_path_buf();
        let font_dir = if macos {
            home.join("Library/Fonts")
        } else {
            data.filter(|p| p.is_absolute())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("fonts")
        };
        Self {
            font: font_dir.join(NAME),
            font_root,
            config_root,
            config: config
                .filter(|p| p.is_absolute())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".config")),
            home: home.to_path_buf(),
            state: state.to_path_buf(),
        }
    }

    // Resolve only caller-selected roots. Descendants remain subject to link checks.
    fn resolved(&self) -> io::Result<Self> {
        let font_root = resolve_root(&self.font_root)?;
        let config_root = resolve_root(&self.config_root)?;
        Ok(Self {
            font: font_root.join(
                self.font
                    .strip_prefix(&self.font_root)
                    .map_err(io::Error::other)?,
            ),
            config: config_root.join(
                self.config
                    .strip_prefix(&self.config_root)
                    .map_err(io::Error::other)?,
            ),
            home: resolve_root(&self.home)?,
            state: resolve_root(&self.state)?,
            font_root,
            config_root,
        })
    }

    fn terminals(&self) -> Vec<(PathBuf, &'static str)> {
        let mut paths = vec![(
            self.config.join("kitty/kitty.conf"),
            "symbol_map U+E1A0-U+E1BA Herdr Agent Icons Max",
        )];
        for directory in [
            self.config.join("ghostty"),
            self.home
                .join("Library/Application Support/com.mitchellh.ghostty"),
        ] {
            for name in ["config", "config.ghostty"] {
                paths.push((
                    directory.join(name),
                    "font-codepoint-map = U+E1A0-U+E1BA=Herdr Agent Icons Max",
                ));
            }
        }
        paths
    }
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

// Missing roots are resolved via their nearest existing ancestor without creating them.
fn resolve_root(root: &Path) -> io::Result<PathBuf> {
    if !root.is_absolute() {
        return Err(conflict("An absolute font action root is required"));
    }
    let mut existing = root;
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(
                    existing
                        .file_name()
                        .ok_or_else(|| conflict("Invalid root component"))?,
                );
                existing = existing
                    .parent()
                    .ok_or_else(|| conflict("Missing root ancestor"))?;
            }
            Err(error) => return Err(error),
        }
    }
    let mut resolved = fs::canonicalize(existing)?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

// Paths use canonical roots; reject links introduced beneath those roots.
fn check_path(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(conflict(
                    "Refusing a symlinked font, configuration, or state path",
                ));
            }
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    check_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| conflict("Missing parent directory"))?;
    let temp = parent.join(format!(
        ".herdr-lineup-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        if let Ok(meta) = fs::metadata(path) {
            file.set_permissions(meta.permissions())?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn load(path: &Path) -> io::Result<Value> {
    check_path(path)?;
    match fs::read(path) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            if value["version"] != 1
                || !value["entries"].is_array()
                || !value["font_owned"].is_boolean()
            {
                return Err(conflict("Invalid font ownership record"));
            }
            Ok(value)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(json!({"version":1,"font_owned":false,"entries":[]}))
        }
        Err(error) => Err(error),
    }
}

fn save(path: &Path, state: &Value) -> io::Result<()> {
    write_atomic(path, &serde_json::to_vec(state).map_err(io::Error::other)?)
}

/// Install only after explicit opt-in. Records ownership separately from font presence.
pub fn install(locations: &Locations) -> io::Result<Vec<String>> {
    apply(locations, false)
}

/// Remove only recorded, byte-identical font and managed blocks.
pub fn uninstall(locations: &Locations) -> io::Result<Vec<String>> {
    apply(locations, true)
}

fn apply(locations: &Locations, removing: bool) -> io::Result<Vec<String>> {
    apply_with(locations, removing, write_atomic)
}

struct FontLock(fs::File);

impl Drop for FontLock {
    fn drop(&mut self) {
        // Close alone can leave flock held by an unrelated fork until exec.
        // Explicit unlock releases the shared open-file description on every exit.
        let _ = self.0.unlock();
    }
}

fn apply_with(
    locations: &Locations,
    removing: bool,
    mut writer: impl FnMut(&Path, &[u8]) -> io::Result<()>,
) -> io::Result<Vec<String>> {
    let locations = locations.resolved()?;
    check_path(&locations.state)?;
    fs::create_dir_all(&locations.state)?;
    let lock_path = locations.state.join("font.lock");
    check_path(&lock_path)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    lock.try_lock().map_err(io::Error::other)?;
    let _lock = FontLock(lock);
    let journal = locations.state.join("font-ownership.json");
    let mut state = load(&journal)?;
    check_path(&locations.font)?;
    if state["font_owned"] == true && state["font_path"].as_str() != locations.font.to_str() {
        return Err(conflict(
            "Font ownership record belongs to a different font directory",
        ));
    }
    if locations.font.exists() && fs::read(&locations.font)? != FONT {
        return Err(conflict("Installed font differs; leaving it unchanged"));
    }
    let terminals = locations.terminals();
    for (path, _) in &terminals {
        check_path(path)?;
    }
    let mut changes = Vec::new();
    for entry in state["entries"].as_array().unwrap() {
        let path = PathBuf::from(
            entry["path"]
                .as_str()
                .ok_or_else(|| conflict("Invalid terminal path record"))?,
        );
        let owned = entry["owned"]
            .as_str()
            .ok_or_else(|| conflict("Invalid terminal block record"))?;
        if !terminals.iter().any(|(candidate, _)| candidate == &path)
            || !owned.contains(START)
            || !owned.contains(END)
        {
            return Err(conflict(
                "Font ownership record does not match this home/config directory",
            ));
        }
        check_path(&path)?;
        if !path.exists() {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        if text.matches(START).count() != 1
            || text.matches(END).count() != 1
            || text.matches(owned).count() != 1
        {
            return Err(conflict(
                "Managed font block was modified; resolve it manually",
            ));
        }
        if removing {
            changes.push((path, text.replacen(owned, "", 1)));
        }
    }
    if !removing {
        for (path, mapping) in terminals {
            check_path(&path)?;
            if !path.exists()
                || state["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry["path"].as_str() == path.to_str())
            {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            if text.contains(START) || text.contains(END) {
                return Err(conflict(
                    "Unrecorded font block exists; leaving it unchanged",
                ));
            }
            let separator = if text.is_empty() || text.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            let owned = format!("{separator}{START}\n{mapping}\n{END}\n");
            state["entries"].as_array_mut().unwrap().push(json!({"path":path.to_str().ok_or_else(|| conflict("Non-UTF-8 config path"))?,"owned":owned}));
            changes.push((path, format!("{text}{owned}")));
        }
    }
    // Preflight all ownership checks before changing any font or terminal bytes.
    let originals: Vec<_> = changes
        .iter()
        .map(|(path, _)| Ok((path.clone(), fs::read(path)?)))
        .collect::<io::Result<_>>()?;
    let font_existed = locations.font.exists();
    let old_journal = fs::read(&journal).ok();
    let mut created_font = false;
    let result = (|| {
        if removing {
            for (path, text) in &changes {
                writer(path, text.as_bytes())?;
            }
            if state["font_owned"] == true && locations.font.exists() {
                fs::remove_file(&locations.font)?;
            }
            state = json!({"version":1,"font_owned":false,"entries":[]});
            save(&journal, &state)
        } else {
            if !font_existed {
                fs::create_dir_all(locations.font.parent().unwrap())?;
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&locations.font)?;
                created_font = true;
                file.write_all(FONT)?;
                file.sync_all()?;
                state["font_owned"] = json!(true);
                state["font_path"] = json!(
                    locations
                        .font
                        .to_str()
                        .ok_or_else(|| conflict("Non-UTF-8 font path"))?
                );
            }
            for (path, text) in &changes {
                writer(path, text.as_bytes())?;
            }
            save(&journal, &state)
        }
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (path, bytes) in originals.iter().rev() {
            if let Err(rollback) = write_atomic(path, bytes) {
                failures.push(rollback.to_string());
            }
        }
        if font_existed && !locations.font.exists() {
            if let Err(rollback) = write_atomic(&locations.font, FONT) {
                failures.push(rollback.to_string());
            }
        } else if created_font
            && locations.font.exists()
            && let Err(rollback) = fs::remove_file(&locations.font)
        {
            failures.push(rollback.to_string());
        }
        if let Some(bytes) = old_journal
            && let Err(rollback) = write_atomic(&journal, &bytes)
        {
            failures.push(rollback.to_string());
        }
        return Err(conflict(&format!(
            "Font action failed: {error}; rollback errors: {failures:?}"
        )));
    }
    Ok(vec![format!("font: {} {}", if removing { "removed owned installation from" } else { "available at" }, locations.font.display()),
        "terminal: map U+E1A0-U+E1BA to Herdr Agent Icons Max. Existing Ghostty/kitty configs use managed blocks; for other terminals configure the mapping manually. Reload the terminal configuration or reopen the terminal. On Linux run fc-cache if needed.".into()])
}

/// Resolve runtime paths only for an explicit install-font/uninstall-font command.
pub fn run(removing: bool) -> io::Result<Vec<String>> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| conflict("An absolute HOME is required"))?;
    let state = env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| conflict("An absolute HERDR_PLUGIN_STATE_DIR is required"))?;
    let data = env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let config = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let locations = Locations::new(
        &home,
        data.as_deref(),
        config.as_deref(),
        &state,
        cfg!(target_os = "macos"),
    );
    apply(&locations, removing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> (tempfile::TempDir, Locations) {
        let root = tempfile::tempdir().unwrap();
        let locations = Locations::new(root.path(), None, None, &root.path().join("state"), false);
        (root, locations)
    }

    #[cfg(unix)]
    struct ForkedLockHolder {
        pid: libc::pid_t,
        release: std::os::unix::net::UnixStream,
    }

    #[cfg(unix)]
    impl ForkedLockHolder {
        fn spawn() -> Self {
            use std::os::fd::AsRawFd;
            let (release, child) = std::os::unix::net::UnixStream::pair().unwrap();
            let parent_fd = release.as_raw_fd();
            let child_fd = child.as_raw_fd();
            // SAFETY: the child uses only async-signal-safe calls and never
            // returns to Rust. A bounded poll prevents an orphaned test child.
            let pid = unsafe {
                let pid = libc::fork();
                if pid == 0 {
                    libc::close(parent_fd);
                    let mut ready = libc::pollfd {
                        fd: child_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    libc::poll(&mut ready, 1, 5_000);
                    libc::_exit(0);
                }
                pid
            };
            assert!(pid > 0, "fork failed: {}", io::Error::last_os_error());
            Self { pid, release }
        }

        fn assert_running(&self) {
            // SAFETY: query only this helper's child without blocking.
            let status = unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), libc::WNOHANG) };
            assert_eq!(status, 0, "fork holder exited before the lock check");
        }
    }

    #[cfg(unix)]
    impl Drop for ForkedLockHolder {
        fn drop(&mut self) {
            let _ = self.release.shutdown(std::net::Shutdown::Both);
            // SAFETY: wait only for the child created by this helper.
            while unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), 0) } < 0 {
                if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn completed_actions_release_locks_inherited_by_an_unrelated_fork() {
        for removing in [false, true] {
            for fail_write in [false, true] {
                let (_root, locations) = fixture();
                let config = locations.config.join("kitty/kitty.conf");
                fs::create_dir_all(config.parent().unwrap()).unwrap();
                fs::write(&config, "# original\n").unwrap();
                if removing {
                    install(&locations).unwrap();
                }
                let mut child = None;
                let result = apply_with(&locations, removing, |path, bytes| {
                    // Fork on another thread while the real action owns its lock.
                    child = Some(std::thread::spawn(ForkedLockHolder::spawn).join().unwrap());
                    if fail_write {
                        Err(conflict("injected write failure"))
                    } else {
                        write_atomic(path, bytes)
                    }
                });
                assert_eq!(result.is_err(), fail_write);
                let child = child.unwrap();
                child.assert_running();
                // The child still has the descriptor open after the action ends.
                let next = install(&locations);
                child.assert_running();
                drop(child);
                next.unwrap();
                uninstall(&locations).unwrap();
                assert_eq!(fs::read_to_string(config).unwrap(), "# original\n");
            }
        }
    }

    #[test]
    fn active_font_action_refuses_competing_actions_without_writes() {
        let (_root, locations) = fixture();
        let config = locations.config.join("kitty/kitty.conf");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "# original\n").unwrap();
        apply_with(&locations, false, |path, bytes| {
            let before = fs::read(path).unwrap();
            assert!(install(&locations).is_err());
            assert!(uninstall(&locations).is_err());
            assert_eq!(fs::read(path).unwrap(), before);
            assert!(!locations.state.join("font-ownership.json").exists());
            write_atomic(path, bytes)
        })
        .unwrap();
        uninstall(&locations).unwrap();
        assert_eq!(fs::read_to_string(config).unwrap(), "# original\n");
    }

    #[test]
    fn install_and_uninstall_preserve_terminal_bytes_and_are_idempotent() {
        let (_root, locations) = fixture();
        let ghostty = locations.config.join("ghostty/config");
        let kitty = locations.config.join("kitty/kitty.conf");
        for path in [&ghostty, &kitty] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "# user settings\nfont-size = 12").unwrap();
        }
        install(&locations).unwrap();
        let first = fs::read(&ghostty).unwrap();
        assert!(String::from_utf8_lossy(&first).contains("U+E1A0-U+E1BA"));
        assert!(!String::from_utf8_lossy(&first).contains("E1C0"));
        install(&locations).unwrap();
        assert_eq!(fs::read(&ghostty).unwrap(), first);
        fs::write(
            &kitty,
            format!(
                "{}\n# later user edit\n",
                fs::read_to_string(&kitty).unwrap()
            ),
        )
        .unwrap();
        uninstall(&locations).unwrap();
        assert_eq!(
            fs::read_to_string(&ghostty).unwrap(),
            "# user settings\nfont-size = 12"
        );
        assert_eq!(
            fs::read_to_string(&kitty).unwrap(),
            "# user settings\nfont-size = 12\n# later user edit\n"
        );
        uninstall(&locations).unwrap();
        assert!(!locations.font.exists());
    }

    #[test]
    fn preexisting_font_is_never_removed_and_missing_configs_are_not_created() {
        let (_root, locations) = fixture();
        fs::create_dir_all(locations.font.parent().unwrap()).unwrap();
        fs::write(&locations.font, FONT).unwrap();
        install(&locations).unwrap();
        uninstall(&locations).unwrap();
        assert_eq!(fs::read(&locations.font).unwrap(), FONT);
        assert!(!locations.config.join("ghostty/config").exists());
    }

    #[test]
    fn modified_managed_block_refuses_without_changes() {
        let (_root, locations) = fixture();
        let path = locations.config.join("ghostty/config");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# original\n").unwrap();
        install(&locations).unwrap();
        let modified = fs::read_to_string(&path)
            .unwrap()
            .replace("Herdr Agent Icons Max", "User Font");
        fs::write(&path, &modified).unwrap();
        assert!(uninstall(&locations).is_err());
        assert!(install(&locations).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), modified);
        assert!(locations.font.exists());
    }

    #[test]
    fn changed_font_directory_and_modified_font_are_preserved() {
        let (root, locations) = fixture();
        install(&locations).unwrap();
        let other = Locations::new(
            root.path(),
            Some(&root.path().join("other-data")),
            None,
            &locations.state,
            false,
        );
        fs::create_dir_all(other.font.parent().unwrap()).unwrap();
        fs::write(&other.font, FONT).unwrap();
        assert!(uninstall(&other).is_err());
        assert!(locations.font.exists());
        assert!(other.font.exists());
        fs::write(&locations.font, b"user replaced font").unwrap();
        assert!(uninstall(&locations).is_err());
        assert_eq!(fs::read(&locations.font).unwrap(), b"user replaced font");
    }

    #[test]
    fn unrecorded_markers_and_invalid_terminal_files_refuse_before_font_creation() {
        let (_root, locations) = fixture();
        let path = locations.config.join("ghostty/config");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("{START}\nuser mapping\n{END}\n")).unwrap();
        assert!(install(&locations).is_err());
        assert!(!locations.font.exists());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(install(&locations).is_err());
        assert!(!locations.font.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_configs_are_never_followed() {
        let (root, locations) = fixture();
        let outside = root.path().join("outside");
        fs::write(&outside, "untouched").unwrap();
        let path = locations.config.join("ghostty/config");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(install(&locations).is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "untouched");
        assert!(!locations.font.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_home_ancestor_allows_install_and_uninstall() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir_all(real.join("home")).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        for macos in [false, true] {
            let home = alias.join("home");
            let locations = Locations::new(&home, None, None, &home.join("state"), macos);
            let config = locations.config.join("kitty/kitty.conf");
            fs::create_dir_all(config.parent().unwrap()).unwrap();
            fs::write(&config, "# original\n").unwrap();
            install(&locations).unwrap();
            assert!(locations.font.exists());
            uninstall(&locations).unwrap();
            assert!(!locations.font.exists());
            assert_eq!(fs::read_to_string(config).unwrap(), "# original\n");
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicit_xdg_and_state_roots_resolve_links_without_creating_missing_configs() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        fs::create_dir(&real).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let locations = Locations::new(
            root.path(),
            Some(&alias.join("data")),
            Some(&alias.join("config")),
            &alias.join("state"),
            false,
        );
        install(&locations).unwrap();
        assert!(real.join("data/fonts").join(NAME).exists());
        assert!(!real.join("config").exists());
        uninstall(&locations).unwrap();
        assert!(!locations.font.exists());
    }

    #[cfg(unix)]
    #[test]
    fn installed_target_replaced_by_symlink_is_preserved() {
        for target in ["font", "config", "journal", "lock"] {
            let (root, locations) = fixture();
            let config = locations.config.join("kitty/kitty.conf");
            fs::create_dir_all(config.parent().unwrap()).unwrap();
            fs::write(&config, "# original\n").unwrap();
            install(&locations).unwrap();
            let path = match target {
                "font" => locations.font.clone(),
                "config" => config.clone(),
                "journal" => locations.state.join("font-ownership.json"),
                _ => locations.state.join("font.lock"),
            };
            let outside = root.path().join("outside");
            fs::rename(&path, &outside).unwrap();
            let before = fs::read(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, &path).unwrap();
            let font_before = fs::read(&locations.font).unwrap();
            let config_before = fs::read(&config).unwrap();
            assert!(uninstall(&locations).is_err(), "{target}");
            assert!(install(&locations).is_err(), "{target}");
            assert_eq!(fs::read(&outside).unwrap(), before);
            assert_eq!(fs::read(&locations.font).unwrap(), font_before);
            assert_eq!(fs::read(&config).unwrap(), config_before);
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_targets_and_directories_below_roots_are_rejected() {
        for target in [
            "font",
            "config",
            "journal",
            "lock",
            "font-parent",
            "config-parent",
        ] {
            let (root, locations) = fixture();
            let outside = root.path().join("outside");
            let path = match target {
                "font" => locations.font.clone(),
                "config" => locations.config.join("kitty/kitty.conf"),
                "journal" => locations.state.join("font-ownership.json"),
                "lock" => locations.state.join("font.lock"),
                "font-parent" => locations.font.parent().unwrap().to_path_buf(),
                _ => locations.config.join("kitty"),
            };
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if target.ends_with("parent") {
                fs::create_dir(&outside).unwrap();
            } else {
                fs::write(&outside, "untouched").unwrap();
            }
            std::os::unix::fs::symlink(&outside, &path).unwrap();
            assert!(install(&locations).is_err(), "{target}");
            assert!(uninstall(&locations).is_err(), "{target}");
            if outside.is_file() {
                assert_eq!(fs::read_to_string(outside).unwrap(), "untouched");
            } else {
                assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
            }
        }
    }

    #[test]
    fn write_failure_restores_original_terminal_bytes_and_font_ownership() {
        let (_root, locations) = fixture();
        let ghostty = locations.config.join("ghostty/config");
        let kitty = locations.config.join("kitty/kitty.conf");
        for path in [&ghostty, &kitty] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "# original\n").unwrap();
        }
        let mut writes = 0;
        assert!(
            apply_with(&locations, false, |path, bytes| {
                writes += 1;
                if writes == 2 {
                    Err(conflict("injected write failure"))
                } else {
                    write_atomic(path, bytes)
                }
            })
            .is_err()
        );
        assert_eq!(fs::read_to_string(&ghostty).unwrap(), "# original\n");
        assert_eq!(fs::read_to_string(&kitty).unwrap(), "# original\n");
        assert!(!locations.font.exists());
        install(&locations).unwrap();
        let before = fs::read(&ghostty).unwrap();
        let mut writes = 0;
        assert!(
            apply_with(&locations, true, |path, bytes| {
                writes += 1;
                if writes == 2 {
                    Err(conflict("injected write failure"))
                } else {
                    write_atomic(path, bytes)
                }
            })
            .is_err()
        );
        assert_eq!(fs::read(&ghostty).unwrap(), before);
        assert!(locations.font.exists());
        uninstall(&locations).unwrap();
    }

    #[test]
    fn xdg_and_macos_locations_are_explicit() {
        let home = std::path::Path::new("/fake/home");
        let data = std::path::Path::new("/fake/data");
        let config = std::path::Path::new("/fake/config");
        let state = std::path::Path::new("/fake/state");
        let linux = Locations::new(home, Some(data), Some(config), state, false);
        assert_eq!(linux.font.parent().unwrap(), data.join("fonts"));
        assert_eq!(linux.config, config);
        let mac = Locations::new(home, Some(data), Some(config), state, true);
        assert_eq!(mac.font.parent().unwrap(), home.join("Library/Fonts"));
    }
}
