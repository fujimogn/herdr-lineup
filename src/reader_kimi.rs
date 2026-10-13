//! Completed-turn Kimi context from one exact local session.
//! Weekly quota stays unavailable. This reader never uses the network. The
//! `toml` crate parses `config.toml` and the reader keeps only the selected
//! alias's window keys.

use crate::model::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CACHE_ENTRIES: usize = 128;
type Turn = (f64, SystemTime, String);

struct CachedWire {
    fingerprint: Fingerprint,
    observation: Result<Turn, MissingReason>,
}
static CACHE: LazyLock<Mutex<HashMap<(PathBuf, String), CachedWire>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct KimiReader;
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static TEST_GATE: Mutex<()> = Mutex::new(());
impl UsageReader for KimiReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        #[cfg(test)]
        let _gate = TEST_GATE.lock().unwrap_or_else(|error| error.into_inner());
        let source = SourceKind::KimiSession;
        if !associated(request, Provider::Kimi) {
            return UsageReading::missing(request, source, MissingReason::Unassociated);
        }
        bounded_read(request, source, &BUSY, move |request| {
            let started = std::time::Instant::now();
            match read_session(request) {
                Ok((percent, time)) => {
                    let evaluated = ReadRequest {
                        now: request.now_after(started),
                        ..*request
                    };
                    UsageReading::sample(&evaluated, source, percent, time)
                }
                Err(reason) => UsageReading::missing(request, source, reason),
            }
        })
    }
}

fn read_session(request: &ReadRequest<'_>) -> Result<(f64, SystemTime), MissingReason> {
    let mut budget = Budget::new(request);
    let wanted = lexical_absolute(&request.identity.repository_identity)
        .ok_or(MissingReason::Unassociated)?;
    let states = budget.files(&request.home.join("sessions"), 2, |path| {
        path.file_name().is_some_and(|name| name == "state.json")
    })?;
    let mut matched = Vec::new();
    let mut saw_state = false;
    let mut unproven_duplicate = false;
    for path in states {
        saw_state = true;
        // Another session's broken file is not a match. Timeout still applies
        // to the whole read. Two readable matches stay duplicates below.
        match state_binding(&budget, &path) {
            Ok(Some((id, cwd))) if id == request.identity.native_session_id && cwd == wanted => {
                matched.push(path);
            }
            Ok(_) => {}
            Err(reason @ (MissingReason::Timeout | MissingReason::EntryLimit)) => {
                return Err(reason);
            }
            // A state.json we cannot read still counts when its session
            // directory is this native id: that is the path shape of a
            // duplicate, so uniqueness is unproven. A different session
            // directory cannot be this id and does not hide a unique match.
            Err(MissingReason::PermissionDenied) => {
                if session_directory_could_match(&path, &request.identity.native_session_id) {
                    unproven_duplicate = true;
                }
            }
            Err(_) => {}
        }
    }
    if unproven_duplicate || matched.len() > 1 {
        return Err(MissingReason::Unassociated);
    }
    let Some(state) = matched.first() else {
        return Err(if saw_state {
            MissingReason::Unassociated
        } else {
            MissingReason::NotFound
        });
    };
    let session_dir = state.parent().ok_or(MissingReason::Invalid)?;
    let wire = session_dir.join("agents/main/wire.jsonl");
    let file = open_nofollow(session_dir, &["agents", "main", "wire.jsonl"])?;
    let (tokens, observed, alias) = read_wire(request, &budget, &wire, file)?;
    let percent = context_percent(&budget, request, tokens, &alias)?;
    Ok((percent, observed))
}

fn state_binding(budget: &Budget, path: &Path) -> Result<Option<(String, PathBuf)>, MissingReason> {
    let mut file = budget.open(path)?;
    let before = Fingerprint::of(&file.metadata().map_err(|error| io_reason(&error))?);
    let bytes = budget.read_range(&mut file, 0, before.size as usize)?;
    let current = fs::symlink_metadata(path).map_err(|error| io_reason(&error))?;
    if !current.is_file() || current.file_type().is_symlink() {
        return Err(MissingReason::SourceError);
    }
    let path_after = Fingerprint::of(&current);
    let file_after = Fingerprint::of(&file.metadata().map_err(|error| io_reason(&error))?);
    if snapshot_change(before, path_after, file_after) != SnapshotChange::Stable {
        return Err(MissingReason::SourceError);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| MissingReason::Invalid)?;
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(cwd) = value
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(lexical_absolute)
    else {
        return Ok(None);
    };
    Ok(Some((id.to_owned(), cwd)))
}

