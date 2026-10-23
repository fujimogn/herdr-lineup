//! Explicit per-user icon font installation; never invoked at startup.

use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
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
                return Err(conflict(&format!(
                    "Refusing symlinked font, configuration, or state path: {}",
                    ancestor.display()
                )));
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

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
    device: u64,
    inode: u64,
    bytes: Vec<u8>,
}

fn stamp(path: &Path) -> io::Result<Option<FileStamp>> {
    check_path(path)?;
    match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(mut file) => {
            let meta = file.metadata()?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(Some(FileStamp {
                device: meta.dev(),
                inode: meta.ino(),
                bytes,
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn require_bytes(path: &Path, expected: Option<&[u8]>) -> io::Result<()> {
    if stamp(path)?.as_ref().map(|file| file.bytes.as_slice()) != expected {
        return Err(conflict(&format!(
            "Concurrent change at {}; leaving it unchanged",
            path.display()
        )));
    }
    Ok(())
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
    writer: impl FnMut(&Path, &[u8]) -> io::Result<()>,
) -> io::Result<Vec<String>> {
    apply_with_font(locations, removing, writer, |file, bytes| {
        file.write_all(bytes)?;
        file.sync_all()
    })
}

fn apply_with_font(
    locations: &Locations,
    removing: bool,
    writer: impl FnMut(&Path, &[u8]) -> io::Result<()>,
    font_writer: impl FnMut(&mut fs::File, &[u8]) -> io::Result<()>,
) -> io::Result<Vec<String>> {
    apply_with_font_and_remove(locations, removing, writer, font_writer, |_| Ok(()))
}

fn apply_with_font_and_remove(
    locations: &Locations,
    removing: bool,
    mut writer: impl FnMut(&Path, &[u8]) -> io::Result<()>,
    mut font_writer: impl FnMut(&mut fs::File, &[u8]) -> io::Result<()>,
    mut before_font_remove: impl FnMut(&Path) -> io::Result<()>,
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
    if state.get("pending").is_some() {
        return Err(conflict(&format!(
            "Font ownership is unresolved at {}; inspect the recorded paths and current files manually before another action",
            journal.display()
        )));
    }
    let original_state = state.clone();
    check_path(&locations.font)?;
    if state["font_owned"] == true && state["font_path"].as_str() != locations.font.to_str() {
        return Err(conflict(
            "Font ownership record belongs to a different font directory",
        ));
    }
    let original_font = stamp(&locations.font)?;
    if original_font
        .as_ref()
        .is_some_and(|file| file.bytes != FONT)
    {
        return Err(conflict(&format!(
            "Installed font differs at {}; leaving it unchanged",
            locations.font.display()
        )));
    }
    let terminals = locations.terminals();
    for (path, _) in &terminals {
        check_path(path)?;
    }
    let mut changes = Vec::new();
    let mut retained = Vec::new();
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
            retained.push(entry.clone());
            continue;
        }
        let original = fs::read(&path)?;
        let text = std::str::from_utf8(&original).map_err(io::Error::other)?;
        if !text.contains(START) && !text.contains(END) {
            continue;
        }
        if text.matches(START).count() != 1
            || text.matches(END).count() != 1
            || text.matches(owned).count() != 1
        {
            return Err(conflict(&format!(
                "Managed font block was modified at {}; resolve it manually",
                path.display()
            )));
        }
        if !removing {
            retained.push(entry.clone());
        }
        if removing {
            changes.push((path, text.replacen(owned, "", 1), original));
        }
    }
    state["entries"] = json!(retained);
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
            let original = fs::read(&path)?;
            let text = std::str::from_utf8(&original).map_err(io::Error::other)?;
            if text.contains(START) || text.contains(END) {
                return Err(conflict(&format!(
                    "Unrecorded font block exists at {}; leaving it unchanged",
                    path.display()
                )));
            }
            let separator = if text.is_empty() || text.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            let owned = format!("{separator}{START}\n{mapping}\n{END}\n");
            state["entries"].as_array_mut().unwrap().push(json!({"path":path.to_str().ok_or_else(|| conflict("Non-UTF-8 config path"))?,"owned":owned}));
            changes.push((path, format!("{text}{owned}"), original));
        }
        if original_font.is_none() {
            state["font_owned"] = json!(true);
            state["font_path"] = json!(
                locations
                    .font
                    .to_str()
                    .ok_or_else(|| conflict("Non-UTF-8 font path"))?
            );
        }
    }
    let font_existed = original_font.is_some();
    let old_journal = match fs::read(&journal) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut changed_paths = Vec::new();
    let mut affected_paths: Vec<PathBuf> =
        changes.iter().map(|(path, _, _)| path.clone()).collect();
    if (!removing && !font_existed) || (removing && state["font_owned"] == true && font_existed) {
        affected_paths.push(locations.font.clone());
    }
    let mut action_path = journal.clone();
    let result: io::Result<()> = (|| {
        // The pending record is durable before any font or terminal mutation.
        // An interrupted or failed action remains blocked for manual inspection.
        let mut pending_state = if removing {
            original_state.clone()
        } else {
            state.clone()
        };
        let final_state = if removing {
            json!({"version":1,"font_owned":false,"entries":state["entries"]})
        } else {
            state.clone()
        };
        pending_state["pending"] = json!({
            "action": if removing { "uninstall" } else { "install" },
            "paths": affected_paths.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>()
        });
        let pending_journal = serde_json::to_vec(&pending_state).map_err(io::Error::other)?;
        let next_journal = serde_json::to_vec(&final_state).map_err(io::Error::other)?;
        let needs_write = !affected_paths.is_empty()
            || old_journal
                .as_deref()
                .is_some_and(|old| old != next_journal.as_slice());
        if needs_write {
            require_bytes(&journal, old_journal.as_deref())?;
            write_atomic(&journal, &pending_journal)?;
            changed_paths.push(journal.clone());
        }
        if removing {
            for (path, text, original) in &changes {
                action_path = path.clone();
                if &fs::read(path)? != original {
                    return Err(conflict(&format!(
                        "Configuration file was modified concurrently at {}; leaving it unchanged",
                        path.display()
                    )));
                }
                writer(path, text.as_bytes())?;
                require_bytes(path, Some(text.as_bytes()))?;
                changed_paths.push(path.clone());
            }
            if state["font_owned"] == true && font_existed {
                action_path = locations.font.clone();
                before_font_remove(&locations.font)?;
                if stamp(&locations.font)? != original_font {
                    return Err(conflict(
                        "Installed font changed before removal; leaving it unchanged",
                    ));
                }
                fs::remove_file(&locations.font)?;
                changed_paths.push(locations.font.clone());
            }
        } else {
            if !font_existed {
                action_path = locations.font.clone();
                fs::create_dir_all(locations.font.parent().unwrap())?;
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&locations.font)?;
                changed_paths.push(locations.font.clone());
                font_writer(&mut file, FONT)?;
                require_bytes(&locations.font, Some(FONT))?;
            }
            for (path, text, original) in &changes {
                action_path = path.clone();
                if &fs::read(path)? != original {
                    return Err(conflict(&format!(
                        "Configuration file was modified concurrently at {}; leaving it unchanged",
                        path.display()
                    )));
                }
                writer(path, text.as_bytes())?;
                require_bytes(path, Some(text.as_bytes()))?;
                changed_paths.push(path.clone());
            }
        }
        if needs_write {
            action_path = journal.clone();
            require_bytes(&journal, Some(&pending_journal))?;
            write_atomic(&journal, &next_journal)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        return Err(conflict(&format!(
            "Font action failed at {}: {error}; partial state or a conflict is possible; no automatic rollback or deletion was attempted; ownership journal: {}; inspect these paths manually before another action: {}",
            action_path.display(),
            journal.display(),
            affected_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let removed_owned_file = changed_paths.iter().any(|path| path != &journal);
    let mut output = vec![format!(
        "font: {} {}",
        if removing && removed_owned_file {
            "removed owned installation from"
        } else if removing {
            "nothing managed was removed from"
        } else {
            "available at"
        },
        locations.font.display()
    )];
    output.extend(
        changed_paths
            .into_iter()
            .map(|path| format!("changed: {}", path.display())),
    );
    output.push("terminal: map U+E1A0-U+E1BA to Herdr Agent Icons Max. Existing Ghostty/kitty configs use managed blocks; for other terminals configure the mapping manually. Reload the terminal configuration or reopen the terminal. On Linux run fc-cache if needed.".into());
    Ok(output)
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

    #[test]
    fn uninstall_without_owned_files_reports_no_installation() {
        let (_root, locations) = fixture();
        let output = uninstall(&locations).unwrap().join("\n");
        assert!(output.contains("nothing managed"), "{output}");
        assert!(!output.contains("removed owned installation"), "{output}");
        assert!(!output.contains("changed:"), "{output}");
    }

    #[test]
    fn repeated_actions_report_only_changed_bytes() {
        let (_root, locations) = fixture();
        assert!(install(&locations).unwrap().join("\n").contains("changed:"));
        assert!(!install(&locations).unwrap().join("\n").contains("changed:"));
        assert!(
            uninstall(&locations)
                .unwrap()
                .join("\n")
                .contains("changed:")
        );
        assert!(
            !uninstall(&locations)
                .unwrap()
                .join("\n")
                .contains("changed:")
        );
    }

    #[test]
    fn conflicts_identify_the_affected_path() {
        let (_root, locations) = fixture();
        fs::create_dir_all(locations.font.parent().unwrap()).unwrap();
        fs::write(&locations.font, b"different font").unwrap();
        assert!(
            install(&locations)
                .unwrap_err()
                .to_string()
                .contains(&locations.font.display().to_string())
        );
        fs::remove_file(&locations.font).unwrap();

        let path = locations.config.join("kitty/kitty.conf");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("{START}\nuser map\n{END}\n")).unwrap();
        assert!(
            install(&locations)
                .unwrap_err()
                .to_string()
                .contains(&path.display().to_string())
        );
        fs::write(&path, "# original\n").unwrap();
        install(&locations).unwrap();
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("Herdr Agent Icons Max", "Other Font"),
        )
        .unwrap();
        assert!(
            uninstall(&locations)
                .unwrap_err()
                .to_string()
                .contains(&path.display().to_string())
        );
    }

    #[test]
    fn temporarily_missing_config_survives_uninstall_and_can_be_cleaned_later() {
        let (_root, locations) = fixture();
        let path = locations.config.join("kitty/kitty.conf");
        let parked = locations.config.join("kitty/kitty.conf.parked");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# original\n").unwrap();
        install(&locations).unwrap();
        fs::rename(&path, &parked).unwrap();
        uninstall(&locations).unwrap();
        fs::rename(&parked, &path).unwrap();
        install(&locations).unwrap();
        uninstall(&locations).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "# original\n");
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
                if fail_write {
                    assert!(next.is_err());
                    assert!(locations.state.join("font-ownership.json").exists());
                } else {
                    next.unwrap();
                    uninstall(&locations).unwrap();
                    assert_eq!(fs::read_to_string(config).unwrap(), "# original\n");
                }
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
            assert!(locations.state.join("font-ownership.json").exists());
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
    fn uninstall_clears_journal_when_owned_font_is_already_missing() {
        let (_root, locations) = fixture();
        install(&locations).unwrap();
        let journal = locations.state.join("font-ownership.json");
        assert_eq!(load(&journal).unwrap()["font_owned"], true);
        fs::remove_file(&locations.font).unwrap();

        let output = uninstall(&locations).unwrap().join("\n");
        assert!(output.contains(&journal.display().to_string()));
        let state = load(&journal).unwrap();
        assert_eq!(state["font_owned"], false);
        assert!(state.get("pending").is_none());
        assert!(!locations.font.exists());
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
        let error = install(&locations).unwrap_err();
        assert!(error.to_string().contains(&path.display().to_string()));
        assert_eq!(fs::read_to_string(outside).unwrap(), "untouched");
        assert!(!locations.font.exists());
    }

    #[test]
    fn deleted_managed_block_can_be_reinstalled_or_uninstalled() {
        for removing in [false, true] {
            let (_root, locations) = fixture();
            let path = locations.config.join("kitty/kitty.conf");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "# original\n").unwrap();
            install(&locations).unwrap();
            fs::write(&path, "# original\n").unwrap();
            if removing {
                uninstall(&locations).unwrap();
                assert_eq!(fs::read_to_string(&path).unwrap(), "# original\n");
                assert!(!locations.font.exists());
            } else {
                install(&locations).unwrap();
                assert!(fs::read_to_string(&path).unwrap().contains(START));
                uninstall(&locations).unwrap();
            }
        }
    }

    #[test]
    fn temporarily_missing_config_keeps_ownership_on_install() {
        let (_root, locations) = fixture();
        let path = locations.config.join("kitty/kitty.conf");
        let parked = locations.config.join("kitty/kitty.conf.parked");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# original\n").unwrap();
        install(&locations).unwrap();
        fs::rename(&path, &parked).unwrap();
        install(&locations).unwrap();
        fs::rename(&parked, &path).unwrap();
        uninstall(&locations).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "# original\n");
    }

    #[test]
    fn interrupted_install_requires_manual_recovery() {
        for removing in [false, true] {
            let (_root, locations) = fixture();
            let path = locations.config.join("kitty/kitty.conf");
            let other = locations.config.join("ghostty/config");
            for candidate in [&path, &other] {
                fs::create_dir_all(candidate.parent().unwrap()).unwrap();
                fs::write(candidate, "# original\n").unwrap();
            }
            let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                apply_with(&locations, false, |path, bytes| {
                    write_atomic(path, bytes)?;
                    panic!("simulated crash after terminal write");
                })
            }));
            assert!(interrupted.is_err());
            assert!(fs::read_to_string(&path).unwrap().contains(START));
            let journal = locations.state.join("font-ownership.json");
            assert!(fs::read_to_string(&journal).unwrap().contains("pending"));
            let result = if removing {
                uninstall(&locations)
            } else {
                install(&locations)
            };
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains(&journal.display().to_string())
            );
            assert_eq!(fs::read_to_string(&other).unwrap(), "# original\n");
        }
    }

    #[test]
    fn actions_report_each_changed_file() {
        let (_root, locations) = fixture();
        let path = locations.config.join("kitty/kitty.conf");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# original\n").unwrap();
        let install_output = install(&locations).unwrap().join("\n");
        assert!(install_output.contains(&path.display().to_string()));
        assert!(
            install_output.contains(
                &locations
                    .state
                    .join("font-ownership.json")
                    .display()
                    .to_string()
            )
        );
        let uninstall_output = uninstall(&locations).unwrap().join("\n");
        assert!(uninstall_output.contains(&path.display().to_string()));
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
    fn failed_install_and_uninstall_keep_journal_and_earlier_writes() {
        for removing in [false, true] {
            let (_root, locations) = fixture();
            let kitty = locations.config.join("kitty/kitty.conf");
            let ghostty = locations.config.join("ghostty/config");
            for path in [&kitty, &ghostty] {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, "# original\n").unwrap();
            }
            if removing {
                install(&locations).unwrap();
            }
            let mut writes = 0;
            let journal = locations.state.join("font-ownership.json");
            let result = apply_with(&locations, removing, |path, bytes| {
                assert_eq!(
                    load(&journal).unwrap()["pending"]["action"],
                    if removing { "uninstall" } else { "install" }
                );
                writes += 1;
                if writes == 2 {
                    Err(conflict("injected terminal write failure"))
                } else {
                    write_atomic(path, bytes)
                }
            });
            let message = result.unwrap_err().to_string();
            assert!(
                message.contains(&ghostty.display().to_string()),
                "{message}"
            );
            let state = load(&journal).unwrap();
            assert_eq!(
                state["pending"]["action"],
                if removing { "uninstall" } else { "install" }
            );
            assert!(
                state["pending"]["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|path| path == &json!(ghostty.to_string_lossy()))
            );
            assert!(message.contains(&journal.display().to_string()));
            assert_eq!(
                fs::read_to_string(&ghostty).unwrap().contains(START),
                removing
            );
            assert_eq!(
                fs::read_to_string(&kitty).unwrap().contains(START),
                !removing
            );
            assert!(locations.font.exists());
            let before = fs::read(&kitty).unwrap();
            assert!(install(&locations).is_err());
            assert!(uninstall(&locations).is_err());
            assert_eq!(fs::read(&kitty).unwrap(), before);
        }
    }

    #[test]
    fn partial_terminal_write_and_external_edit_are_retained() {
        for removing in [false, true] {
            let (_root, locations) = fixture();
            let kitty = locations.config.join("kitty/kitty.conf");
            let ghostty = locations.config.join("ghostty/config");
            for path in [&kitty, &ghostty] {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, "# original\n").unwrap();
            }
            if removing {
                install(&locations).unwrap();
            }
            let result = apply_with(&locations, removing, |path, bytes| {
                if path == ghostty {
                    fs::write(path, b"partial terminal write")?;
                    return Err(conflict("injected partial write"));
                }
                write_atomic(path, bytes)
            });
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains(&ghostty.display().to_string())
            );
            assert_eq!(fs::read(&ghostty).unwrap(), b"partial terminal write");
            assert!(locations.state.join("font-ownership.json").exists());
            assert!(install(&locations).is_err());
        }

        let (_root, locations) = fixture();
        let kitty = locations.config.join("kitty/kitty.conf");
        let ghostty = locations.config.join("ghostty/config");
        for path in [&kitty, &ghostty] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "# original\n").unwrap();
        }
        let result = apply_with(&locations, false, |path, bytes| {
            write_atomic(path, bytes)?;
            if path == kitty {
                fs::write(&ghostty, b"# external edit\n")?;
            }
            Ok(())
        });
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains(&ghostty.display().to_string()),
            "{message}"
        );
        assert_eq!(fs::read(&ghostty).unwrap(), b"# external edit\n");
        assert!(fs::read_to_string(&kitty).unwrap().contains(START));
        assert!(locations.font.exists());
        assert!(load(&locations.state.join("font-ownership.json")).unwrap()["pending"].is_object());
    }

    #[test]
    fn journal_change_at_completion_preserves_partial_installation() {
        let (_root, locations) = fixture();
        let kitty = locations.config.join("kitty/kitty.conf");
        fs::create_dir_all(kitty.parent().unwrap()).unwrap();
        fs::write(&kitty, "# original\n").unwrap();
        let journal = locations.state.join("font-ownership.json");
        let result = apply_with(&locations, false, |path, bytes| {
            write_atomic(path, bytes)?;
            fs::write(&journal, b"external journal")
        });
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains(&journal.display().to_string()),
            "{message}"
        );
        assert_eq!(fs::read(&journal).unwrap(), b"external journal");
        assert!(locations.font.exists());
        assert!(fs::read_to_string(&kitty).unwrap().contains(START));
        assert!(install(&locations).is_err());
    }

    #[test]
    fn invalid_journal_preflight_does_not_start_installation() {
        let (_root, locations) = fixture();
        let kitty = locations.config.join("kitty/kitty.conf");
        fs::create_dir_all(kitty.parent().unwrap()).unwrap();
        fs::write(&kitty, "# original\n").unwrap();
        fs::create_dir_all(&locations.state).unwrap();
        let journal = locations.state.join("font-ownership.json");
        fs::write(&journal, b"invalid journal").unwrap();
        assert!(install(&locations).is_err());
        assert_eq!(fs::read(&journal).unwrap(), b"invalid journal");
        assert!(!locations.font.exists());
        assert_eq!(fs::read_to_string(&kitty).unwrap(), "# original\n");
    }

    #[test]
    fn same_byte_font_replacement_before_removal_is_preserved() {
        let (_root, locations) = fixture();
        let kitty = locations.config.join("kitty/kitty.conf");
        fs::create_dir_all(kitty.parent().unwrap()).unwrap();
        fs::write(&kitty, "# original\n").unwrap();
        install(&locations).unwrap();
        let result = apply_with_font_and_remove(
            &locations,
            true,
            write_atomic,
            |file, bytes| file.write_all(bytes),
            |path| {
                let replacement = path.with_extension("replacement");
                fs::write(&replacement, FONT)?;
                fs::rename(replacement, path)
            },
        );
        let message = result.unwrap_err().to_string();
        assert!(message.contains("font changed before removal"), "{message}");
        assert_eq!(fs::read(&locations.font).unwrap(), FONT);
        assert_eq!(fs::read_to_string(&kitty).unwrap(), "# original\n");
        assert!(load(&locations.state.join("font-ownership.json")).unwrap()["pending"].is_object());
    }

    #[test]
    fn partial_font_write_or_sync_failure_keeps_font_and_journal() {
        for fail_stage in ["partial_write", "sync"] {
            let (_root, locations) = fixture();
            let kitty = locations.config.join("kitty/kitty.conf");
            fs::create_dir_all(kitty.parent().unwrap()).unwrap();
            fs::write(&kitty, "# original\n").unwrap();
            let result = apply_with_font(&locations, false, write_atomic, |file, bytes| {
                if fail_stage == "partial_write" {
                    file.write_all(b"partial corrupted font")?;
                    Err(conflict("injected font write failure"))
                } else {
                    file.write_all(bytes)?;
                    Err(conflict("injected font sync failure"))
                }
            });
            let message = result.unwrap_err().to_string();
            assert!(
                message.contains(&locations.font.display().to_string()),
                "{message}"
            );
            assert_eq!(
                fs::read(&locations.font).unwrap(),
                if fail_stage == "partial_write" {
                    b"partial corrupted font".as_slice()
                } else {
                    FONT
                }
            );
            assert!(
                load(&locations.state.join("font-ownership.json")).unwrap()["pending"].is_object()
            );
            assert_eq!(fs::read_to_string(&kitty).unwrap(), "# original\n");
            assert!(install(&locations).is_err());
        }
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
