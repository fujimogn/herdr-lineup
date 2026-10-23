//! Local repository identity and origin/main commit-time headings.

use std::{
    collections::HashMap,
    io::Read,
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, TimeZone, Timelike};

/// A working repository's common identity and locally known shipment time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub name: String,
    pub ship_unix: Option<i64>,
}

/// Read local Git data only; never fetch or fall back to another commit.
/// Missing refs retain the repository name, while non-working repositories
/// and failed identity queries return `None`.
#[cfg(test)]
fn read_repository(cwd: &Path) -> Option<Repository> {
    match read_repository_controlled(cwd, None) {
        RepositoryRead::Complete(repository) => repository,
        RepositoryRead::Interrupted => None,
    }
}

#[derive(Debug)]
pub(crate) enum RepositoryRead {
    Complete(Option<Repository>),
    Interrupted,
}

/// One heading read together with the instant its data was verified by Git.
/// A cache hit carries the original verification instant and never renews it.
#[derive(Debug)]
pub(crate) struct RepositoryObservation {
    pub(crate) read: RepositoryRead,
    // Publication does not consume the verification age yet; the wiring task
    // that keeps the heading confirmation instant removes this allowance.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) verified_at: Option<Instant>,
}

/// The longest any single heading query may run, whatever the caller allows.
const QUERY_LIMIT: Duration = Duration::from_secs(1);
const CACHE_CAPACITY: usize = 64;
/// A matching fingerprint is trusted only this long after the Git read.
const VERIFIED_TTL: Duration = Duration::from_secs(30);

/// Bound all Git commands for one heading and cancel them on daemon shutdown.
/// Distinguish interruption from confirmed absence so publication can retain a
/// previous heading only when its workspace identity is still authoritative.
pub(crate) fn read_repository_controlled(
    cwd: &Path,
    shutdown: Option<&AtomicBool>,
) -> RepositoryRead {
    // A fresh cache never hits, so this keeps the uncached full Git read.
    RepositoryCache::default()
        .observe(cwd, shutdown, Instant::now() + QUERY_LIMIT)
        .read
}

/// Stat identity of one file or directory the downstream Git reads depend on.
/// Normal absence is a state of its own; any other stat error is inaccessible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileFingerprint {
    Absent,
    Present {
        dev: u64,
        ino: u64,
        size: u64,
        mtime: (i64, i64),
        ctime: (i64, i64),
    },
}

struct Inaccessible;

impl FileFingerprint {
    fn read(path: &Path) -> Result<Self, Inaccessible> {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(path) {
            Ok(meta) => Ok(Self::Present {
                dev: meta.dev(),
                ino: meta.ino(),
                size: meta.size(),
                mtime: (meta.mtime(), meta.mtime_nsec()),
                ctime: (meta.ctime(), meta.ctime_nsec()),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::Absent),
            Err(_) => Err(Inaccessible),
        }
    }
}

/// Every common-directory dependency of `worktree list` and `for-each-ref`
/// for a files-backend repository without replacement refs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CompoundGitFingerprint {
    common_dir: FileFingerprint,
    config: FileFingerprint,
    worktrees: FileFingerprint,
    origin_main: FileFingerprint,
    packed_refs: FileFingerprint,
    alternates: FileFingerprint,
    replace: FileFingerprint,
    reftable: FileFingerprint,
}

impl CompoundGitFingerprint {
    fn read(common: &Path) -> Result<Self, Inaccessible> {
        let file = |relative: &str| FileFingerprint::read(&common.join(relative));
        Ok(Self {
            common_dir: FileFingerprint::read(common)?,
            config: file("config")?,
            worktrees: file("worktrees")?,
            origin_main: file("refs/remotes/origin/main")?,
            packed_refs: file("packed-refs")?,
            alternates: file("objects/info/alternates")?,
            replace: file("refs/replace")?,
            reftable: file("reftable")?,
        })
    }

    /// A replacement directory, empty or not, or a reftable directory makes
    /// stat tracking unsound, so such repositories always run full Git.
    fn trackable(&self) -> bool {
        self.replace == FileFingerprint::Absent && self.reftable == FileFingerprint::Absent
    }
}

#[derive(Debug, Clone)]
struct CachedRepository {
    repository: Repository,
    fingerprint: CompoundGitFingerprint,
    /// Git derives the main root from the common directory, so the common
    /// fingerprint already covers it; this stat is defense in depth.
    root: PathBuf,
    root_fingerprint: FileFingerprint,
    /// Set only by a full Git read; a hit never renews it.
    verified_at: Instant,
    /// Updated by hits to order least-recently-used eviction.
    last_access: Instant,
}

/// Downstream heading cache keyed by canonical Git common directory. Every
/// observation still runs `rev-parse` natively for identity, ownership and
/// repository-format checks; only `worktree list` and `for-each-ref` are
/// elided while the compound fingerprint matches and the entry is fresh.
#[derive(Default)]
pub(crate) struct RepositoryCache {
    entries: HashMap<PathBuf, CachedRepository>,
    #[cfg(test)]
    full_queries: usize,
    #[cfg(test)]
    after_full_query: Option<Box<dyn FnMut()>>,
}

enum Outcome {
    /// `rev-parse` refused cwd, so no common directory is known.
    Rejected,
    /// The common directory is known but its downstream read is unusable.
    Discard(PathBuf),
    Hit(PathBuf),
    Verified {
        common: PathBuf,
        repository: Repository,
        store: Option<Box<CachedRepository>>,
    },
}

impl RepositoryCache {
    /// Observe the heading for `cwd` before the caller's absolute deadline,
    /// itself clamped to the single-query limit. Synchronous stat calls are
    /// not interruptible, so the deadline bounds Git children strictly and
    /// filesystem metadata reads only cooperatively.
    pub(crate) fn observe(
        &mut self,
        cwd: &Path,
        shutdown: Option<&AtomicBool>,
        deadline: Instant,
    ) -> RepositoryObservation {
        let now = Instant::now();
        self.observe_at(cwd, shutdown, deadline.min(now + QUERY_LIMIT), now)
    }

