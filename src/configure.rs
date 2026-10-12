//! Opt-in sidebar configuration transactions.

use std::ffi::OsStr;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use toml_edit::{Array, DocumentMut, InlineTable, Item, value};

const BEGIN: &str = "# BEGIN herdr-lineup managed sidebar v1";
const END: &str = "# END herdr-lineup managed sidebar v1";

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Configure,
    Unconfigure,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Unchanged,
    Changed { backup: std::path::PathBuf },
}

/// Resolve Herdr's Unix config location without reading user configuration.
pub fn config_path(override_path: Option<&OsStr>, home: Option<&OsStr>) -> Result<PathBuf, String> {
    if let Some(path) = override_path {
        if path.is_empty() {
            return Err("HERDR_CONFIG_PATH must not be empty".into());
        }
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            Ok(path)
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .map_err(|_| "cannot resolve relative HERDR_CONFIG_PATH".into())
        };
    }
    let home = home
        .filter(|home| !home.is_empty())
        .ok_or("HOME is missing")?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err("HOME must be an absolute path".into());
    }
    Ok(home.join(".config/herdr/config.toml"))
}

/// Only the explicitly selected configure/unconfigure CLI commands call this.
pub fn run(action: Action) -> Result<Outcome, String> {
    let path = config_path(
        std::env::var_os("HERDR_CONFIG_PATH").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )?;
    eprintln!("Herdr configuration target: {}", path.display());
    let binary = std::env::var_os("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".into());
    let state = match std::env::var_os("HERDR_PLUGIN_STATE_DIR") {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => {
            let home = std::env::var_os("HOME").ok_or("HOME is missing")?;
            PathBuf::from(home).join(".local/state/herdr-lineup")
        }
    };
    if !state.is_absolute() {
        return Err("plugin state directory must be absolute".into());
    }
    apply_validated(
        &path,
        &state,
        action,
        |candidate| herdr_command(&binary, ["config", "check"], candidate),
        |target| herdr_command(&binary, ["server", "reload-config"], target),
    )
}

fn herdr_command(binary: &OsStr, args: [&str; 2], target: &Path) -> Result<(), String> {
    let label = format!("herdr {} {}", args[0], args[1]);
    let status = Command::new(binary)
        .args(args)
        .env("HERDR_CONFIG_PATH", target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| format!("cannot execute {label}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{label} failed ({status}); run config check manually for diagnostics"
        ))
    }
}

/// Edit only an exact owned block. The injected reload callback makes all
/// transaction tests independent of the user's running Herdr session.
pub fn apply(
    path: &Path,
    state: &Path,
    action: Action,
    mut reload: impl FnMut(&Path) -> Result<(), String>,
) -> Result<Outcome, String> {
    apply_validated(path, state, action, |_| Ok(()), &mut reload)
}