/// Open `root/parts...` one component at a time. `O_NOFOLLOW` rejects a symlink
/// at that component, matching the Codex reader's refusal to follow links.
fn open_nofollow(root: &Path, parts: &[&str]) -> Result<File, MissingReason> {
    let mut current = open_directory(root)?;
    let (file_name, directories) = parts.split_last().ok_or(MissingReason::Invalid)?;
    for name in directories {
        current = openat(
            &current,
            name,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY,
        )?;
    }
    let file = openat(
        &current,
        file_name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
    )?;
    if !file
        .metadata()
        .map_err(|error| io_reason(&error))?
        .is_file()
    {
        return Err(MissingReason::Invalid);
    }
    Ok(file)
}

fn open_directory(path: &Path) -> Result<File, MissingReason> {
    let meta = fs::symlink_metadata(path).map_err(|error| io_reason(&error))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(MissingReason::Invalid);
    }
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)
        .map_err(|error| io_reason(&error))
}

fn openat(dir: &File, name: &str, flags: i32) -> Result<File, MissingReason> {
    let name = std::ffi::CString::new(name).map_err(|_| MissingReason::Invalid)?;
    // SAFETY: `dir` is an open directory and `name` is one NUL-terminated path
    // component. `O_NOFOLLOW` makes a symlink return an error instead of a target.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(io_reason(&std::io::Error::last_os_error()));
    }
    // SAFETY: `fd` is a new descriptor owned by this function.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn read_wire(
    request: &ReadRequest<'_>,
    budget: &Budget,
    path: &Path,
    mut file: File,
) -> Result<Turn, MissingReason> {
    let fingerprint = Fingerprint::of(&file.metadata().map_err(|error| io_reason(&error))?);
    let key = (
        path.to_path_buf(),
        request.identity.native_session_id.clone(),
    );
    if let Some(cached) = CACHE
        .lock()
        .map_err(|_| MissingReason::SourceError)?
        .get(&key)
        && cached.fingerprint == fingerprint
    {
        budget.check()?;
        return cached.observation.clone();
    }
    let text = read_jsonl_tail(budget, &mut file, fingerprint.size, |_| Ok(()))?;
    budget.check()?;
    let result = parse_wire(&text, budget);
    let current = fs::symlink_metadata(path).map_err(|error| io_reason(&error))?;
    if !current.is_file() || current.file_type().is_symlink() {
        return Err(MissingReason::SourceError);
    }
    let path_after = Fingerprint::of(&current);
    let file_after = Fingerprint::of(&file.metadata().map_err(|error| io_reason(&error))?);
    match snapshot_change(fingerprint, path_after, file_after) {
        SnapshotChange::Appended => return result,
        SnapshotChange::Rejected => return Err(MissingReason::SourceError),
        SnapshotChange::Stable => {}
    }
    if !matches!(
        result,
        Err(MissingReason::Timeout | MissingReason::SourceError | MissingReason::PermissionDenied)
    ) {
        let mut cache = CACHE.lock().map_err(|_| MissingReason::SourceError)?;
        if cache.len() >= CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(
            key,
            CachedWire {
                fingerprint,
                observation: result.clone(),
            },
        );
    }
    result
}

fn parse_wire(text: &str, budget: &Budget) -> Result<Turn, MissingReason> {
    let mut alias = None;
    let mut turn: Option<Turn> = None;
    let mut binding_after_turn = false;
    let mut invalidated = false;
    for line in text.split_inclusive('\n') {
        budget.check()?;
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) if !line.ends_with('\n') && error.is_eof() => continue,
            Err(_) => return Err(MissingReason::Invalid),
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let agent = value.get("agentId").and_then(Value::as_str);
        if agent == Some("main") && matches!(kind, "profile.bind" | "config.update") {
            if let Some(next) = value.get("modelAlias") {
                // A binding that arrives after the selected turn is not that
                // turn's alias. The next completed turn may adopt it.
                binding_after_turn = turn.is_some();
                alias = next
                    .as_str()
                    .filter(|alias| !alias.is_empty())
                    .map(str::to_owned);
                if alias.is_none() {
                    binding_after_turn = true;
                }
            }
        } else if kind == "token_counting.turn_recorded" && agent == Some("main") {
            let tokens = value
                .get("tokens")
                .and_then(Value::as_f64)
                .filter(|tokens| tokens.is_finite() && *tokens >= 0.0)
                .ok_or(MissingReason::Invalid)?;
            let observed = epoch_millis(value.get("time"))?;
            let Some(current) = alias.clone() else {
                turn = None;
                invalidated = true;
                binding_after_turn = false;
                continue;
            };
            turn = Some((tokens, observed, current));
            invalidated = false;
            binding_after_turn = false;
        } else if turn.is_some() && context_changed(kind, agent) {
            invalidated = true;
        }
    }
    if invalidated || binding_after_turn {
        return Err(MissingReason::Invalid);
    }
    turn.ok_or(MissingReason::NotFound)
}