    /// `now` stamps verification and ages entries; cancellation always uses
    /// the real clock against `deadline`.
    fn observe_at(
        &mut self,
        cwd: &Path,
        shutdown: Option<&AtomicBool>,
        deadline: Instant,
        now: Instant,
    ) -> RepositoryObservation {
        let outcome = self.query(cwd, shutdown, deadline, now);
        if Instant::now() >= deadline || shutdown.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            // An interrupted read proves nothing; leave every entry as it was.
            return RepositoryObservation {
                read: RepositoryRead::Interrupted,
                verified_at: None,
            };
        }
        let unverified = RepositoryObservation {
            read: RepositoryRead::Complete(None),
            verified_at: None,
        };
        match outcome {
            Outcome::Rejected => {
                // The refused cwd may belong to any cached repository, and
                // only 64 entries exist: verify everything again from Git.
                self.entries.clear();
                unverified
            }
            Outcome::Discard(common) => {
                self.entries.remove(&common);
                unverified
            }
            Outcome::Hit(common) => {
                let Some(entry) = self.entries.get_mut(&common) else {
                    return unverified;
                };
                entry.last_access = now;
                RepositoryObservation {
                    read: RepositoryRead::Complete(Some(entry.repository.clone())),
                    verified_at: Some(entry.verified_at),
                }
            }
            Outcome::Verified {
                common,
                repository,
                store,
            } => {
                match store {
                    Some(entry) => self.insert(common, *entry),
                    None => {
                        self.entries.remove(&common);
                    }
                }
                RepositoryObservation {
                    read: RepositoryRead::Complete(Some(repository)),
                    verified_at: Some(now),
                }
            }
        }
    }

    fn insert(&mut self, common: PathBuf, entry: CachedRepository) {
        if !self.entries.contains_key(&common) && self.entries.len() >= CACHE_CAPACITY {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, cached)| cached.last_access)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(common, entry);
    }

    fn query(
        &mut self,
        cwd: &Path,
        shutdown: Option<&AtomicBool>,
        deadline: Instant,
        now: Instant,
    ) -> Outcome {
        let output = |path: &Path, args: &[&str]| git_output(path, args, deadline, shutdown);
        let Some(rev_parse) = output(
            cwd,
            &[
                "rev-parse",
                "--is-inside-work-tree",
                "--path-format=absolute",
                "--git-common-dir",
                "--show-ref-format",
            ],
        ) else {
            return Outcome::Rejected;
        };
        // The common directory may contain newlines; the format line cannot.
        // Git without --show-ref-format echoes the option, an unknown backend.
        let Some((common, ref_format)) = rev_parse
            .strip_prefix("true\n")
            .and_then(|value| value.strip_suffix('\n'))
            .and_then(|value| value.rsplit_once('\n'))
        else {
            return Outcome::Rejected;
        };
        let Ok(common) = Path::new(common).canonicalize() else {
            return Outcome::Rejected;
        };
        let Ok(before) = CompoundGitFingerprint::read(&common) else {
            return Outcome::Discard(common);
        };
        let Ok(plain_config) = config_without_includes(&common) else {
            return Outcome::Discard(common);
        };
        let trackable = ref_format == "files" && before.trackable() && plain_config;
        if trackable
            && let Some(entry) = self.entries.get(&common)
            && entry.fingerprint == before
            && now.saturating_duration_since(entry.verified_at) < VERIFIED_TTL
            && FileFingerprint::read(&entry.root).is_ok_and(|root| root == entry.root_fingerprint)
        {
            return Outcome::Hit(common);
        }

        #[cfg(test)]
        {
            self.full_queries += 1;
        }
        // The first worktree is the main working repository, even when cwd is a
        // linked worktree. Query its common directory, not the linked root name.
        let root = output(&common, &["worktree", "list", "--porcelain", "-z"]).and_then(|list| {
            let root = list.split('\0').next()?.strip_prefix("worktree ")?;
            Path::new(root).canonicalize().ok()
        });
        let Some((root, name)) = root.and_then(|root| {
            let name = root.file_name()?.to_str()?.to_owned();
            Some((root, name))
        }) else {
            return Outcome::Discard(common);
        };
        // Unlike log, this format never verifies signatures through a configured
        // GPG program. Keep the format free of signature verification atoms.
        // The refs/replace pattern detects packed-only replacement refs.
        let refs = output(
            cwd,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(symref)%00%(committerdate:unix)",
                "refs/remotes/origin/main",
                "refs/replace",
            ],
        );
        #[cfg(test)]
        if let Some(hook) = self.after_full_query.as_mut() {
            hook();
        }
        let mut ship_unix = None;
        let mut symbolic = false;
        let mut replaced = false;
        for line in refs.as_deref().unwrap_or_default().lines() {
            let mut fields = line.split('\0');
            let (Some(refname), Some(symref), Some(unix)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            // for-each-ref patterns also match descendants such as main/topic.
            if refname == "refs/remotes/origin/main" {
                ship_unix = unix.parse().ok();
                symbolic = !symref.is_empty();
            } else if refname.starts_with("refs/replace/") {
                replaced = true;
            }
        }
        // Data read while a dependency changed may mix two states; never
        // publish or cache it.
        let (Ok(after), Ok(root_fingerprint)) = (
            CompoundGitFingerprint::read(&common),
            FileFingerprint::read(&root),
        ) else {
            return Outcome::Discard(common);
        };
        if after != before || root_fingerprint == FileFingerprint::Absent {
            return Outcome::Discard(common);
        }
        // A symlinked loose ref changes with its target, which is not tracked.
        let linked_ref = std::fs::symlink_metadata(common.join("refs/remotes/origin/main"))
            .is_ok_and(|meta| meta.file_type().is_symlink());
        let repository = Repository { name, ship_unix };
        let store = (trackable && !replaced && !symbolic && !linked_ref && ship_unix.is_some())
            .then(|| {
                Box::new(CachedRepository {
                    repository: repository.clone(),
                    fingerprint: before,
                    root,
                    root_fingerprint,
                    verified_at: now,
                    last_access: now,
                })
            });
        Outcome::Verified {
            common,
            repository,
            store,
        }
    }
}

/// Included files are not tracked by the fingerprint, so a repository whose
/// own configuration includes another file is never cached.
fn config_without_includes(common: &Path) -> Result<bool, Inaccessible> {
    match std::fs::read(common.join("config")) {
        Ok(bytes) => Ok(!bytes
            .to_ascii_lowercase()
            .windows(b"include".len())
            .any(|window| window == b"include")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err(Inaccessible),
    }
}

/// Format using the injected clock's timezone and date. Runtime callers should
/// pass `chrono::Local::now()`; tests can inject a fixed offset and instant.
pub fn format_ship_time<Tz: TimeZone>(ship_unix: Option<i64>, now: &DateTime<Tz>) -> String {
    let Some(ship) = ship_unix.and_then(|unix| now.timezone().timestamp_opt(unix, 0).single())
    else {
        return "—".to_owned();
    };
    if ship.date_naive() == now.date_naive() {
        format!("{:02}:{:02}", ship.hour(), ship.minute())
    } else {
        format!("{}/{}", ship.month(), ship.day())
    }
}

/// Choose workspace cwd first, otherwise the first available pane by id.
/// Callers must supply only panes belonging to this workspace. An available
/// non-Git cwd is authoritative and does not trigger a different pane fallback.
pub fn resolve_workspace_cwd<'a>(
    workspace_cwd: Option<&'a Path>,
    pane_cwds: &[(&str, Option<&'a Path>)],
) -> Option<&'a Path> {
    workspace_cwd.or_else(|| {
        pane_cwds
            .iter()
            .filter_map(|(id, cwd)| cwd.map(|cwd| (*id, cwd)))
            .min_by_key(|(id, _)| *id)
            .map(|(_, cwd)| cwd)
    })
}

fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.env_clear();
    // Preserve executable lookup and the user's Git configuration locations.
    // The heading never contacts a remote, so proxy and authentication state
    // are not needed by this child.
    for key in [
        "PATH",
        "HOME",
        "TMPDIR",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .arg("-C")
        .arg(cwd)
        .args(["--no-pager", "-c", "core.fsmonitor=false"]);
    // These built-in, read-only queries do not invoke aliases or hooks; disable
    // pager and fsmonitor explicitly and never request signature verification.
    // Local object reads must not lazily fetch absent partial-clone objects.
    // Deny remote protocols as well, including on older Git versions.
    command
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "");
    command
}

