//! Codex UI context percentage from exact-session rollout token_count observations.
use crate::model::*;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

// A context observation outside this suffix is NotFound, not a full-file scan.
// The header must fit in 64 KiB. Oversized headers are SourceError.
const TAIL_BYTES: usize = 2 * 1024 * 1024;
const HEADER_BYTES: usize = 64 * 1024;
const CACHE_ENTRIES: usize = 128;
type Observation = Result<(f64, SystemTime), MissingReason>;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    dev: u64,
    inode: u64,
    size: u64,
    mtime: (i64, i64),
}
impl Fingerprint {
    fn of(meta: &Metadata) -> Self {
        Self {
            dev: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
        }
    }
}
struct CachedRollout {
    fingerprint: Fingerprint,
    observation: Observation,
}
// Only paths, exact session IDs and derived observations are retained in memory.
// Discovery still runs on cache hits to reject duplicate and symlinked sources.
static CACHE: LazyLock<Mutex<HashMap<(PathBuf, String), CachedRollout>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct CodexReader;
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
impl UsageReader for CodexReader {
    fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
        let source = SourceKind::CodexRollout;
        if !associated(request, Provider::Codex)
            || !safe_component(&request.identity.native_session_id)
        {
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
fn rollout(path: &Path) -> bool {
    path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
        s.starts_with("rollout-") && (s.ends_with(".jsonl") || s.ends_with(".jsonl.zst"))
    })
}
fn read_session(request: &ReadRequest<'_>) -> Result<(f64, std::time::SystemTime), MissingReason> {
    read_session_with_hook(request, |_| {})
}

fn read_session_with_hook(
    request: &ReadRequest<'_>,
    after_read: impl FnOnce(&Path),
) -> Result<(f64, std::time::SystemTime), MissingReason> {
    let mut budget = Budget::new(request);
    // Discovery reads names only. Other sessions and credential files are never opened.
    let mut paths = budget.files(&request.home.join("sessions"), 3, rollout)?;
    paths.extend(budget.files(&request.home.join("archived_sessions"), 3, rollout)?);
    let id = &request.identity.native_session_id;
    paths.retain(|path| {
        path.file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| {
                name.ends_with(&format!("-{id}.jsonl"))
                    || name.ends_with(&format!("-{id}.jsonl.zst"))
            })
    });
    if paths.len() > 1 {
        return Err(MissingReason::Unassociated);
    }
    let path = paths.first().ok_or(MissingReason::NotFound)?;
    let mut file = budget.open(path)?;
    let fingerprint = Fingerprint::of(&file.metadata().map_err(|e| io_reason(&e))?);
    let key = (path.to_path_buf(), id.clone());
    if let Some(cached) = CACHE
        .lock()
        .map_err(|_| MissingReason::SourceError)?
        .get(&key)
        && cached.fingerprint == fingerprint
    {
        budget.check()?;
        return cached.observation;
    }
    let result = if path.extension().is_some_and(|e| e == "zst") {
        // Keep the existing 16 MiB decompressed limit (SourceError on overflow).
        budget
            .jsonl_text(path, true)
            .and_then(|text| parse_records(&text, id, &budget, false))
    } else {
        read_tail(&mut file, fingerprint.size, id, &budget)
    };
    budget.check()?;
    after_read(path);
    // Never cache a mixed snapshot while a writer appends, truncates or replaces it.
    let current = fs::symlink_metadata(path).map_err(|e| io_reason(&e))?;
    if !current.is_file() || current.file_type().is_symlink() {
        return Err(MissingReason::SourceError);
    }
    let path_after = Fingerprint::of(&current);
    let file_after = Fingerprint::of(&file.metadata().map_err(|e| io_reason(&e))?);
    if path_after != fingerprint || file_after != fingerprint {
        // An append cannot change bytes in the prefix read above. Return its
        // complete observation, but require a fresh read on the next cycle.
        if path_after.dev == fingerprint.dev
            && path_after.inode == fingerprint.inode
            && file_after.dev == fingerprint.dev
            && file_after.inode == fingerprint.inode
            && path_after.size > fingerprint.size
            && file_after.size >= path_after.size
        {
            return result;
        }
        return Err(MissingReason::SourceError);
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
            CachedRollout {
                fingerprint,
                observation: result,
            },
        );
    }
    result
}

