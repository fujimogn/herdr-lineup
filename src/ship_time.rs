//! Local repository identity and origin/main commit-time headings.

use std::path::Path;
use std::process::Command;

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
pub fn read_repository(cwd: &Path) -> Option<Repository> {
    if git_output(cwd, &["rev-parse", "--is-inside-work-tree"])? != "true\n" {
        return None;
    }
    let common = git_output(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = Path::new(common.strip_suffix('\n')?).canonicalize().ok()?;
    // The first worktree is the main working repository, even when cwd is a
    // linked worktree. Query its common directory, not the linked root name.
    let worktrees = git_output(&common, &["worktree", "list", "--porcelain", "-z"])?;
    let root = worktrees.split('\0').next()?.strip_prefix("worktree ")?;
    let root = Path::new(root).canonicalize().ok()?;
    let name = root.file_name()?.to_str()?.to_owned();
    let ship_unix = git_output(
        cwd,
        &[
            "log",
            "-1",
            "--format=%ct",
            "refs/remotes/origin/main",
            "--",
        ],
    )
    .and_then(|value| value.trim().parse().ok());
    Some(Repository { name, ship_unix })
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
    command.arg("-C").arg(cwd);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    // Local log reads must not lazily fetch absent partial-clone objects.
    // Deny remote protocols as well, including on older Git versions.
    command
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "");
    command
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = git_command(cwd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use std::path::PathBuf;

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
            let cleanup = tempfile::Builder::new()
                .prefix("herdr-lineup-ship-time-")
                .tempdir_in(parent)
                .unwrap();
            let root = cleanup.path().to_owned();
            let repo = root.join("canonical-repo");
            std::fs::create_dir(&repo).unwrap();
            let fixture = Self {
                root,
                repo,
                _cleanup: cleanup,
            };
            fixture.git(&["init", "--initial-branch=main"], None);
            fixture.commit("2024-01-01T00:00:00Z");
            fixture
        }

        fn git(&self, args: &[&str], date: Option<&str>) -> String {
            let mut command = Command::new("git");
            command.arg("-C").arg(&self.repo).args(args);
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
    fn git_command_discards_inherited_git_environment() {
        let command = git_command(Path::new("/repository"));
        let removed: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_owned())
            .collect();
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_")
                && key != "GIT_NO_LAZY_FETCH"
                && key != "GIT_ALLOW_PROTOCOL"
            {
                assert!(removed.contains(&key));
            }
        }
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["-C", "/repository"]
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
}