/// Validate the staged candidate before creating a backup or replacing config.
pub fn apply_validated(
    path: &Path,
    state: &Path,
    action: Action,
    mut validate: impl FnMut(&Path) -> Result<(), String>,
    mut reload: impl FnMut(&Path) -> Result<(), String>,
) -> Result<Outcome, String> {
    let _guard = transaction_lock(path, state)?;
    let before = read_regular(path)?;
    let text = std::str::from_utf8(before.as_deref().unwrap_or_default())
        .map_err(|_| "Herdr config must be UTF-8".to_owned())?;
    let candidate = plan(text, action)?;
    let Some(candidate) = candidate else {
        return Ok(Outcome::Unchanged);
    };
    let parent = path.parent().ok_or("config target has no parent")?;
    fs::create_dir_all(parent).map_err(|_| "cannot create config directory")?;
    let mut staged =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "cannot stage proposed config")?;
    staged
        .write_all(candidate.as_bytes())
        .map_err(|_| "cannot stage proposed config")?;
    staged
        .as_file()
        .sync_all()
        .map_err(|_| "cannot flush proposed config")?;
    validate(staged.path()).map_err(|error| {
        format!("candidate validation failed: {error}; no config replacement made")
    })?;
    let permissions = if before.is_some() {
        Some(
            fs::metadata(path)
                .map_err(|_| "cannot inspect config permissions")?
                .permissions(),
        )
    } else {
        None
    };
    // NamedTempFile creates a unique file exclusively; keeping it preserves
    // the non-overwriting backup even if replacement or reload later fails.
    let name = path
        .file_name()
        .ok_or("config target has no filename")?
        .to_string_lossy();
    let mut backup = tempfile::Builder::new()
        .prefix(&format!("{name}."))
        .suffix(".bak")
        .tempfile_in(parent)
        .map_err(|_| "cannot create config backup")?;
    if let Some(permissions) = &permissions {
        backup
            .as_file()
            .set_permissions(permissions.clone())
            .map_err(|_| "cannot set backup permissions")?;
    }
    backup
        .write_all(before.as_deref().unwrap_or_default())
        .map_err(|_| "cannot write config backup")?;
    backup
        .as_file()
        .sync_all()
        .map_err(|_| "cannot flush config backup")?;
    let (_, backup) = backup.keep().map_err(|_| "cannot retain config backup")?;
    if read_regular(path)? != before {
        return Err("config changed concurrently; no replacement made".into());
    }
    atomic_write(path, candidate.as_bytes(), permissions.as_ref())?;
    if let Err(first) = reload(path) {
        // Never overwrite an intervening external edit during rollback.
        if read_regular(path)?.as_deref() != Some(candidate.as_bytes()) {
            return Err(format!(
                "reload failed: {first}; rollback refused: config changed concurrently; backup: {}",
                backup.display()
            ));
        }
        let restored = if let Some(bytes) = &before {
            atomic_write(path, bytes, permissions.as_ref())
        } else {
            fs::remove_file(path).map_err(|_| "cannot restore absent config".to_owned())
        };
        restored.map_err(|error| {
            format!(
                "reload failed: {first}; rollback failed: {error}; backup: {}",
                backup.display()
            )
        })?;
        return match reload(path) {
            Ok(()) => Err(format!("reload failed: {first}; original config restored")),
            Err(second) => Err(format!(
                "reload failed: {first}; original config restored; restored config reload failed: {second}"
            )),
        };
    }
    Ok(Outcome::Changed { backup })
}

struct TransactionLock(fs::File);

impl Drop for TransactionLock {
    fn drop(&mut self) {
        // Explicit unlock also releases a descriptor briefly inherited by an
        // unrelated concurrent fork before its close-on-exec takes effect.
        let _ = self.0.unlock();
    }
}

fn transaction_lock(path: &Path, state: &Path) -> Result<TransactionLock, String> {
    if !path.is_absolute() || !state.is_absolute() {
        return Err("config and state paths must be absolute".into());
    }
    // A stable sidecar inode survives config's atomic replacement and remains
    // locked through validation, replacement, reload, and rollback. Never
    // unlink it: doing so would split future callers across different locks.
    // Advisory locks serialize Lineup actions. Non-cooperating external
    // editors still require the optimistic byte checks around replacement.
    // Canonicalize the nearest existing ancestor too, so an absent file keeps
    // the same lock identity after creation (including symlinked parents).
    let canonical = path
        .ancestors()
        .find_map(|ancestor| {
            ancestor
                .canonicalize()
                .ok()
                .map(|root| root.join(path.strip_prefix(ancestor).expect("path ancestor")))
        })
        .ok_or("cannot resolve config lock identity")?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hash);
    fs::create_dir_all(state).map_err(|_| "cannot create plugin state directory")?;
    let lock_path = state.join(format!("configure-{:016x}.lock", hash.finish()));
    if let Ok(metadata) = fs::symlink_metadata(&lock_path)
        && !metadata.is_file()
    {
        return Err("configuration lock must be a regular file".into());
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|_| "cannot open configuration lock")?;
    lock.try_lock()
        .map_err(|_| "another configuration action owns this target; retry later")?;
    Ok(TransactionLock(lock))
}

fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => fs::read(path).map(Some).map_err(|_| "cannot read Herdr config".into()),
        Ok(_) => Err("config target must be a regular file, not a symlink; use manual setup for managed dotfiles".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("cannot inspect Herdr config".into()),
    }
}

fn atomic_write(
    path: &Path,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
) -> Result<(), String> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("config target has no parent")?)
            .map_err(|_| "cannot create config replacement")?;
    if let Some(permissions) = permissions {
        file.as_file()
            .set_permissions(permissions.clone())
            .map_err(|_| "cannot set config permissions")?;
    }
    file.write_all(bytes)
        .map_err(|_| "cannot write config replacement")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "cannot flush config replacement")?;
    file.persist(path)
        .map_err(|_| "cannot atomically replace Herdr config")?;
    Ok(())
}