struct GitChild {
    child: Child,
    reaped: bool,
}

impl Drop for GitChild {
    fn drop(&mut self) {
        if !self.reaped {
            // The unreaped child pins its PID; never signal a recycled group.
            // SAFETY: process_group(0) creates this child's own process group.
            unsafe {
                libc::killpg(self.child.id() as i32, libc::SIGKILL);
            }
            let _ = self.child.kill();
            loop {
                match self.child.wait() {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    _ => break,
                }
            }
        }
    }
}

fn git_output(
    cwd: &Path,
    args: &[&str],
    deadline: Instant,
    shutdown: Option<&AtomicBool>,
) -> Option<String> {
    let cancelled =
        || Instant::now() >= deadline || shutdown.is_some_and(|flag| flag.load(Ordering::Acquire));
    if cancelled() {
        return None;
    }
    let child = git_command(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let mut owned = GitChild {
        child,
        reaped: false,
    };
    let mut stdout = owned.child.stdout.take()?;
    let fd = stdout.as_raw_fd();
    // SAFETY: stdout owns fd, and only this reader changes its file flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return None;
    }
    let mut bytes = Vec::new();
    let mut eof = false;
    loop {
        if cancelled() {
            return None;
        }
        if !eof {
            let mut buffer = [0u8; 8192];
            match stdout.read(&mut buffer) {
                Ok(0) => eof = true,
                Ok(count) => {
                    if bytes.len() + count > 4 * 1024 * 1024 {
                        return None;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    continue;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => return None,
            }
        }
        // Do not reap the group leader while a descendant still owns stdout:
        // a timeout must be able to kill that exact group without PID reuse.
        // WNOWAIT preserves the exited leader and therefore pins the process
        // group ID until descendants have been killed, even after stdout EOF.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if eof {
            // SAFETY: info is writable, and this unreaped PID belongs to owned.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    owned.child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return None;
            }
        }
        if eof && info.si_signo == libc::SIGCHLD {
            // SAFETY: WNOWAIT kept the group leader unreaped, preventing reuse.
            unsafe {
                libc::killpg(owned.child.id() as i32, libc::SIGKILL);
            }
            let status = owned.child.wait().ok()?;
            owned.reaped = true;
            return status
                .success()
                .then(|| String::from_utf8(bytes).ok())
                .flatten();
        }
        if !eof {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let millis = remaining.as_millis().clamp(1, 50) as i32;
            let mut descriptor = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut descriptor, 1, millis) };
            if ready < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return None;
            }
        } else {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let millis = remaining.as_millis().clamp(1, 5) as i32;
            let ready = unsafe { libc::poll(std::ptr::null_mut(), 0, millis) };
            if ready < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use std::path::PathBuf;

    #[test]
    fn completed_git_kills_descendants_with_closed_stdout() {
        if let Some(root) = std::env::var_os("LINEUP_TEST_GIT_DESCENDANT") {
            let root = PathBuf::from(root);
            assert_eq!(read_repository(&root), None);
            let pid: i32 = std::fs::read_to_string(root.join("descendant-pid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            let mut gone = false;
            while Instant::now() < deadline {
                if unsafe { libc::kill(pid, 0) } < 0 {
                    gone = std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if !gone {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
            assert!(gone, "successful Git exit left a running descendant");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let script = bin.join("git");
        crate::test_support::write_executable(&script, "#!/bin/sh\ntest -z \"$LINEUP_TEST_GIT_DESCENDANT\" || exit 42\ntest -n \"$HOME\" || exit 43\n/bin/sleep 30 >/dev/null 2>&1 &\necho $! > \"$2/descendant-pid\"\nprintf 'false\\n'\nexit 0\n").unwrap();

        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ship_time::tests::completed_git_kills_descendants_with_closed_stdout",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", dir.path())
            .env("PATH", bin)
            .env("LINEUP_TEST_GIT_DESCENDANT", dir.path())
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn repository_deadline_reaps_stalled_git_without_shutdown() {
        if let Some(root) = std::env::var_os("LINEUP_TEST_GIT_DEADLINE") {
            let root = PathBuf::from(root);
            let started = Instant::now();
            assert!(matches!(
                read_repository_controlled(&root, None),
                RepositoryRead::Interrupted
            ));
            assert!(started.elapsed() < Duration::from_secs(2));
            let pid: i32 = std::fs::read_to_string(root.join("git-pid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let script = bin.join("git");
        crate::test_support::write_executable(
            &script,
            "#!/bin/sh\necho $$ > \"$2/git-pid\"\nexec /bin/sleep 30\n",
        )
        .unwrap();

        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ship_time::tests::repository_deadline_reaps_stalled_git_without_shutdown",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", dir.path())
            .env("PATH", bin)
            .env("LINEUP_TEST_GIT_DEADLINE", dir.path())
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn at(value: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(value).unwrap()
    }

    #[test]
    fn today_uses_zero_padded_local_hour_and_minute() {
        let now = at("2026-10-12T23:59:59+09:00");
        let ship = at("2026-10-12T09:05:00+09:00");
        assert_eq!(format_ship_time(Some(ship.timestamp()), &now), "09:05");
    }

    #[test]
    fn yesterday_and_future_dates_use_unpadded_month_and_day() {
        let now = at("2026-01-02T00:00:00+09:00");
        assert_eq!(
            format_ship_time(Some(at("2026-01-01T23:59:59+09:00").timestamp()), &now),
            "1/1"
        );
        assert_eq!(
            format_ship_time(Some(at("2026-01-03T00:00:00+09:00").timestamp()), &now),
            "1/3"
        );
        assert_eq!(
            format_ship_time(Some(at("2025-01-02T00:00:00+09:00").timestamp()), &now),
            "1/2"
        );
    }

    #[test]
    fn crossing_local_midnight_reformats_the_same_commit() {
        let ship = at("2026-12-31T23:59:59+09:00").timestamp();
        assert_eq!(
            format_ship_time(Some(ship), &at("2026-12-31T23:59:59+09:00")),
            "23:59"
        );
        assert_eq!(
            format_ship_time(Some(ship), &at("2027-01-01T00:00:00+09:00")),
            "12/31"
        );
    }

    #[test]
    fn formatting_uses_the_injected_timezone_instead_of_utc() {
        let ship = at("2026-10-11T23:05:00Z").timestamp();
        assert_eq!(
            format_ship_time(Some(ship), &at("2026-10-12T12:00:00+09:00")),
            "08:05"
        );
        assert_eq!(
            format_ship_time(Some(ship), &at("2026-10-12T12:00:00-07:00")),
            "10/11"
        );
        assert_eq!(
            format_ship_time(Some(ship), &at("2026-10-11T23:30:00-07:00")),
            "16:05"
        );
    }

    #[test]
    fn absent_or_out_of_range_timestamp_is_unavailable() {
        let now = at("2026-10-12T12:00:00+09:00");
        assert_eq!(format_ship_time(None, &now), "—");
        assert_eq!(format_ship_time(Some(i64::MAX), &now), "—");
    }

    #[test]
    fn workspace_cwd_precedes_lexicographic_pane_fallback() {
        let first = Path::new("/first");
        let second = Path::new("/second");
        let panes = [("p2", Some(second)), ("p0", None), ("p1", Some(first))];
        assert_eq!(resolve_workspace_cwd(None, &panes), Some(first));
        assert_eq!(resolve_workspace_cwd(Some(second), &panes), Some(second));
        assert_eq!(resolve_workspace_cwd(None, &[]), None);
    }

    // The guard exclusively owns synthetic Git data and removes it on drop,
    // including when a test assertion panics.
    struct Fixture {
        root: PathBuf,
        repo: PathBuf,
        _cleanup: tempfile::TempDir,
    }

    #[test]
    fn dropping_fixture_removes_its_git_tree() {
        // Own the RED-test parent so a failed assertion still cleans its tree.
        let parent = tempfile::tempdir().unwrap();
        let fixture = Fixture::new_in(parent.path());
        let root = fixture.root.clone();
        drop(fixture);
        assert!(!root.exists(), "fixture tree must be removed on drop");
    }

    impl Fixture {
        fn new() -> Self {
            Self::new_in(&std::env::temp_dir())
        }

        fn new_in(parent: &Path) -> Self {
            Self::init_in(parent, &[])
        }

        fn init_in(parent: &Path, init: &[&str]) -> Self {
            let cleanup = tempfile::Builder::new()
                .prefix("herdr-lineup-ship-time-")
                .tempdir_in(parent)
                .unwrap();
            let root = cleanup.path().to_owned();
            // TMPDIR can be inside a real repository. This deliberately
            // unresolved Git marker stops discovery at our fixture boundary,
            // even though production commands discard GIT_CEILING_DIRECTORIES.
            std::fs::write(root.join(".git"), "gitdir: unavailable-fixture-git-dir\n").unwrap();
            let repo = root.join("canonical-repo");
            std::fs::create_dir(&repo).unwrap();
            let fixture = Self {
                root,
                repo,
                _cleanup: cleanup,
            };
            fixture.git(&[&["init", "--initial-branch=main"], init].concat(), None);
            fixture.commit("2024-01-01T00:00:00Z");
            fixture
        }

        fn git(&self, args: &[&str], date: Option<&str>) -> String {
            fixture_git(&self.repo, args, date)
        }

        fn commit(&self, date: &str) {
            self.git(
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "synthetic",
                ],
                Some(date),
            );
        }
    }

    fn fixture_git(repo: &Path, args: &[&str], date: Option<&str>) -> String {
        let mut command = Command::new("git");
        command.arg("-C").arg(repo).args(args);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        if let Some(date) = date {
            command
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn missing_origin_main_keeps_repository_name_without_substituting_head() {
        let fixture = Fixture::new();
        assert_eq!(
            read_repository(&fixture.repo),
            Some(Repository {
                name: "canonical-repo".to_owned(),
                ship_unix: None
            })
        );
    }

    #[test]
    fn reads_local_origin_main_committer_time_and_refreshes_ref() {
        let fixture = Fixture::new();
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        fixture.commit("2024-01-02T00:00:00Z");
        assert_eq!(
            read_repository(&fixture.repo).unwrap().ship_unix,
            Some(1_704_067_200)
        );
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        assert_eq!(
            read_repository(&fixture.repo).unwrap().ship_unix,
            Some(1_704_153_600)
        );
    }

    #[test]
    fn packed_refs_are_read_and_refreshed_correctly() {
        let fixture = Fixture::new();
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        assert_eq!(
            read_repository(&fixture.repo).unwrap().ship_unix,
            Some(1_704_067_200)
        );
        // Pack loose refs into .git/packed-refs
        fixture.git(&["pack-refs", "--all"], None);
        assert!(!fixture.repo.join(".git/refs/remotes/origin/main").exists());
        assert!(fixture.repo.join(".git/packed-refs").exists());
        assert_eq!(
            read_repository(&fixture.repo).unwrap().ship_unix,
            Some(1_704_067_200)
        );
        // Update ref after packing (creates a loose ref that shadows packed-refs)
        fixture.commit("2024-01-02T00:00:00Z");
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        assert!(fixture.repo.join(".git/refs/remotes/origin/main").exists());
        assert_eq!(
            read_repository(&fixture.repo).unwrap().ship_unix,
            Some(1_704_153_600)
        );
        // Pack again
        fixture.git(&["pack-refs", "--all"], None);
        assert!(!fixture.repo.join(".git/refs/remotes/origin/main").exists());
        assert_eq!(
            read_repository(&fixture.repo).unwrap().ship_unix,
            Some(1_704_153_600)
        );
    }

    #[test]
    fn replace_refs_alter_committer_time_and_require_invalidation() {
        let fixture = Fixture::new();
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        let initial_time = read_repository(&fixture.repo).unwrap().ship_unix;
        assert_eq!(initial_time, Some(1_704_067_200));

        // Create a second commit with different timestamp
        fixture.commit("2024-01-02T00:00:00Z");
        let head_commit = fixture.git(&["rev-parse", "HEAD"], None);
        let origin_main_commit = fixture.git(&["rev-parse", "refs/remotes/origin/main"], None);

        // Replace origin/main target commit with head_commit
        fixture.git(&["replace", &origin_main_commit, &head_commit], None);

        // Git for-each-ref respects git replace by default, yielding the replaced commit time
        let replaced_time = read_repository(&fixture.repo).unwrap().ship_unix;
        assert_eq!(replaced_time, Some(1_704_153_600));

        // Pack replacement refs into packed-refs. Prune unlinks loose ref files,
        // leaving the directory empty. Verify and remove the empty directory.
        fixture.git(&["pack-refs", "--all", "--prune"], None);
        let loose_replace = fixture.repo.join(".git/refs/replace");
        if loose_replace.exists() {
            std::fs::remove_dir(&loose_replace)
                .expect("pack-refs --prune must leave refs/replace empty");
        }
        assert!(!loose_replace.exists());
        // Packed-only replacement ref is still effective and yields replaced commit time
        let packed_replaced_time = read_repository(&fixture.repo).unwrap().ship_unix;
        assert_eq!(packed_replaced_time, Some(1_704_153_600));

        // Delete replace ref
        fixture.git(&["replace", "-d", &origin_main_commit], None);
        let restored_time = read_repository(&fixture.repo).unwrap().ship_unix;
        assert_eq!(restored_time, Some(1_704_067_200));
    }

    impl Fixture {
        fn signed_origin_main(&self) {
            // A synthetic signature triggers Git's verifier without any keys.
            let commit = self.git(&["cat-file", "commit", "HEAD"], None);
            let commit = commit.replacen(
                "\n\n",
                "\ngpgsig -----BEGIN PGP SIGNATURE-----\n synthetic\n -----END PGP SIGNATURE-----\n\n",
                1,
            );
            let path = self.root.join("signed-commit");
            std::fs::write(&path, commit).unwrap();
            let oid = self.git(
                &["hash-object", "-t", "commit", "-w", path.to_str().unwrap()],
                None,
            );
            self.git(&["update-ref", "refs/remotes/origin/main", &oid], None);
        }

        fn hostile_config(&self) -> Vec<PathBuf> {
            let script = self.root.join("external-program");
            let body = "#!/bin/sh\nprintf 'executed\\n' > \"$0.marker\"\nexit 1\n";
            crate::test_support::write_executable(&script, body).unwrap();
            let program = script.to_str().unwrap();
            let shell_program = format!("'{}'", program.replace('\'', "'\\''"));
            for key in [
                "core.fsmonitor",
                "core.pager",
                "gpg.program",
                "gpg.openpgp.program",
                "gpg.ssh.program",
                "gpg.x509.program",
                "core.sshCommand",
                "core.gitProxy",
                "diff.external",
            ] {
                self.git(&["config", key, program], None);
            }
            for key in ["rev-parse", "worktree", "log", "for-each-ref"] {
                self.git(
                    &[
                        "config",
                        &format!("alias.{key}"),
                        &format!("!{shell_program}"),
                    ],
                    None,
                );
                self.git(&["config", &format!("pager.{key}"), "true"], None);
            }
            self.git(
                &["config", "credential.helper", &format!("!{shell_program}")],
                None,
            );
            self.git(&["config", "log.showSignature", "true"], None);
            self.git(&["config", "gpg.format", "openpgp"], None);
            self.git(&["config", "protocol.allow", "always"], None);
            self.git(
                &["config", "remote.origin.url", "ssh://fixture.invalid/repo"],
                None,
            );
            let hooks = self.root.join("hooks");
            std::fs::create_dir(&hooks).unwrap();
            let mut markers = vec![script.with_extension("marker")];
            for hook in [
                "post-checkout",
                "post-index-change",
                "reference-transaction",
                "pre-auto-gc",
                "pre-command",
                "post-command",
            ] {
                let path = hooks.join(hook);
                crate::test_support::write_executable(&path, body).unwrap();
                markers.push(path.with_extension("marker"));
            }
            self.git(&["config", "core.hooksPath", hooks.to_str().unwrap()], None);
            markers
        }
    }

    #[test]
    fn repository_reads_do_not_execute_repo_configured_programs() {
        let fixture = Fixture::new();
        fixture.signed_origin_main();
        let linked = fixture.root.join("detached-worktree");
        fixture.git(
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
            None,
        );
        let markers = fixture.hostile_config();
        // Both entry points exercise every production Git invocation, including
        // worktree list from the common directory and both rev-parse queries.
        for cwd in [&fixture.repo, &linked] {
            let repository = read_repository(cwd);
            for marker in &markers {
                assert!(
                    !marker.exists(),
                    "Git executed a configured program: {marker:?}"
                );
            }
            assert_eq!(
                repository,
                Some(Repository {
                    name: "canonical-repo".to_owned(),
                    ship_unix: Some(1_704_067_200),
                })
            );
        }
    }

    #[test]
    fn missing_or_broken_origin_main_does_not_run_configured_programs() {
        for ref_contents in [
            None,
            Some("broken\n"),
            Some("1111111111111111111111111111111111111111\n"),
        ] {
            let fixture = Fixture::new();
            // Exercise the missing-object path of a partial clone as well.
            fixture.git(&["config", "remote.origin.promisor", "true"], None);
            fixture.git(
                &["config", "remote.origin.partialclonefilter", "blob:none"],
                None,
            );
            let refs = fixture.repo.join(".git/refs/remotes/origin");
            std::fs::create_dir_all(&refs).unwrap();
            if let Some(contents) = ref_contents {
                std::fs::write(refs.join("main"), contents).unwrap();
            }
            let markers = fixture.hostile_config();
            // Missing objects must not reach SSH, credential helpers, or an
            // external transport even if repository config permits protocols.
            for remote in [
                "ssh://fixture.invalid/repo".to_owned(),
                "https://fixture.invalid/repo".to_owned(),
                format!("ext::{}", fixture.root.join("external-program").display()),
            ] {
                fixture.git(&["config", "remote.origin.url", &remote], None);
                let repository = read_repository(&fixture.repo);
                for marker in &markers {
                    assert!(
                        !marker.exists(),
                        "Git executed a configured program: {marker:?}"
                    );
                }
                assert_eq!(
                    repository,
                    Some(Repository {
                        name: "canonical-repo".to_owned(),
                        ship_unix: None,
                    })
                );
            }
        }
    }

    #[test]
    fn origin_main_prefix_does_not_substitute_a_different_ref() {
        let fixture = Fixture::new();
        fixture.git(
            &["update-ref", "refs/remotes/origin/main/topic", "HEAD"],
            None,
        );
        assert_eq!(read_repository(&fixture.repo).unwrap().ship_unix, None);
    }

    #[test]
    fn linked_worktree_uses_canonical_repository_name() {
        let fixture = Fixture::new();
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        let linked = fixture.root.join("different-worktree-name");
        fixture.git(
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
            None,
        );
        assert_eq!(
            read_repository(&linked),
            Some(Repository {
                name: "canonical-repo".to_owned(),
                ship_unix: Some(1_704_067_200)
            })
        );
        assert_eq!(read_repository(&fixture.root), None);
        assert_eq!(read_repository(&fixture.repo.join(".git")), None);
    }

    #[test]
    fn linked_worktree_lifecycle_add_and_remove() {
        let fixture = Fixture::new();
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        let linked = fixture.root.join("ephemeral-worktree");
        fixture.git(
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
            None,
        );
        assert_eq!(
            read_repository(&linked),
            Some(Repository {
                name: "canonical-repo".to_owned(),
                ship_unix: Some(1_704_067_200),
            })
        );
        fixture.git(&["worktree", "remove", linked.to_str().unwrap()], None);
        assert_eq!(read_repository(&linked), None);
        // Main repository is unaffected
        assert_eq!(
            read_repository(&fixture.repo),
            Some(Repository {
                name: "canonical-repo".to_owned(),
                ship_unix: Some(1_704_067_200),
            })
        );
    }

    #[test]
    fn git_command_sets_only_required_environment() {
        let command = git_command(Path::new("/repository"));
        for (key, value) in command.get_envs() {
            assert!(value.is_some());
            assert!(
                [
                    "PATH",
                    "HOME",
                    "TMPDIR",
                    "XDG_CONFIG_HOME",
                    "XDG_CACHE_HOME",
                    "GIT_NO_LAZY_FETCH",
                    "GIT_ALLOW_PROTOCOL"
                ]
                .iter()
                .any(|allowed| key == *allowed)
            );
        }
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "-C",
                "/repository",
                "--no-pager",
                "-c",
                "core.fsmonitor=false"
            ]
        );
    }

    #[test]
    fn git_command_disables_promisor_fetch_and_all_remote_protocols() {
        let command = git_command(Path::new("/repository"));
        let env: Vec<_> = command.get_envs().collect();
        assert!(env.contains(&(
            std::ffi::OsStr::new("GIT_NO_LAZY_FETCH"),
            Some(std::ffi::OsStr::new("1"))
        )));
        assert!(env.contains(&(
            std::ffi::OsStr::new("GIT_ALLOW_PROTOCOL"),
            Some(std::ffi::OsStr::new(""))
        )));
    }

    #[test]
    fn repository_path_with_embedded_newline_is_preserved() {
        let fixture = Fixture::new();
        let newline_repo = fixture.root.join("repo\nwith\nnewline");
        std::fs::create_dir(&newline_repo).unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(&newline_repo)
            .args(["init", "--initial-branch=main"])
            .status()
            .unwrap();
        assert!(status.success());
        let repo = read_repository(&newline_repo).expect("must parse repo path with newlines");
        assert_eq!(repo.name, "repo\nwith\nnewline");
    }

    // Cache fixtures inject the verification clock; Git children still run
    // against the real clock with the production one-second limit.
    fn observe(cache: &mut RepositoryCache, cwd: &Path, now: Instant) -> RepositoryObservation {
        cache.observe_at(cwd, None, Instant::now() + QUERY_LIMIT, now)
    }

    fn ship(observation: &RepositoryObservation) -> Option<i64> {
        match &observation.read {
            RepositoryRead::Complete(Some(repository)) => repository.ship_unix,
            other => panic!("expected a repository, got {other:?}"),
        }
    }

    fn is_unverified_absence(observation: &RepositoryObservation) -> bool {
        matches!(observation.read, RepositoryRead::Complete(None))
            && observation.verified_at.is_none()
    }

    const DAY_ONE: i64 = 1_704_067_200;
    const DAY_TWO: i64 = 1_704_153_600;

    impl Fixture {
        fn with_origin_main() -> Self {
            let fixture = Self::new();
            fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
            fixture
        }

        fn common(&self) -> PathBuf {
            self.repo.join(".git").canonicalize().unwrap()
        }

        fn linked(&self, name: &str) -> PathBuf {
            let linked = self.root.join(name);
            self.git(
                &[
                    "worktree",
                    "add",
                    "--detach",
                    linked.to_str().unwrap(),
                    "HEAD",
                ],
                None,
            );
            linked
        }
    }

    #[test]
    fn subdirectory_cwd_hits_cache_after_one_full_query() {
        let fixture = Fixture::with_origin_main();
        let deep = fixture.repo.join("src/deep");
        std::fs::create_dir_all(&deep).unwrap();
        // Normal absence is a fingerprint state and still allows hits.
        assert!(!fixture.repo.join(".git/packed-refs").exists());
        assert!(!fixture.repo.join(".git/worktrees").exists());
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        let first = observe(&mut cache, &deep, t0);
        assert_eq!(
            first.read.complete(),
            Some(Repository {
                name: "canonical-repo".to_owned(),
                ship_unix: Some(DAY_ONE),
            })
        );
        assert_eq!(first.verified_at, Some(t0));
        let second = observe(&mut cache, &deep, t0 + Duration::from_secs(5));
        assert_eq!(ship(&second), Some(DAY_ONE));
        assert_eq!(second.verified_at, Some(t0));
        assert_eq!(cache.full_queries, 1);
        let entry = &cache.entries[&fixture.common()];
        assert_eq!(entry.verified_at, t0);
        assert_eq!(entry.last_access, t0 + Duration::from_secs(5));
    }

    #[test]
    fn linked_worktrees_share_the_entry_and_inherit_its_verification() {
        let fixture = Fixture::with_origin_main();
        let first = fixture.linked("first-linked");
        let second = fixture.linked("second-linked");
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        assert_eq!(observe(&mut cache, &fixture.repo, t0).verified_at, Some(t0));
        for (offset, cwd) in [(2, &first), (4, &second)] {
            let observation = observe(&mut cache, cwd, t0 + Duration::from_secs(offset));
            assert_eq!(observation.read.complete().unwrap().name, "canonical-repo");
            assert_eq!(observation.verified_at, Some(t0));
        }
        assert_eq!(cache.full_queries, 1);
    }

    #[test]
    fn worktree_add_and_remove_force_full_queries() {
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        let linked = fixture.linked("added-later");
        assert_eq!(ship(&observe(&mut cache, &linked, t0)), Some(DAY_ONE));
        assert_eq!(cache.full_queries, 2);
        fixture.git(&["worktree", "remove", linked.to_str().unwrap()], None);
        // The removed worktree is refused by rev-parse and clears the cache.
        assert!(is_unverified_absence(&observe(&mut cache, &linked, t0)));
        assert!(cache.entries.is_empty());
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        assert_eq!(cache.full_queries, 3);
    }

    #[test]
    fn loose_and_packed_origin_main_changes_force_full_queries() {
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        fixture.commit("2024-01-02T00:00:00Z");
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_TWO));
        assert_eq!(cache.full_queries, 2);
        fixture.git(&["pack-refs", "--all"], None);
        assert!(!fixture.repo.join(".git/refs/remotes/origin/main").exists());
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_TWO));
        assert_eq!(cache.full_queries, 3);
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_TWO));
        assert_eq!(cache.full_queries, 3);
        // A packed value later shadowed by a new loose ref.
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD~"], None);
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        assert_eq!(cache.full_queries, 4);
    }

    impl Fixture {
        /// Replace the origin/main commit with HEAD, a later commit.
        fn replace_origin_main(&self) {
            let head = self.git(&["rev-parse", "HEAD"], None);
            let origin = self.git(&["rev-parse", "refs/remotes/origin/main"], None);
            self.git(&["replace", &origin, &head], None);
        }
    }

    #[test]
    fn empty_replace_directory_after_warm_cache_disables_caching() {
        let fixture = Fixture::with_origin_main();
        fixture.commit("2024-01-02T00:00:00Z");
        let replace = fixture.repo.join(".git/refs/replace");
        assert!(!replace.exists());
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        assert!(cache.entries.contains_key(&fixture.common()));
        std::fs::create_dir(&replace).unwrap();
        let t1 = t0 + Duration::from_secs(1);
        let observation = observe(&mut cache, &fixture.repo, t1);
        assert_eq!(
            (ship(&observation), observation.verified_at),
            (Some(DAY_ONE), Some(t1))
        );
        assert!(cache.entries.is_empty(), "uncacheable read must evict");
        observe(&mut cache, &fixture.repo, t1);
        assert_eq!(cache.full_queries, 3);
        fixture.replace_origin_main();
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t1)), Some(DAY_TWO));
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn initially_empty_replace_directory_is_never_cached() {
        let fixture = Fixture::with_origin_main();
        fixture.commit("2024-01-02T00:00:00Z");
        std::fs::create_dir(fixture.repo.join(".git/refs/replace")).unwrap();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        for _ in 0..2 {
            assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        }
        assert_eq!(cache.full_queries, 2);
        fixture.replace_origin_main();
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_TWO));
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn packed_only_replacement_is_never_cached() {
        let fixture = Fixture::with_origin_main();
        fixture.commit("2024-01-02T00:00:00Z");
        fixture.replace_origin_main();
        fixture.git(&["pack-refs", "--all", "--prune"], None);
        let loose = fixture.repo.join(".git/refs/replace");
        if loose.exists() {
            std::fs::remove_dir(&loose).expect("pack-refs --prune must leave refs/replace empty");
        }
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        for offset in [0, 1] {
            let now = t0 + Duration::from_secs(offset);
            let observation = observe(&mut cache, &fixture.repo, now);
            assert_eq!(
                (ship(&observation), observation.verified_at),
                (Some(DAY_TWO), Some(now))
            );
        }
        assert_eq!(cache.full_queries, 2);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn reftable_repository_is_never_cached() {
        let fixture = Fixture::init_in(&std::env::temp_dir(), &["--ref-format=reftable"]);
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        fixture.commit("2024-01-02T00:00:00Z");
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_TWO));
        assert_eq!(cache.full_queries, 2);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn symbolic_origin_main_is_never_cached() {
        let fixture = Fixture::new();
        fixture.git(
            &[
                "symbolic-ref",
                "refs/remotes/origin/main",
                "refs/heads/main",
            ],
            None,
        );
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        // The symref file is unchanged while its target moves.
        fixture.commit("2024-01-02T00:00:00Z");
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_TWO));
        assert_eq!(cache.full_queries, 2);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn missing_ref_or_object_is_never_cached() {
        let fixture = Fixture::new();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        for _ in 0..2 {
            let observation = observe(&mut cache, &fixture.repo, t0);
            assert_eq!(
                (ship(&observation), observation.verified_at),
                (None, Some(t0))
            );
        }
        // A ref to an absent object reads as unavailable until the object exists.
        let commit = fixture.git(&["cat-file", "commit", "HEAD"], None);
        let commit = commit.replace("synthetic", "written later");
        let path = fixture.root.join("later-commit");
        std::fs::write(&path, commit).unwrap();
        let oid = fixture.git(
            &["hash-object", "-t", "commit", path.to_str().unwrap()],
            None,
        );
        let refs = fixture.repo.join(".git/refs/remotes/origin");
        std::fs::create_dir_all(&refs).unwrap();
        std::fs::write(refs.join("main"), format!("{oid}\n")).unwrap();
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), None);
        fixture.git(
            &["hash-object", "-t", "commit", "-w", path.to_str().unwrap()],
            None,
        );
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        assert_eq!(cache.full_queries, 4);
        assert!(cache.entries.contains_key(&fixture.common()));
    }

    #[test]
    fn dependency_change_during_full_query_discards_the_result() {
        let fixture = Fixture::with_origin_main();
        let first = fixture.git(&["rev-parse", "HEAD"], None);
        fixture.commit("2024-01-02T00:00:00Z");
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        let repo = fixture.repo.clone();
        cache.after_full_query = Some(Box::new(move || {
            fixture_git(
                &repo,
                &["update-ref", "refs/remotes/origin/main", &first],
                None,
            );
        }));
        assert!(is_unverified_absence(&observe(
            &mut cache,
            &fixture.repo,
            t0
        )));
        assert!(cache.entries.is_empty(), "the old entry must be discarded");
        cache.after_full_query = None;
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        assert_eq!(cache.full_queries, 3);
    }

    #[test]
    fn verified_entry_expires_after_thirty_seconds_without_renewal() {
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        let almost = t0 + VERIFIED_TTL - Duration::from_millis(1);
        assert_eq!(
            observe(&mut cache, &fixture.repo, almost).verified_at,
            Some(t0)
        );
        assert_eq!(cache.full_queries, 1);
        let expired = t0 + VERIFIED_TTL;
        assert_eq!(
            observe(&mut cache, &fixture.repo, expired).verified_at,
            Some(expired)
        );
        assert_eq!(cache.full_queries, 2);
        let later = expired + Duration::from_secs(1);
        assert_eq!(
            observe(&mut cache, &fixture.repo, later).verified_at,
            Some(expired)
        );
        assert_eq!(cache.full_queries, 2);
    }

    #[test]
    fn config_change_forces_and_included_config_disables_caching() {
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        fixture.git(&["config", "fixture.key", "value"], None);
        observe(&mut cache, &fixture.repo, t0);
        assert_eq!(cache.full_queries, 2);
        // The fingerprint cannot follow included files.
        let included = fixture.root.join("included.cfg");
        std::fs::write(&included, "[fixture]\n\tother = value\n").unwrap();
        fixture.git(
            &["config", "include.path", included.to_str().unwrap()],
            None,
        );
        for _ in 0..2 {
            assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        }
        assert_eq!(cache.full_queries, 4);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn rejected_identity_clears_every_entry() {
        let fixture = Fixture::with_origin_main();
        let linked = fixture.linked("broken-pointer");
        let other = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        observe(&mut cache, &other.repo, t0);
        assert_eq!(cache.entries.len(), 2);
        std::fs::write(linked.join(".git"), "gitdir: /nonexistent/fixture/gitdir\n").unwrap();
        assert!(is_unverified_absence(&observe(&mut cache, &linked, t0)));
        assert!(cache.entries.is_empty());
        // Every repository is verified again from a full read.
        let t1 = t0 + Duration::from_secs(1);
        assert_eq!(observe(&mut cache, &fixture.repo, t1).verified_at, Some(t1));
        assert_eq!(cache.full_queries, 3);
    }

    #[test]
    fn unsupported_repository_format_is_refused_every_time() {
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        fixture.git(&["config", "core.repositoryformatversion", "1"], None);
        fixture.git(&["config", "extensions.fixtureUnknown", "true"], None);
        for _ in 0..2 {
            assert!(is_unverified_absence(&observe(
                &mut cache,
                &fixture.repo,
                t0
            )));
        }
        assert!(cache.entries.is_empty());
        assert_eq!(cache.full_queries, 1);
    }

    #[test]
    fn reinitialized_repository_at_the_same_path_forces_a_full_query() {
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        std::fs::remove_dir_all(fixture.repo.join(".git")).unwrap();
        fixture.git(&["init", "--initial-branch=main"], None);
        fixture.commit("2024-01-02T00:00:00Z");
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_TWO));
        assert_eq!(cache.full_queries, 2);
    }

    #[test]
    fn renamed_main_worktree_is_read_under_its_new_identity() {
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        // The main root contains the common directory, so moving the root
        // changes the cache key; the old entry no longer serves any cwd.
        let renamed = fixture.root.join("renamed-repo");
        std::fs::rename(&fixture.repo, &renamed).unwrap();
        assert!(is_unverified_absence(&observe(
            &mut cache,
            &fixture.repo,
            t0
        )));
        let observation = observe(&mut cache, &renamed, t0);
        assert_eq!(observation.read.complete().unwrap().name, "renamed-repo");
        assert_eq!(cache.full_queries, 2);
    }

    // Restores a directory mode even when an assertion panics.
    struct ModeGuard(PathBuf);

    impl Drop for ModeGuard {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    #[test]
    fn inaccessible_dependency_fails_closed_and_discards_the_entry() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        let origin = fixture.repo.join(".git/refs/remotes/origin");
        let guard = ModeGuard(origin.clone());
        std::fs::set_permissions(&origin, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(is_unverified_absence(&observe(
            &mut cache,
            &fixture.repo,
            t0
        )));
        assert!(cache.entries.is_empty());
        drop(guard);
        assert_eq!(ship(&observe(&mut cache, &fixture.repo, t0)), Some(DAY_ONE));
        assert_eq!(cache.full_queries, 2);
    }

    #[test]
    fn interrupted_observation_leaves_the_entry_unchanged() {
        let fixture = Fixture::with_origin_main();
        let mut cache = RepositoryCache::default();
        let t0 = Instant::now();
        observe(&mut cache, &fixture.repo, t0);
        // The caller deadline wins over the query limit and spawns nothing.
        let elapsed = cache.observe(&fixture.repo, None, Instant::now());
        assert!(matches!(elapsed.read, RepositoryRead::Interrupted));
        assert_eq!(elapsed.verified_at, None);
        // A full query that completes after its deadline stores nothing.
        fixture.commit("2024-01-02T00:00:00Z");
        fixture.git(&["update-ref", "refs/remotes/origin/main", "HEAD"], None);
        cache.after_full_query = Some(Box::new(|| std::thread::sleep(Duration::from_millis(300))));
        let late = cache.observe_at(
            &fixture.repo,
            None,
            Instant::now() + Duration::from_millis(200),
            t0 + Duration::from_secs(1),
        );
        assert!(matches!(late.read, RepositoryRead::Interrupted));
        assert_eq!(late.verified_at, None);
        let entry = &cache.entries[&fixture.common()];
        assert_eq!(
            (entry.verified_at, entry.repository.ship_unix),
            (t0, Some(DAY_ONE))
        );
        assert_eq!(cache.full_queries, 2);
    }

    #[test]
    fn full_cache_evicts_the_least_recently_accessed_entry() {
        let t0 = Instant::now();
        let fingerprint = CompoundGitFingerprint {
            common_dir: FileFingerprint::Absent,
            config: FileFingerprint::Absent,
            worktrees: FileFingerprint::Absent,
            origin_main: FileFingerprint::Absent,
            packed_refs: FileFingerprint::Absent,
            alternates: FileFingerprint::Absent,
            replace: FileFingerprint::Absent,
            reftable: FileFingerprint::Absent,
        };
        let entry = |access: u64| CachedRepository {
            repository: Repository {
                name: "synthetic".to_owned(),
                ship_unix: Some(DAY_ONE),
            },
            fingerprint: fingerprint.clone(),
            root: PathBuf::from("/synthetic"),
            root_fingerprint: FileFingerprint::Absent,
            verified_at: t0,
            last_access: t0 + Duration::from_secs(access),
        };
        let key = |index: usize| PathBuf::from(format!("/common/{index}"));
        let mut cache = RepositoryCache::default();
        for index in 0..CACHE_CAPACITY {
            cache.insert(key(index), entry(index as u64 + 1));
        }
        // Entry 0 was accessed most recently, so entry 1 is the oldest.
        cache.entries.get_mut(&key(0)).unwrap().last_access = t0 + Duration::from_secs(1000);
        cache.insert(key(0), entry(2000));
        assert_eq!(cache.entries.len(), CACHE_CAPACITY);
        cache.insert(key(CACHE_CAPACITY), entry(3000));
        assert_eq!(cache.entries.len(), CACHE_CAPACITY);
        assert!(!cache.entries.contains_key(&key(1)));
        assert!(cache.entries.contains_key(&key(0)));
    }

    impl RepositoryRead {
        fn complete(self) -> Option<Repository> {
            match self {
                Self::Complete(repository) => repository,
                Self::Interrupted => panic!("unexpected interruption"),
            }
        }
    }
}