fn context_changed(kind: &str, agent: Option<&str>) -> bool {
    if agent.is_some_and(|agent| agent != "main") {
        return false;
    }
    matches!(
        kind,
        "token_counting.measured"
            | "token_counting.truncated"
            | "token_counting.rebased"
            | "context.apply_compaction"
            | "context.undo"
            | "context.clear"
            | "context.append_message"
            | "context.append_loop_event"
            | "turn.prompt"
            | "turn.steer"
            | "turn.cancel"
            | "turn.step.interrupted"
            | "turn.step.retrying"
            | "llm.request"
    )
}

fn context_percent(
    budget: &Budget,
    request: &ReadRequest<'_>,
    tokens: f64,
    alias: &str,
) -> Result<f64, MissingReason> {
    let window = model_window(budget, request.home, alias)?;
    let percent = tokens * 100.0 / window;
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return Err(MissingReason::Invalid);
    }
    // Future timestamps are judged by `UsageReading::sample` at
    // `request.now_after`, the same clock Codex uses for context. A completed
    // turn does not expire by age.
    Ok(percent)
}

#[derive(Deserialize)]
struct WindowDocument {
    #[serde(default)]
    models: HashMap<String, WindowKeys>,
}

/// Only the two window integers are deserialized. Every other key, including
/// credential tables, is discarded by serde and is not stored on this struct.
#[derive(Deserialize, Default)]
struct WindowKeys {
    max_input_size: Option<i64>,
    max_context_size: Option<i64>,
}

fn model_window(budget: &Budget, home: &Path, alias: &str) -> Result<f64, MissingReason> {
    let text = budget.text(&home.join("config.toml"), false)?;
    // Parse errors can quote file text. Do not log or keep them.
    let document: WindowDocument = toml::from_str(&text).map_err(|_| MissingReason::Invalid)?;
    drop(text);
    let Some(model) = document.models.get(alias) else {
        return Err(MissingReason::Invalid);
    };
    let input = window_at_least_one(model.max_input_size)?;
    let context = window_at_least_one(model.max_context_size)?;
    // The installed client clamps an input window above the context window.
    // Those two sources disagree on which number wins, so an inverted pair is
    // unavailable instead of a guessed denominator.
    match (input, context) {
        (Some(input), Some(context)) if input > context => Err(MissingReason::Invalid),
        (Some(input), _) => Ok(input),
        (None, Some(context)) => Ok(context),
        _ => Err(MissingReason::Invalid),
    }
}

/// Kimi accepts an integer of at least 1. Zero, negative, and a present
/// non-integer never fall back to the other window.
fn window_at_least_one(value: Option<i64>) -> Result<Option<f64>, MissingReason> {
    match value {
        None => Ok(None),
        Some(value) if value >= 1 => Ok(Some(value as f64)),
        Some(_) => Err(MissingReason::Invalid),
    }
}

fn session_directory_could_match(state: &Path, native_id: &str) -> bool {
    match state
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
    {
        Some(name) => name == native_id,
        None => true,
    }
}

fn epoch_millis(value: Option<&Value>) -> Result<SystemTime, MissingReason> {
    let millis = value
        .and_then(Value::as_u64)
        .ok_or(MissingReason::Invalid)?;
    UNIX_EPOCH
        .checked_add(Duration::from_millis(millis))
        .ok_or(MissingReason::Invalid)
}