fn parse(text: &str) -> Result<DocumentMut, String> {
    // TOML errors can echo user config. Return a fixed diagnostic instead.
    text.parse()
        .map_err(|_| "invalid Herdr TOML; no changes made".into())
}

fn managed_block() -> String {
    let mut block = "[ui.sidebar.agents]\nrows = []\n\n[ui.sidebar.spaces]\nrows = []\n"
        .parse::<DocumentMut>()
        .expect("static TOML");
    let rows = [
        vec![
            "$lineup_logo",
            "$lineup_state_idle",
            "$lineup_state_working",
            "$lineup_state_blocked",
            "$lineup_state_done",
            "$lineup_state_unknown",
            "$lineup_context_text",
            "$lineup_weekly_text",
        ],
        vec!["$lineup_repo", "$lineup_ship_text"],
    ];
    for (kind, tokens) in ["agents", "spaces"].into_iter().zip(rows) {
        let mut row = Array::new();
        for token in tokens {
            let color = match token {
                "$lineup_state_idle" => Some("#808080"),
                "$lineup_state_working" => Some("#89b4fa"),
                "$lineup_state_blocked" => Some("#f38ba8"),
                "$lineup_state_done" => Some("#a6e3a1"),
                "$lineup_state_unknown" => Some("#6c7086"),
                _ => None,
            };
            if let Some(color) = color {
                let mut style = InlineTable::new();
                style.insert("token", token.into());
                style.insert("fg", color.into());
                row.push(style);
            } else {
                row.push(token);
            }
        }
        let mut rows = Array::new();
        rows.push(row);
        block["ui"]["sidebar"][kind]["rows"] = value(rows);
    }
    format!("\n{BEGIN}\n{block}{END}\n")
}

fn sidebar_table(doc: &DocumentMut) -> Result<Option<&toml_edit::Table>, String> {
    let mut table = doc.as_table();
    for name in ["ui", "sidebar"] {
        match table.get(name) {
            None => return Ok(None),
            Some(item) => {
                table = item
                    .as_table()
                    .ok_or("conflicting ui/sidebar representation; use manual setup")?
            }
        }
    }
    Ok(Some(table))
}

fn refuse_existing_layout(doc: &DocumentMut) -> Result<(), String> {
    if let Some(table) = sidebar_table(doc)? {
        for name in ["agents", "spaces"] {
            if table.contains_key(name) {
                return Err(format!(
                    "conflicting ui.sidebar.{name} definition; use manual setup"
                ));
            }
        }
    }
    Ok(())
}