fn read_tail(file: &mut fs::File, size: u64, id: &str, budget: &Budget) -> Observation {
    if size <= TAIL_BYTES as u64 {
        let text = Budget::decode_jsonl(budget.read_range(file, 0, size as usize)?)?;
        return parse_records(&text, id, budget, false);
    }
    let header = budget.read_range(file, 0, HEADER_BYTES)?;
    let end = header
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(MissingReason::SourceError)?;
    let value: Value =
        serde_json::from_slice(&header[..end]).map_err(|_| MissingReason::Invalid)?;
    if value["type"] != "session_meta" || value["payload"]["id"].as_str() != Some(id) {
        return Err(MissingReason::Unassociated);
    }
    // Include the preceding byte: a suffix exactly on a line boundary must keep
    // its first record. Otherwise discard bytes before the next newline, before
    // UTF-8 decoding (the split can itself be inside a multibyte character).
    let start = size - TAIL_BYTES as u64 - 1;
    let bytes = budget.read_range(file, start, TAIL_BYTES + 1)?;
    let complete = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |i| i + 1);
    let text = Budget::decode_jsonl(bytes[complete..].to_vec())?;
    parse_records(&text, id, budget, true)
}

fn parse_records(text: &str, id: &str, budget: &Budget, mut bound: bool) -> Observation {
    let mut observation = None;
    for line in text.split_inclusive('\n') {
        budget.check()?;
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            // An append may be visible before its final bytes/newline arrive.
            // Only an unfinished final record is transient; completed corruption
            // and malformed syntax still invalidate the source.
            Err(error) if !line.ends_with('\n') && error.is_eof() => continue,
            Err(_) => return Err(MissingReason::Invalid),
        };
        match value["type"].as_str() {
            Some("session_meta") => {
                if value["payload"]["id"].as_str() != Some(id) || bound {
                    return Err(MissingReason::Unassociated);
                }
                bound = true;
            }
            Some("event_msg") if value["payload"]["type"] == "token_count" => {
                let info = &value["payload"]["info"];
                // Quota-only events have no context observation. In particular,
                // resumed sessions can emit these before any valid token sample.
                if info.is_null() {
                    continue;
                }
                let used = info["last_token_usage"]["total_tokens"]
                    .as_f64()
                    .ok_or(MissingReason::Invalid)?;
                let capacity = info["model_context_window"]
                    .as_f64()
                    .ok_or(MissingReason::Invalid)?;
                if !used.is_finite() || !capacity.is_finite() || used < 0.0 || capacity <= 12_000.0
                {
                    return Err(MissingReason::Invalid);
                }
                // Match the retired herdr-agent-quota convention and Codex's UI:
                // deduct the 12,000-token baseline from both used and capacity.
                observation = Some((
                    (used - 12_000.0).max(0.0) / (capacity - 12_000.0) * 100.0,
                    timestamp(&value["timestamp"]).ok_or(MissingReason::Invalid)?,
                ));
            }
            _ => {}
        }
    }
    if !bound {
        return Err(MissingReason::Unassociated);
    }
    observation.ok_or(MissingReason::NotFound)
}
#[cfg(test)]
mod tests {
    #[test]
    fn append_after_read_returns_complete_prefix_without_caching_it() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let original = fs::read_to_string(
            dir.path()
                .join("sessions/2026/10/12/rollout-test-session-1.jsonl"),
        )
        .unwrap();
        let (_, sample) = original.split_once('\n').unwrap();
        let next = sample.replace("26000", "28000");
        let result = read_session_with_hook(&r, |path| {
            fs::OpenOptions::new()
                .append(true)
                .open(path)
                .unwrap()
                .write_all(next.as_bytes())
                .unwrap();
        });
        assert!((result.unwrap().0 - 7.0).abs() < 1e-9);
        assert!((read_session(&r).unwrap().0 - 8.0).abs() < 1e-9);
    }

    #[test]
    fn replacement_truncation_and_same_size_rewrite_during_read_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let original = fs::read_to_string(&path).unwrap();
        let old_time = fs::metadata(&path).unwrap().modified().unwrap();

        assert_eq!(
            read_session_with_hook(&r, |path| {
                fs::write(path, "").unwrap();
            }),
            Err(MissingReason::SourceError)
        );
        fs::write(&path, &original).unwrap();
        assert_eq!(
            read_session_with_hook(&r, |path| {
                let replacement = path.with_extension("replacement");
                fs::write(&replacement, &original).unwrap();
                fs::rename(replacement, path).unwrap();
            }),
            Err(MissingReason::SourceError)
        );
        assert_eq!(
            read_session_with_hook(&r, |path| {
                fs::write(path, original.replace("26000", "28000")).unwrap();
                fs::File::options()
                    .write(true)
                    .open(path)
                    .unwrap()
                    .set_modified(old_time + std::time::Duration::from_secs(1))
                    .unwrap();
            }),
            Err(MissingReason::SourceError)
        );
    }

    #[test]
    fn large_rollout_reads_latest_context_from_tail() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let text = fs::read_to_string(&path).unwrap();
        let (header, sample) = text.split_once('\n').unwrap();
        let padding = "{\"type\":\"response_item\"}\n".repeat(800_000);
        fs::write(&path, format!("{header}\n{padding}{sample}")).unwrap();
        assert!(fs::metadata(&path).unwrap().len() > 16 * 1024 * 1024);
        crate::model::tests::CONTENT_READS.set(0);
        assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
        let reads = crate::model::tests::CONTENT_READS.get();
        assert!(reads > 0);
        assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
        assert_eq!(crate::model::tests::CONTENT_READS.get(), reads);
    }

    #[test]
    fn unchanged_rollout_does_not_read_content_again() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        crate::model::tests::CONTENT_READS.set(0);
        let first = read_session(&r).unwrap();
        let reads = crate::model::tests::CONTENT_READS.get();
        assert!(reads > 0);
        assert_eq!(read_session(&r).unwrap(), first);
        assert_eq!(crate::model::tests::CONTENT_READS.get(), reads);
    }

    #[test]
    fn tail_starting_inside_a_record_discards_the_fragment() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let text = fs::read_to_string(&path).unwrap();
        let (header, sample) = text.split_once('\n').unwrap();
        // The tail begins in a long JSON string, potentially inside UTF-8.
        let padding = "あ".repeat(6 * 1024 * 1024);
        fs::write(
            &path,
            format!("{header}\n{{\"text\":\"{padding}\"}}\n{sample}"),
        )
        .unwrap();
        assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
    }

    #[test]
    fn tail_boundary_null_events_and_partial_append_preserve_last_sample() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let text = fs::read_to_string(&path).unwrap();
        let (header, sample) = text.split_once('\n').unwrap();
        let empty =
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":null}}\n";
        let partial = "{\"type\":";
        let filler = " ".repeat(TAIL_BYTES - sample.len() - empty.len() - partial.len() - 3);
        // The only sample starts exactly at the suffix boundary.
        let suffix = format!("{sample}{filler}{{}}\n{empty}{partial}");
        assert_eq!(suffix.len(), TAIL_BYTES);
        fs::write(&path, format!("{header}\n{suffix}")).unwrap();
        assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
        fs::write(&path, format!("{header}\n{suffix}\n")).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::Invalid));
    }

    #[test]
    fn bounded_tail_requires_matching_header_and_returns_missing_outside_window() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let text = fs::read_to_string(&path).unwrap();
        let (header, sample) = text.split_once('\n').unwrap();
        let padding = format!("{{\"text\":\"{}\"}}\n", "x".repeat(TAIL_BYTES));
        fs::write(&path, format!("{header}\n{sample}{padding}")).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::NotFound));
        let wrong_header = header.replace("session-1", "session-2");
        fs::write(&path, format!("{wrong_header}\n{padding}{sample}")).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::Unassociated));
        fs::write(&path, format!("{header}\n{padding}{header}\n{sample}")).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::Unassociated));
        fs::write(&path, format!("{padding}{sample}")).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::SourceError));
    }

    #[test]
    fn cache_invalidates_on_append_rewrite_replacement_truncation_and_duplicate() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let original = fs::read_to_string(&path).unwrap();
        let (_, sample) = original.split_once('\n').unwrap();
        assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(sample.replace("26000", "28000").as_bytes())
            .unwrap();
        assert!((read_session(&r).unwrap().0 - 8.0).abs() < 1e-9);

        let changed = fs::read_to_string(&path).unwrap().replace("28000", "30000");
        let prior = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, changed).unwrap();
        // Force an mtime-only fingerprint change even on coarse filesystems.
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(prior + std::time::Duration::from_secs(1))
            .unwrap();
        assert!((read_session(&r).unwrap().0 - 9.0).abs() < 1e-9);

        let metadata = fs::metadata(&path).unwrap();
        let replacement = path.with_extension("replacement");
        fs::write(
            &replacement,
            fs::read_to_string(&path).unwrap().replace("30000", "32000"),
        )
        .unwrap();
        fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_modified(metadata.modified().unwrap())
            .unwrap();
        fs::rename(replacement, &path).unwrap();
        assert!((read_session(&r).unwrap().0 - 10.0).abs() < 1e-9);

        fs::write(&path, &original).unwrap();
        assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
        fs::create_dir(dir.path().join("archived_sessions")).unwrap();
        fs::write(
            dir.path()
                .join("archived_sessions/rollout-copy-session-1.jsonl"),
            &original,
        )
        .unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::Unassociated));
    }

    #[test]
    fn cache_does_not_cross_homes_or_follow_replaced_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        fixture(other.path(), "session-2", "2026-10-12T00:00:00Z", 26_000);
        assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
        assert_eq!(
            read_session(&request(&id, other.path())),
            Err(MissingReason::Unassociated)
        );
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let moved = path.with_extension("saved");
        fs::rename(&path, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &path).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::NotFound));
        assert!(matches!(
            Budget::new(&r).open(&path),
            Err(MissingReason::Invalid)
        ));
    }

    #[test]
    fn compressed_rollout_keeps_decompressed_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fs::create_dir(dir.path().join("archived_sessions")).unwrap();
        let bytes = vec![b' '; 16 * 1024 * 1024 + 1];
        fs::write(
            dir.path()
                .join("archived_sessions/rollout-test-session-1.jsonl.zst"),
            zstd::stream::encode_all(bytes.as_slice(), 1).unwrap(),
        )
        .unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::SourceError));
    }

    #[test]
    fn context_matches_codex_baseline_and_rejects_small_windows() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        for (used, capacity, expected) in [
            (26_000, 212_000, Some(7.0)),
            (0, 212_000, Some(0.0)),
            (11_999, 212_000, Some(0.0)),
            (12_000, 212_000, Some(0.0)),
            (212_000, 212_000, Some(100.0)),
            (70, 12_000, None),
            (70, 11_999, None),
        ] {
            fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", used);
            let path = dir
                .path()
                .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
            let text = fs::read_to_string(&path).unwrap().replace(
                "\"model_context_window\":212000",
                &format!("\"model_context_window\":{capacity}"),
            );
            fs::write(path, text).unwrap();
            let result = read_session(&r);
            match expected {
                Some(percent) => assert!(
                    (result.unwrap().0 - percent).abs() < 1e-9,
                    "used={used}, capacity={capacity}"
                ),
                None => assert_eq!(result, Err(MissingReason::Invalid)),
            }
        }
    }
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn assert_percent(outcome: UsageOutcome) {
        let UsageOutcome::Available(percent) = outcome else {
            panic!("context unavailable");
        };
        assert!((percent - 7.0).abs() < 1e-9);
    }
    use super::*;
    use crate::model::tests::{identity, request};
    use std::fs;
    fn fixture(home: &Path, session: &str, time: &str, used: i64) {
        fs::create_dir_all(home.join("sessions/2026/10/12")).unwrap();
        let text = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session}\"}}}}\n{{\"type\":\"event_msg\",\"timestamp\":\"{time}\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"total_tokens\":{used}}},\"model_context_window\":212000}}}}}}\n"
        );
        fs::write(
            home.join("sessions/2026/10/12/rollout-test-session-1.jsonl"),
            text,
        )
        .unwrap();
    }
    #[test]
    fn active_append_split_utf8_does_not_hide_completed_observation() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{\"text\":\"");
        bytes.extend_from_slice(&[0xe3, 0x81]);
        fs::write(&path, &bytes).unwrap();
        assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
        bytes.push(b'\n');
        fs::write(&path, bytes).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::SourceError));
    }

    #[test]
    fn active_append_preserves_completed_observation_and_rejects_completed_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let complete = fs::read_to_string(&path).unwrap();
        for tail in [
            "{\"type\":",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":",
            "{\"text\":\"unfinished",
        ] {
            fs::write(&path, format!("{complete}{tail}")).unwrap();
            assert!((read_session(&r).unwrap().0 - 7.0).abs() < 1e-9);
        }
        fs::write(&path, format!("{complete}{{\"type\":\n")).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::Invalid));
        fs::write(&path, format!("{complete}{{oops")).unwrap();
        assert_eq!(read_session(&r), Err(MissingReason::Invalid));
    }

    #[test]
    fn null_info_events_do_not_hide_bound_context_observations() {
        let _serial = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let path = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        let text = fs::read_to_string(&path).unwrap();
        let (metadata, sample) = text.split_once('\n').unwrap();
        let empty = "{\"type\":\"event_msg\",\"timestamp\":\"2026-10-12T00:00:00Z\",\"payload\":{\"type\":\"token_count\",\"info\":null}}\n";
        // Real resumed rollouts can begin with quota-only token_count events.
        // Such events also occur after observations and contain no context sample.
        fs::write(&path, format!("{metadata}\n{empty}{sample}{empty}")).unwrap();
        assert_percent(CodexReader.read(&r).context);
        fs::write(&path, format!("{metadata}\n{empty}")).unwrap();
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::NotFound)
        );
    }
    #[test]
    fn normal_missing_idle_and_mismatch() {
        let _serial = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::NotFound)
        );
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let reading = CodexReader.read(&r);
        assert_percent(reading.context);
        assert_eq!(
            reading.weekly_remaining,
            UsageOutcome::Unavailable(MissingReason::Unsupported)
        );
        assert!(reading.weekly_resets_at.is_none());
        fixture(dir.path(), "session-1", "2026-10-11T23:58:59Z", 26_000);
        assert_percent(CodexReader.read(&r).context);
        fixture(dir.path(), "session-2", "2026-10-12T00:00:00Z", 26_000);
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
    }
    #[test]
    fn compressed_archive_and_ambiguous_duplicate() {
        let _serial = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 26_000);
        let original = dir
            .path()
            .join("sessions/2026/10/12/rollout-test-session-1.jsonl");
        fs::create_dir(dir.path().join("archived_sessions")).unwrap();
        let archive = dir
            .path()
            .join("archived_sessions/rollout-test-session-1.jsonl.zst");
        fs::write(
            &archive,
            zstd::stream::encode_all(fs::read(&original).unwrap().as_slice(), 1).unwrap(),
        )
        .unwrap();
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
        fs::remove_file(original).unwrap();
        assert_percent(CodexReader.read(&r).context);
    }
    #[test]
    fn rejects_invalid_future_timeout_and_unbound_partial_tail() {
        let _serial = TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let id = identity(Provider::Codex);
        let mut r = request(&id, dir.path());
        fixture(dir.path(), "session-1", "2026-10-12T00:00:00Z", 212_001);
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
        fixture(dir.path(), "session-1", "2026-10-12T00:00:01Z", 10);
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Invalid)
        );
        r.timeout = std::time::Duration::ZERO;
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
        r.timeout = std::time::Duration::from_secs(2);
        fs::write(
            dir.path()
                .join("sessions/2026/10/12/rollout-test-session-1.jsonl"),
            "{\"type\":",
        )
        .unwrap();
        assert_eq!(
            CodexReader.read(&r).context,
            UsageOutcome::Unavailable(MissingReason::Unassociated)
        );
    }
}
