//! Fixed, bounded persistence for weekly quota diagnostics.

use std::io;

/// Persisted diagnostics contain only closed categories, never upstream data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WeeklyResult {
    Ok,
    Pending,
    AuthMissing,
    AuthIo,
    AuthShape,
    AuthExpired,
    AuthAmbiguous,
    Unauthorized,
    Transient,
    InvalidResponse,
    Request,
    NonWeekly,
    Disabled,
}
impl WeeklyResult {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Pending => "pending",
            Self::AuthMissing => "auth_missing",
            Self::AuthIo => "auth_io",
            Self::AuthShape => "auth_shape",
            Self::AuthExpired => "auth_expired",
            Self::AuthAmbiguous => "auth_ambiguous",
            Self::Unauthorized => "unauthorized",
            Self::Transient => "transient",
            Self::InvalidResponse => "invalid_response",
            Self::Request => "request",
            Self::NonWeekly => "non_weekly",
            Self::Disabled => "disabled",
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) enum WeeklyProvider {
    Codex,
    Grok,
    Kimi,
}
impl WeeklyProvider {
    fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Grok => "grok",
            Self::Kimi => "kimi",
        }
    }
    fn path(self, root: &std::path::Path) -> std::path::PathBuf {
        root.join(format!("weekly-{}.json", self.name()))
    }
}

pub(crate) fn write_weekly_result(
    root: &std::path::Path,
    provider: WeeklyProvider,
    result: WeeklyResult,
) -> io::Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(file.as_file_mut(), &result)?;
    file.flush()?;
    file.persist(provider.path(root))
        .map_err(|_| io::Error::other("cannot save weekly diagnostic"))?;
    Ok(())
}

/// Bound and validate local diagnostics before displaying any bytes. Symlinks,
/// unknown strings and malformed records are never echoed to the terminal.
fn read_weekly_result(root: &std::path::Path, provider: WeeklyProvider) -> &'static str {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(provider.path(root))
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return "not_observed",
        Err(_) => return "invalid_diagnostic",
    };
    if !file
        .metadata()
        .is_ok_and(|meta| meta.is_file() && meta.len() <= 128)
    {
        return "invalid_diagnostic";
    }
    let mut bytes = Vec::new();
    if file.take(129).read_to_end(&mut bytes).is_err() || bytes.len() > 128 {
        return "invalid_diagnostic";
    }
    serde_json::from_slice::<WeeklyResult>(&bytes)
        .map(WeeklyResult::label)
        .unwrap_or("invalid_diagnostic")
}

pub(crate) fn weekly_diagnostics(root: &std::path::Path) -> String {
    format!(
        "Last saved weekly query results: codex={}; grok={}; kimi={}",
        read_weekly_result(root, WeeklyProvider::Codex),
        read_weekly_result(root, WeeklyProvider::Grok),
        read_weekly_result(root, WeeklyProvider::Kimi)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_diagnostics_round_trip_only_fixed_categories() {
        let dir = tempfile::tempdir().unwrap();
        for result in [
            WeeklyResult::Ok,
            WeeklyResult::AuthMissing,
            WeeklyResult::AuthShape,
            WeeklyResult::AuthExpired,
            WeeklyResult::AuthAmbiguous,
            WeeklyResult::Unauthorized,
            WeeklyResult::Transient,
            WeeklyResult::NonWeekly,
            WeeklyResult::Disabled,
        ] {
            write_weekly_result(dir.path(), WeeklyProvider::Grok, result).unwrap();
            let text = weekly_diagnostics(dir.path());
            assert!(text.contains(&format!("grok={}", result.label())));
            assert!(text.contains("codex=not_observed"));
            write_weekly_result(dir.path(), WeeklyProvider::Kimi, result).unwrap();
            assert!(weekly_diagnostics(dir.path()).contains(&format!("kimi={}", result.label())));
        }
        std::fs::write(
            dir.path().join("weekly-grok.json"),
            r#""fake-private-body""#,
        )
        .unwrap();
        assert!(!weekly_diagnostics(dir.path()).contains("fake-private-body"));
        assert!(weekly_diagnostics(dir.path()).contains("grok=invalid_diagnostic"));
        std::fs::remove_file(dir.path().join("weekly-grok.json")).unwrap();
        std::os::unix::fs::symlink("/fake/private", dir.path().join("weekly-grok.json")).unwrap();
        assert!(weekly_diagnostics(dir.path()).contains("grok=invalid_diagnostic"));
    }
}