fn plan(text: &str, action: Action) -> Result<Option<String>, String> {
    let doc = parse(text)?;
    let block = managed_block();
    let has_marker = text.contains(BEGIN) || text.contains(END);
    if has_marker {
        if text.matches(BEGIN).count() != 1
            || text.matches(END).count() != 1
            || text.matches(&block).count() != 1
        {
            return Err(
                "modified or ambiguous herdr-lineup managed block; resolve manually".into(),
            );
        }
        let sidebar = sidebar_table(&doc)?.ok_or("missing owned sidebar table")?;
        // Even a key added after the end marker can still belong to the last
        // owned TOML table. Refuse rather than silently move its ownership.
        for name in ["agents", "spaces"] {
            let owned = sidebar
                .get(name)
                .and_then(Item::as_table)
                .ok_or("modified owned sidebar table")?;
            if owned.len() != 1 || !owned.contains_key("rows") {
                return Err("modified owned sidebar table; resolve manually".into());
            }
        }
        let remainder = text.replacen(&block, "", 1);
        refuse_existing_layout(&parse(&remainder)?)?;
        return match action {
            Action::Configure => Ok(None),
            Action::Unconfigure => Ok(Some(remainder)),
        };
    }
    match action {
        Action::Unconfigure => Ok(None),
        Action::Configure => {
            refuse_existing_layout(&doc)?;
            let candidate = format!("{text}{block}");
            // Round-trip the proposed edit with toml_edit before any write;
            // no unrelated formatting is normalized or reordered.
            let edited = parse(&candidate)?.to_string();
            if edited != candidate {
                return Err("config formatting cannot be preserved; use manual setup".into());
            }
            Ok(Some(edited))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    const ORIGINAL: &str = "# User comment\nonboarding = false\n\n[theme]\nname = 'catppuccin' # Keep me\n\n[ui]\npane_borders = 'off'\n";

    fn test_state(path: &Path) -> PathBuf {
        path.ancestors().find(|p| p.is_dir()).unwrap().join("state")
    }

    fn apply(
        path: &Path,
        action: Action,
        reload: impl FnMut(&Path) -> Result<(), String>,
    ) -> Result<Outcome, String> {
        super::apply(path, &test_state(path), action, reload)
    }

    fn apply_validated(
        path: &Path,
        action: Action,
        validate: impl FnMut(&Path) -> Result<(), String>,
        reload: impl FnMut(&Path) -> Result<(), String>,
    ) -> Result<Outcome, String> {
        super::apply_validated(path, &test_state(path), action, validate, reload)
    }

    fn file_count(path: &Path) -> usize {
        fs::read_dir(path)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_type().unwrap().is_file())
            .count()
    }

    fn fixture(contents: &str) -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, contents).unwrap();
        (dir, path)
    }

    fn ok_reload(_: &Path) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn transaction_unlock_is_not_delayed_by_a_duplicated_descriptor() {
        let (_dir, path) = fixture(ORIGINAL);
        let state = test_state(&path);
        let guard = transaction_lock(&path, &state).unwrap();
        let inherited = guard.0.try_clone().unwrap();
        assert!(transaction_lock(&path, &state).is_err());
        drop(guard);
        let next = transaction_lock(&path, &state).unwrap();
        drop(inherited);
        drop(next);
    }

    #[test]
    fn concurrent_transaction_cannot_replace_config_during_validation_or_reload() {
        let (_dir, path) = fixture(ORIGINAL);
        let mut attempted = false;
        let result = apply_validated(
            &path,
            Action::Configure,
            |_| {
                attempted = true;
                let other = apply(&path, Action::Configure, ok_reload);
                assert!(other.unwrap_err().contains("another configuration action"));
                Ok(())
            },
            |_| {
                let other = apply(&path, Action::Unconfigure, ok_reload);
                assert!(other.unwrap_err().contains("another configuration action"));
                Ok(())
            },
        );
        assert!(attempted);
        assert!(result.is_ok());
        apply(&path, Action::Unconfigure, ok_reload).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), ORIGINAL);
    }

    #[test]
    fn rejected_candidate_validation_never_replaces_or_reloads_config() {
        let (dir, path) = fixture(ORIGINAL);
        let mut proposed = String::new();
        let result = apply_validated(
            &path,
            Action::Configure,
            |p| {
                proposed = fs::read_to_string(p).unwrap();
                Err("invalid layout".into())
            },
            |_| panic!("no reload"),
        );
        assert!(proposed.contains("$lineup_logo"));
        assert!(result.unwrap_err().contains("invalid layout"));
        assert_eq!(fs::read_to_string(path).unwrap(), ORIGINAL);
        assert_eq!(file_count(dir.path()), 1);
    }

    #[test]
    fn absent_config_is_created_and_failed_reload_restores_absence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("herdr/config.toml");
        assert_eq!(
            apply(&path, Action::Unconfigure, |_| panic!()),
            Ok(Outcome::Unchanged)
        );
        assert!(!path.parent().unwrap().exists());
        let mut calls = 0;
        assert!(
            apply(&path, Action::Configure, |p| {
                calls += 1;
                if calls == 1 {
                    assert!(p.exists());
                    Err("reject".into())
                } else {
                    assert!(!p.exists());
                    Ok(())
                }
            })
            .is_err()
        );
        assert!(!path.exists());
        apply(&path, Action::Configure, ok_reload).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn rollback_does_not_overwrite_a_concurrent_user_edit() {
        let (_dir, path) = fixture(ORIGINAL);
        let error = apply(&path, Action::Configure, |p| {
            fs::write(p, "# external edit\nonboarding = true\n").unwrap();
            Err("reject".into())
        })
        .unwrap_err();
        assert!(error.contains("rollback refused"));
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "# external edit\nonboarding = true\n"
        );
    }

    #[test]
    fn target_resolution_uses_herdr_override_and_injected_home() {
        assert_eq!(
            config_path(Some(OsStr::new("/isolated/custom.toml")), None).unwrap(),
            Path::new("/isolated/custom.toml")
        );
        assert_eq!(
            config_path(None, Some(OsStr::new("/isolated/home"))).unwrap(),
            Path::new("/isolated/home/.config/herdr/config.toml")
        );
        assert!(config_path(None, None).is_err());
        assert!(config_path(Some(OsStr::new("")), None).is_err());
        assert!(config_path(None, Some(OsStr::new("relative"))).is_err());
    }

    #[test]
    fn owned_rows_have_valid_tokens_and_static_state_colors() {
        let (_dir, path) = fixture(ORIGINAL);
        apply(&path, Action::Configure, ok_reload).unwrap();
        let contents = fs::read_to_string(path).unwrap();
        let doc = contents.parse::<DocumentMut>().unwrap();
        let row = doc["ui"]["sidebar"]["agents"]["rows"]
            .as_array()
            .unwrap()
            .get(0)
            .unwrap()
            .as_array()
            .unwrap();
        let mut states = 0;
        for entry in row {
            let token = if let Some(style) = entry.as_inline_table() {
                let token = style["token"].as_str().unwrap();
                assert!(token.starts_with("$lineup_state_"));
                let color = style["fg"].as_str().unwrap();
                assert!(color.starts_with('#') && color.len() == 7);
                states += 1;
                token
            } else {
                entry.as_str().unwrap()
            };
            assert!(crate::tokens::PANE_KEYS.contains(&token.strip_prefix('$').unwrap()));
        }
        assert_eq!(states, 5);
    }

    #[test]
    fn preserves_bytes_and_user_edits_outside_owned_block() {
        let (_dir, path) = fixture(ORIGINAL);
        let mut loaded = String::new();
        let Outcome::Changed { backup } = apply(&path, Action::Configure, |p| {
            loaded = fs::read_to_string(p).unwrap();
            Ok(())
        })
        .unwrap() else {
            panic!("expected change")
        };
        assert_eq!(fs::read_to_string(backup).unwrap(), ORIGINAL);
        assert!(loaded.starts_with(ORIGINAL));
        assert!(loaded.contains("$lineup_logo"));
        assert!(loaded.contains("$lineup_context_text"));
        assert!(loaded.contains("$lineup_weekly_text"));
        assert!(loaded.contains("$lineup_repo"));
        assert!(loaded.contains("$lineup_ship_text"));
        let edited = loaded.replace("name = 'catppuccin'", "name = 'terminal'");
        fs::write(&path, edited).unwrap();
        apply(&path, Action::Unconfigure, ok_reload).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            ORIGINAL.replace("name = 'catppuccin'", "name = 'terminal'")
        );
    }

    #[test]
    fn repeat_actions_do_not_write_backup_or_reload() {
        let (dir, path) = fixture(ORIGINAL);
        apply(&path, Action::Configure, ok_reload).unwrap();
        let configured = fs::read(&path).unwrap();
        let count = file_count(dir.path());
        assert_eq!(
            apply(&path, Action::Configure, |_| panic!("no reload")),
            Ok(Outcome::Unchanged)
        );
        assert_eq!(fs::read(&path).unwrap(), configured);
        assert_eq!(file_count(dir.path()), count);
        apply(&path, Action::Unconfigure, ok_reload).unwrap();
        let count = file_count(dir.path());
        assert_eq!(
            apply(&path, Action::Unconfigure, |_| panic!("no reload")),
            Ok(Outcome::Unchanged)
        );
        assert_eq!(file_count(dir.path()), count);
    }

    #[test]
    fn conflicts_and_invalid_toml_leave_file_untouched() {
        for contents in [
            "[ui.sidebar.agents]\nrows = [['$other_plugin']]\n",
            "[ui.sidebar.spaces]\nrows = [['workspace']]\n",
            "ui = { sidebar = { agents = { rows = [['agent']] } } }\n",
            "[ui.sidebar.agents.rows_by_agent]\ncodex = [['agent']]\n",
            "ui = 42\n",
            "[broken\n",
            "# BEGIN herdr-lineup managed sidebar v1\n",
        ] {
            let (dir, path) = fixture(contents);
            assert!(apply(&path, Action::Configure, |_| panic!("no reload")).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), contents);
            assert_eq!(file_count(dir.path()), 1);
        }
    }

    #[test]
    fn refuses_modified_or_duplicated_owned_blocks_for_both_actions() {
        for action in [Action::Configure, Action::Unconfigure] {
            for duplicate in [false, true] {
                let (_dir, path) = fixture(ORIGINAL);
                apply(&path, Action::Configure, ok_reload).unwrap();
                let contents = fs::read_to_string(&path).unwrap();
                let modified = if duplicate {
                    format!("{contents}{contents}")
                } else {
                    contents.replace("$lineup_logo", "$other_logo")
                };
                fs::write(&path, &modified).unwrap();
                assert!(apply(&path, action, |_| panic!("no reload")).is_err());
                assert_eq!(fs::read_to_string(&path).unwrap(), modified);
            }
        }
    }

    #[test]
    fn rejects_settings_appended_to_owned_table_outside_markers() {
        let (_dir, path) = fixture(ORIGINAL);
        apply(&path, Action::Configure, ok_reload).unwrap();
        let changed = format!("{}row_gap = 1\n", fs::read_to_string(&path).unwrap());
        fs::write(&path, &changed).unwrap();
        assert!(apply(&path, Action::Unconfigure, |_| panic!("no reload")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), changed);
    }

    #[test]
    fn unique_backups_are_never_overwritten() {
        let (dir, path) = fixture(ORIGINAL);
        let existing = dir.path().join("config.toml.bak");
        fs::write(&existing, "previous backup").unwrap();
        let Outcome::Changed { backup: first } =
            apply(&path, Action::Configure, ok_reload).unwrap()
        else {
            panic!()
        };
        let configured = fs::read(&path).unwrap();
        let Outcome::Changed { backup: second } =
            apply(&path, Action::Unconfigure, ok_reload).unwrap()
        else {
            panic!()
        };
        assert_ne!(first, second);
        assert!(first.extension().unwrap() == "bak");
        assert_eq!(fs::read(first).unwrap(), ORIGINAL.as_bytes());
        assert_eq!(fs::read(second).unwrap(), configured);
        assert_eq!(fs::read_to_string(existing).unwrap(), "previous backup");
    }

    #[test]
    fn reload_failure_restores_exact_bytes_and_reloads_restoration() {
        for action in [Action::Configure, Action::Unconfigure] {
            let (_dir, path) = fixture(ORIGINAL);
            if matches!(action, Action::Unconfigure) {
                apply(&path, Action::Configure, ok_reload).unwrap();
            }
            let before = fs::read(&path).unwrap();
            let mut observed = Vec::new();
            let result = apply(&path, action, |p| {
                observed.push(fs::read(p).unwrap());
                if observed.len() == 1 {
                    Err("rejected".into())
                } else {
                    Ok(())
                }
            });
            assert!(result.unwrap_err().contains("restored"));
            assert_eq!(observed.len(), 2);
            assert_ne!(observed[0], before);
            assert_eq!(observed[1], before);
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn both_reload_failures_are_reported() {
        let (_dir, path) = fixture(ORIGINAL);
        let mut calls = 0;
        let error = apply(&path, Action::Configure, |_| {
            calls += 1;
            Err(format!("failure {calls}"))
        })
        .unwrap_err();
        assert!(error.contains("failure 1"));
        assert!(error.contains("failure 2"));
        assert_eq!(fs::read_to_string(path).unwrap(), ORIGINAL);
    }

    #[test]
    fn newline_and_implicit_parent_tables_round_trip() {
        for contents in [
            "",
            "# comment",
            "[ui]\npane_borders = 'off'",
            "[ui.sidebar]\n# sidebar comment\n",
            "[ui.other]\nvalue = 1\n",
        ] {
            let (_dir, path) = fixture(contents);
            apply(&path, Action::Configure, ok_reload).unwrap();
            apply(&path, Action::Unconfigure, ok_reload).unwrap();
            assert_eq!(fs::read_to_string(path).unwrap(), contents);
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_and_preserves_config_permissions() {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        let (dir, path) = fixture(ORIGINAL);
        let link = dir.path().join("link.toml");
        symlink(&path, &link).unwrap();
        assert!(apply(&link, Action::Configure, |_| panic!("no reload")).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let Outcome::Changed { backup } = apply(&path, Action::Configure, ok_reload).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
