//! Read-only endpoint and daemon compatibility diagnostics.

use crate::{
    config::Config,
    daemon::{Options, probe},
    lock::{RuntimePaths, SessionLock},
    socket::Transport,
};
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
}
impl WeeklyProvider {
    fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Grok => "grok",
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
        "Last saved weekly query results: codex={}; grok={}",
        read_weekly_result(root, WeeklyProvider::Codex),
        read_weekly_result(root, WeeklyProvider::Grok)
    )
}

pub const MIN_HERDR_VERSION: &str = "0.9.3";
pub fn check_transport(transport: &mut Transport) -> io::Result<()> {
    transport.send_line(br#"{"id":"lineup-health","method":"ping","params":{}}"#)?;
    validate_pong(&transport.receive_line()?)
}
pub fn validate_pong(frame: &[u8]) -> io::Result<()> {
    let response: serde_json::Value = serde_json::from_slice(frame)
        .map_err(|_| io::Error::other("invalid Herdr health response"))?;
    let result = &response["result"];
    if response["id"] != "lineup-health"
        || result["type"] != "pong"
        || result["protocol"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= u64::from(u32::MAX))
            .is_none()
    {
        return Err(io::Error::other(
            "Herdr JSON protocol health response is incompatible",
        ));
    }
    let version = result["version"]
        .as_str()
        .ok_or_else(|| io::Error::other("Herdr version is missing"))?;
    let version = version.split('+').next().unwrap_or(version);
    if version.contains('-') {
        return Err(io::Error::other(
            "Herdr prerelease compatibility is unverified",
        ));
    }
    let numbers: Option<Vec<u64>> = version.split('.').map(|part| part.parse().ok()).collect();
    let numbers = numbers
        .filter(|v| v.len() == 3)
        .ok_or_else(|| io::Error::other("invalid Herdr version"))?;
    if numbers.as_slice() < [0, 9, 3].as_slice() {
        return Err(io::Error::other("Herdr 0.9.3 or later is required"));
    }
    // JSON API methods are build independent. Binary protocol number equality
    // would incorrectly reject a compatible newer server (official stability contract).
    Ok(())
}
pub fn run(options: &Options) -> io::Result<()> {
    let config = Config::load(&options.config)?;
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    println!("{}", weekly_diagnostics(&paths.root));
    let mut transport = Transport::connect(&options.socket)?;
    check_transport(&mut transport)?;
    let health = probe(&paths, &options.socket)?;
    if let Some(health) = health {
        if health.refresh_failed || health.last_refresh_ms.is_none() {
            return Err(io::Error::other("daemon refresh is unhealthy"));
        }
        println!(
            "Herdr JSON API compatible; config valid; daemon/control/lock healthy; last refresh {} ms after startup",
            health.last_refresh_ms.unwrap_or(0)
        );
    } else {
        if SessionLock::is_held(&paths)? {
            return Err(io::Error::other(
                "daemon lock is live but control is unavailable",
            ));
        }
        if paths.pid.exists() {
            return Err(io::Error::other(
                "stale diagnostic PID record; no process was signalled",
            ));
        }
        return Err(io::Error::other(
            "daemon is stopped; invoke ensure explicitly",
        ));
    }
    println!(
        "Font mode: {}; auto uses conservative text fallback.",
        config.logo_mode
    );
    println!("Optional provider data may be unavailable; no reader or network call was made.");
    Ok(())
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

    #[test]
    fn validates_minimum_version_and_required_health_response() {
        assert!(validate_pong(br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.3","protocol":22}}"#).is_ok());
        for reply in [
            br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.2","protocol":22}}"#
                .as_slice(),
            br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.3","protocol":0}}"#
                .as_slice(),
            br#"{"id":"wrong","result":{"type":"pong","version":"0.9.3","protocol":22}}"#
                .as_slice(),
        ] {
            assert!(validate_pong(reply).is_err());
        }
    }
}