/// Lexical absolute path, matching Kimi's non-Windows `path.resolve` without
/// resolving symlinks. Relative paths are not comparable to a pane cwd.
fn lexical_absolute(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => normalized.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{identity, request};
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn unsupported_even_with_fresh_stale_or_mismatched_dummy_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut id = identity(Provider::Kimi);
        id.repository_identity = "/synthetic/repo".into();
        let mut r = request(&id, dir.path());
        let path = dir.path().join("sessions/cwd/session-1/agents/main");
        std::fs::create_dir_all(&path).unwrap();
        for dummy in [
            None,
            Some(r#"{"type":"agent.status.updated","sessionId":"session-1","contextUsage":0.07}"#),
            Some(r#"{"type":"agent.status.updated","sessionId":"other","contextUsage":0.07}"#),
        ] {
            if let Some(dummy) = dummy {
                std::fs::write(path.join("wire.jsonl"), dummy).unwrap();
            }
            for age in [0, 61] {
                r.now += std::time::Duration::from_secs(age);
                let reading = KimiReader.read(&r);
                assert_eq!(
                    reading.context,
                    UsageOutcome::Unavailable(MissingReason::NotFound)
                );
                assert_eq!(
                    reading.weekly_remaining,
                    UsageOutcome::Unavailable(MissingReason::Unsupported)
                );
                assert!(reading.observed_at.is_none());
                assert!(reading.weekly_resets_at.is_none());
            }
        }
        let other = identity(Provider::Codex);
        assert_eq!(
            KimiReader.read(&request(&other, dir.path())).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
    }

    fn fresh_ms(request: &ReadRequest<'_>) -> u128 {
        request
            .now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    }

    fn install(
        home: &std::path::Path,
        session: &str,
        cwd: &str,
        tokens: i64,
        time_ms: u128,
        after: &str,
    ) {
        let dir = home.join("sessions/wd_synthetic").join(session);
        std::fs::create_dir_all(dir.join("agents/main")).unwrap();
        std::fs::write(
            dir.join("state.json"),
            format!(r#"{{"id":"{session}","cwd":"{cwd}","title":"synthetic"}}"#),
        )
        .unwrap();
        std::fs::write(
            dir.join("agents/main/wire.jsonl"),
            format!(
                r#"{{"type":"metadata","protocol_version":"1.5","created_at":1}}
{{"type":"profile.bind","agentId":"main","modelAlias":"synthetic-alias","systemPrompt":"synthetic","disallowedTools":[],"thinkingEffort":"off","time":{time_ms}}}
{{"type":"token_counting.turn_recorded","agentId":"main","turnId":1,"length":2,"tokens":{tokens},"time":{time_ms}}}
{after}"#
            ),
        )
        .unwrap();
        std::fs::write(
            home.join("config.toml"),
            "api_key = \"synthetic-credential\"\n\n[models.\"synthetic-alias\"]\nmax_input_size = 1000\nmax_context_size = 100000\n",
        )
        .unwrap();
    }

    fn read_at(home: &std::path::Path, session: &str, cwd: &str) -> UsageOutcome {
        let mut identity = identity(Provider::Kimi);
        identity.native_session_id = session.into();
        identity.repository_identity = cwd.into();
        KimiReader.read(&request(&identity, home)).context
    }

    #[test]
    fn completed_turn_uses_input_window_and_ui_ratio() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let base = request(&bound, dir.path());
        install(
            dir.path(),
            "session-1",
            "/synthetic/repo",
            75,
            fresh_ms(&base),
            "",
        );
        let mut identity = identity(Provider::Kimi);
        identity.repository_identity = "/synthetic/repo/./sub/..".into();
        let reading = KimiReader.read(&request(&identity, dir.path()));
        assert_eq!(reading.context, UsageOutcome::Available(7.5));
        assert_eq!(
            reading.weekly_remaining,
            UsageOutcome::Unavailable(MissingReason::Unsupported)
        );
        assert!(format!("{reading:?}").contains("7.5"));
        assert!(!format!("{reading:?}").contains("synthetic-credential"));
    }

    #[test]
    fn mismatch_duplicate_symlink_turn_compaction_and_alias_hide_context() {
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, std::path::Path::new(".")));
        type Case = (&'static str, fn(&std::path::Path, u128));
        let cases: &[Case] = &[
            ("mismatch", |home, now| {
                install(home, "other-session", "/synthetic/repo", 75, now, "");
            }),
            ("cwd", |home, now| {
                install(home, "session-1", "/elsewhere", 75, now, "");
            }),
            ("duplicate", |home, now| {
                install(home, "session-1", "/synthetic/repo", 75, now, "");
                let extra = home.join("sessions/wd_other/session-1");
                std::fs::create_dir_all(&extra).unwrap();
                std::fs::copy(
                    home.join("sessions/wd_synthetic/session-1/state.json"),
                    extra.join("state.json"),
                )
                .unwrap();
            }),
            ("symlink", |home, now| {
                install(home, "session-1", "/synthetic/repo", 75, now, "");
                let state = home.join("sessions/wd_synthetic/session-1/state.json");
                let real = home.join("real-state.json");
                std::fs::rename(&state, &real).unwrap();
                std::os::unix::fs::symlink(&real, &state).unwrap();
            }),
            ("turn", |home, now| {
                install(
                    home,
                    "session-1",
                    "/synthetic/repo",
                    75,
                    now,
                    &format!(
                        r#"{{"type":"turn.prompt","agentId":"main","input":"synthetic","origin":{{}},"time":{now}}}"#
                    ),
                );
            }),
            ("compaction", |home, now| {
                install(
                    home,
                    "session-1",
                    "/synthetic/repo",
                    75,
                    now,
                    &format!(
                        r#"{{"type":"context.apply_compaction","agentId":"main","summary":"synthetic","compactedCount":1,"time":{now}}}
{{"type":"token_counting.rebased","agentId":"main","length":1,"tokens":10,"measured":false,"time":{now}}}"#
                    ),
                );
            }),
            ("alias", |home, now| {
                install(home, "session-1", "/synthetic/repo", 75, now, "");
                std::fs::write(
                    home.join("config.toml"),
                    "[models.\"other-alias\"]\nmax_context_size = 1000\n",
                )
                .unwrap();
            }),
        ];
        for (name, setup) in cases {
            let dir = tempfile::tempdir().unwrap();
            setup(dir.path(), now);
            let context = read_at(dir.path(), "session-1", "/synthetic/repo");
            assert!(
                !matches!(context, UsageOutcome::Available(_)),
                "{name} returned {context:?}"
            );
        }
    }

    fn write_models(home: &std::path::Path) {
        std::fs::write(
            home.join("config.toml"),
            "api_key = \"synthetic-credential\"\n\n[models.\"alias-a\"]\nmax_input_size = 1000\nmax_context_size = 100000\n\n[models.\"alias-b\"]\nmax_input_size = 200\nmax_context_size = 100000\n",
        )
        .unwrap();
    }

    #[test]
    fn model_switch_uses_the_alias_bound_before_that_turn() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, dir.path()));
        let session = dir.path().join("sessions/wd_synthetic/session-1");
        std::fs::create_dir_all(session.join("agents/main")).unwrap();
        std::fs::write(
            session.join("state.json"),
            r#"{"id":"session-1","cwd":"/synthetic/repo"}"#,
        )
        .unwrap();
        write_models(dir.path());
        std::fs::write(
            session.join("agents/main/wire.jsonl"),
            format!(
                r#"{{"type":"profile.bind","agentId":"main","modelAlias":"alias-a","time":{now}}}
{{"type":"token_counting.turn_recorded","agentId":"main","tokens":80,"time":{now}}}
{{"type":"config.update","agentId":"main","modelAlias":"alias-b","time":{now}}}
{{"type":"token_counting.turn_recorded","agentId":"main","tokens":40,"time":{now}}}
"#
            ),
        )
        .unwrap();
        assert_eq!(
            read_at(dir.path(), "session-1", "/synthetic/repo"),
            UsageOutcome::Available(20.0)
        );

        std::fs::write(
            session.join("agents/main/wire.jsonl"),
            format!(
                r#"{{"type":"profile.bind","agentId":"main","modelAlias":"alias-a","time":{now}}}
{{"type":"token_counting.turn_recorded","agentId":"main","tokens":75,"time":{now}}}
{{"type":"config.update","agentId":"main","modelAlias":"alias-b","time":{now}}}
"#
            ),
        )
        .unwrap();
        assert!(
            !matches!(
                read_at(dir.path(), "session-1", "/synthetic/repo"),
                UsageOutcome::Available(_)
            ),
            "a bind after the only turn must not keep the earlier window"
        );
    }

    #[test]
    fn alias_outside_the_read_tail_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, dir.path()));
        let session = dir.path().join("sessions/wd_synthetic/session-1");
        std::fs::create_dir_all(session.join("agents/main")).unwrap();
        std::fs::write(
            session.join("state.json"),
            r#"{"id":"session-1","cwd":"/synthetic/repo"}"#,
        )
        .unwrap();
        write_models(dir.path());
        let turn = format!(
            "{{\"type\":\"token_counting.turn_recorded\",\"agentId\":\"main\",\"tokens\":40,\"time\":{now}}}\n"
        );
        let bind = format!(
            "{{\"type\":\"profile.bind\",\"agentId\":\"main\",\"modelAlias\":\"alias-b\",\"time\":{now}}}\n"
        );
        let filler = "{\"type\":\"metadata\",\"pad\":\"synthetic\"}\n";
        let mut hidden = bind.clone();
        while hidden.len() + turn.len() <= JSONL_TAIL_BYTES {
            hidden.push_str(filler);
        }
        hidden.push_str(&turn);
        std::fs::write(session.join("agents/main/wire.jsonl"), &hidden).unwrap();
        assert!(
            !matches!(
                read_at(dir.path(), "session-1", "/synthetic/repo"),
                UsageOutcome::Available(_)
            ),
            "an alias outside the tail must not be reused"
        );
        let mut visible = String::new();
        while visible.len() + bind.len() + turn.len() <= JSONL_TAIL_BYTES {
            visible.push_str(filler);
        }
        visible.push_str(&bind);
        visible.push_str(&turn);
        std::fs::write(session.join("agents/main/wire.jsonl"), visible).unwrap();
        assert_eq!(
            read_at(dir.path(), "session-1", "/synthetic/repo"),
            UsageOutcome::Available(20.0)
        );
    }

    #[test]
    fn invalid_other_state_file_does_not_hide_a_unique_match() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, dir.path()));
        install(dir.path(), "session-1", "/synthetic/repo", 75, now, "");
        let broken = dir.path().join("sessions/wd_broken/session-x");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("state.json"), "{").unwrap();
        assert_eq!(
            read_at(dir.path(), "session-1", "/synthetic/repo"),
            UsageOutcome::Available(7.5)
        );
        let extra = dir.path().join("sessions/wd_other/session-1");
        std::fs::create_dir_all(&extra).unwrap();
        std::fs::copy(
            dir.path()
                .join("sessions/wd_synthetic/session-1/state.json"),
            extra.join("state.json"),
        )
        .unwrap();
        assert!(
            !matches!(
                read_at(dir.path(), "session-1", "/synthetic/repo"),
                UsageOutcome::Available(_)
            ),
            "a second readable match is still a duplicate"
        );
    }

    #[test]
    fn symlinked_intermediate_directory_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, dir.path()));
        install(dir.path(), "session-1", "/synthetic/repo", 75, now, "");
        let session = dir.path().join("sessions/wd_synthetic/session-1");
        let real = dir.path().join("real-agents");
        std::fs::rename(session.join("agents"), &real).unwrap();
        std::os::unix::fs::symlink(&real, session.join("agents")).unwrap();
        assert!(
            !matches!(
                read_at(dir.path(), "session-1", "/synthetic/repo"),
                UsageOutcome::Available(_)
            ),
            "agents must not be followed when it is a symlink"
        );
    }

    #[test]
    fn quoted_window_key_uses_kimi_integer_and_rejects_a_string() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, dir.path()));
        install(dir.path(), "session-1", "/synthetic/repo", 75, now, "");
        std::fs::write(
            dir.path().join("config.toml"),
            "[models.\"synthetic-alias\"]\n\"max_input_size\" = 1000\nmax_context_size = 100000\n",
        )
        .unwrap();
        assert_eq!(
            read_at(dir.path(), "session-1", "/synthetic/repo"),
            UsageOutcome::Available(7.5),
            "a quoted key with an integer is the input window, not the context window"
        );
        std::fs::write(
            dir.path().join("config.toml"),
            "[models.\"synthetic-alias\"]\nmax_input_size = \"1000\"\nmax_context_size = 100000\n",
        )
        .unwrap();
        assert!(
            !matches!(
                read_at(dir.path(), "session-1", "/synthetic/repo"),
                UsageOutcome::Available(_)
            ),
            "a quoted numeric string is not a window and must not fall back"
        );
        std::fs::write(
            dir.path().join("config.toml"),
            "[models.\"synthetic-alias\"]\n\"max_input_size\" = 0x3e8\nmax_context_size = 100000\napi_key = \"synthetic-credential\"\n",
        )
        .unwrap();
        assert_eq!(
            read_at(dir.path(), "session-1", "/synthetic/repo"),
            UsageOutcome::Available(7.5),
            "a hexadecimal integer is the input window"
        );
    }

    #[test]
    fn unreadable_state_hides_context_only_when_it_could_be_the_same_session() {
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, std::path::Path::new(".")));
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "session-1", "/synthetic/repo", 75, now, "");
        let other = dir.path().join("sessions/wd_blocked/session-x");
        std::fs::create_dir_all(&other).unwrap();
        let blocked = other.join("state.json");
        std::fs::write(
            &blocked,
            "{\"id\":\"session-1\",\"cwd\":\"/synthetic/repo\"}",
        )
        .unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(
            read_at(dir.path(), "session-1", "/synthetic/repo"),
            UsageOutcome::Available(7.5),
            "another session id cannot be this match"
        );
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o644)).unwrap();
        let same = dir.path().join("sessions/wd_blocked/session-1");
        std::fs::create_dir_all(&same).unwrap();
        let same_state = same.join("state.json");
        std::fs::write(&same_state, "{\"id\":\"unread\"}").unwrap();
        std::fs::set_permissions(&same_state, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(
            !matches!(
                read_at(dir.path(), "session-1", "/synthetic/repo"),
                UsageOutcome::Available(_)
            ),
            "an unreadable state.json named session-1 can still be a duplicate"
        );
    }

    #[test]
    fn symlinked_home_is_resolved_and_sessions_symlink_is_not() {
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, std::path::Path::new(".")));
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-home");
        install(&real, "session-1", "/synthetic/repo", 75, now, "");
        let linked = dir.path().join("linked-home");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        assert_eq!(
            read_at(&linked, "session-1", "/synthetic/repo"),
            UsageOutcome::Available(7.5),
            "the home symlink is the user setting, as with Codex"
        );
        let sessions = real.join("sessions");
        let moved = real.join("real-sessions");
        std::fs::rename(&sessions, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &sessions).unwrap();
        assert!(
            !matches!(
                read_at(&linked, "session-1", "/synthetic/repo"),
                UsageOutcome::Available(_)
            ),
            "a sessions symlink under the home must not be followed"
        );
    }

    #[test]
    fn idle_completed_turn_older_than_max_age_stays_available() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let base = request(&bound, dir.path());
        let twelve_hours_ms = 12 * 60 * 60 * 1000;
        install(
            dir.path(),
            "session-1",
            "/synthetic/repo",
            75,
            fresh_ms(&base) - twelve_hours_ms,
            "",
        );
        assert_eq!(
            read_at(dir.path(), "session-1", "/synthetic/repo"),
            UsageOutcome::Available(7.5)
        );
    }

    #[test]
    fn event_after_completed_turn_hides_context() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, dir.path()));
        install(
            dir.path(),
            "session-1",
            "/synthetic/repo",
            75,
            now,
            &format!(
                r#"{{"type":"context.apply_compaction","agentId":"main","summary":"synthetic","compactedCount":1,"time":{now}}}"#
            ),
        );
        assert!(
            !matches!(
                read_at(dir.path(), "session-1", "/synthetic/repo"),
                UsageOutcome::Available(_)
            ),
            "compaction after the completed turn must hide context"
        );
    }

    #[test]
    fn future_turn_timestamp_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let bound = identity(Provider::Kimi);
        let now = fresh_ms(&request(&bound, dir.path()));
        install(
            dir.path(),
            "session-1",
            "/synthetic/repo",
            75,
            now + 60_000,
            "",
        );
        assert_eq!(
            read_at(dir.path(), "session-1", "/synthetic/repo"),
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
    }

    #[test]
    fn unchanged_wire_does_not_read_content_again() {
        let _gate = TEST_GATE.lock().unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let mut id = identity(Provider::Kimi);
        id.repository_identity = "/synthetic/repo".into();
        let base = request(&id, dir.path());
        install(
            dir.path(),
            "session-1",
            "/synthetic/repo",
            75,
            fresh_ms(&base),
            "",
        );
        crate::model::tests::CONTENT_READS.set(0);
        let first = read_session(&base).unwrap();
        let reads = crate::model::tests::CONTENT_READS.get();
        assert!(
            reads >= 2,
            "state and wire are read on a cold cache: {reads}"
        );
        assert_eq!(read_session(&base).unwrap(), first);
        // Binding and the model window are rechecked. The wire body is not.
        assert_eq!(crate::model::tests::CONTENT_READS.get(), reads + 2);
    }
}
