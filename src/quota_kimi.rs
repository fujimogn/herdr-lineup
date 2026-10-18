//! Read-only Kimi managed usage client. No network request occurs before `poll`.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

const USAGES_URL: &str = "https://api.kimi.com/coding/v1/usages";
const BASE_URL: &str = "https://api.kimi.com/coding/v1";
const MAX_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    interval: Duration,
    timeout: Duration,
}

impl Settings {
    pub fn new(interval: Duration, timeout: Duration) -> Self {
        Self {
            interval: interval.clamp(Duration::from_secs(60), Duration::from_secs(3600)),
            timeout: timeout.clamp(Duration::from_secs(1), Duration::from_secs(30)),
        }
    }

    pub fn interval(self) -> Duration {
        self.interval
    }
    pub fn timeout(self) -> Duration {
        self.timeout
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new(Duration::from_secs(300), Duration::from_secs(10))
    }
}

/// Fixed categories; no URL, token, path, response, or library error is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    AuthMissing,
    AuthIo,
    AuthShape,
    AuthExpired,
    AuthAmbiguous,
    Unauthorized,
    Request,
    Transient,
    InvalidResponse,
}

impl Failure {
    pub fn is_transient(self) -> bool {
        self == Self::Transient
    }

    pub(crate) fn diagnostic(self) -> crate::doctor::WeeklyResult {
        use crate::doctor::WeeklyResult;
        match self {
            Self::AuthMissing => WeeklyResult::AuthMissing,
            Self::AuthIo => WeeklyResult::AuthIo,
            Self::AuthShape => WeeklyResult::AuthShape,
            Self::AuthExpired => WeeklyResult::AuthExpired,
            Self::AuthAmbiguous => WeeklyResult::AuthAmbiguous,
            Self::Unauthorized => WeeklyResult::Unauthorized,
            Self::Request => WeeklyResult::Request,
            Self::Transient => WeeklyResult::Transient,
            Self::InvalidResponse => WeeklyResult::InvalidResponse,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeeklyUsage {
    pub used_percent: f64,
    pub resets_at: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Weekly(WeeklyUsage),
    /// No request was due; the worker keeps its current sample unchanged.
    Throttled,
    Unavailable(Failure),
}

/// Resolve the daemon's Kimi home, without reading a file or changing env.
pub fn auth_path(kimi_home: Option<OsString>, home: Option<OsString>) -> Result<PathBuf, Failure> {
    let base = match kimi_home {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from(home.ok_or(Failure::AuthMissing)?).join(".kimi-code"),
    };
    if !base.is_absolute() {
        return Err(Failure::AuthShape);
    }
    Ok(base.join("credentials/kimi-code.json"))
}

fn validate_base_url(base: Option<OsString>) -> Result<(), Failure> {
    if base.as_ref().is_some_and(|value| value != BASE_URL) {
        Err(Failure::Request)
    } else {
        Ok(())
    }
}

/// The client retains only the credential path and a non-secret weekly sample.
pub struct Client {
    auth_file: PathBuf,
    settings: Settings,
    endpoint: String,
    agent: ureq::Agent,
    next_request: Option<Instant>,
    last_result: crate::doctor::WeeklyResult,
    last_auth_expiry: Option<SystemTime>,
}

impl Client {
    pub fn from_env(settings: Settings) -> Result<Self, Failure> {
        validate_base_url(std::env::var_os("KIMI_CODE_BASE_URL"))?;
        let path = auth_path(std::env::var_os("KIMI_CODE_HOME"), std::env::var_os("HOME"))?;
        Ok(Self::new(path, settings))
    }

    #[cfg(test)]
    pub(crate) fn from_test_endpoint(
        settings: Settings,
        endpoint: String,
    ) -> Result<Self, Failure> {
        let port = endpoint
            .strip_prefix("http://127.0.0.1:")
            .and_then(|value| value.strip_suffix("/usages"))
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .ok_or(Failure::Request)?;
        let path = auth_path(std::env::var_os("KIMI_CODE_HOME"), std::env::var_os("HOME"))?;
        Ok(Self::with_endpoint(
            path,
            settings,
            format!("http://127.0.0.1:{port}/usages"),
        ))
    }

    pub fn new(auth_file: PathBuf, settings: Settings) -> Self {
        Self::with_endpoint(auth_file, settings, USAGES_URL.to_owned())
    }

    fn with_endpoint(auth_file: PathBuf, settings: Settings, endpoint: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(settings.timeout))
            .max_redirects(0)
            .https_only(!cfg!(test))
            .build()
            .new_agent();
        Self {
            auth_file,
            settings,
            endpoint,
            agent,
            next_request: None,
            last_result: crate::doctor::WeeklyResult::Pending,
            last_auth_expiry: None,
        }
    }

    pub(crate) fn last_result(&self) -> crate::doctor::WeeklyResult {
        self.last_result
    }

    pub(crate) fn auth_expiry(&self) -> Option<SystemTime> {
        self.last_auth_expiry
    }

    pub(crate) fn source_stamp(&self) -> Option<SourceStamp> {
        source_stamp(&self.auth_file)
    }

    /// At most one request per interval; the caller owns any sample retention.
    pub fn poll(&mut self) -> Outcome {
        self.poll_at(Instant::now(), SystemTime::now())
    }

    fn poll_at(&mut self, now: Instant, wall_now: SystemTime) -> Outcome {
        if self.next_request.is_some_and(|next| now < next) {
            return Outcome::Throttled;
        }
        self.next_request = Some(now + self.settings.interval);
        let outcome = self.fetch(wall_now);
        self.last_result = match outcome {
            Outcome::Weekly(_) => crate::doctor::WeeklyResult::Ok,
            Outcome::Throttled => self.last_result,
            Outcome::Unavailable(failure) => failure.diagnostic(),
        };
        outcome
    }

    fn fetch(&mut self, wall_now: SystemTime) -> Outcome {
        self.last_auth_expiry = None;
        if let Err(failure) = validate_slot(&self.auth_file) {
            return Outcome::Unavailable(failure);
        }
        let token = match read_credentials(&self.auth_file, wall_now) {
            Ok(token) => token,
            Err(failure) => return Outcome::Unavailable(failure),
        };
        self.last_auth_expiry =
            UNIX_EPOCH.checked_add(Duration::from_secs(token.expires_at as u64));
        let mut response = match self
            .agent
            .get(&self.endpoint)
            .header("Authorization", format!("Bearer {}", token.access_token))
            .header("Accept", "application/json")
            .call()
        {
            Ok(response) => response,
            Err(error) => {
                return Outcome::Unavailable(match error {
                    ureq::Error::StatusCode(401 | 403) => Failure::Unauthorized,
                    ureq::Error::StatusCode(500..=599) => Failure::Transient,
                    ureq::Error::StatusCode(_) => Failure::Request,
                    _ => Failure::Transient,
                });
            }
        };
        if response.status() != 200 {
            return Outcome::Unavailable(Failure::Request);
        }
        let mut bytes = Vec::new();
        if response
            .body_mut()
            .as_reader()
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return Outcome::Unavailable(Failure::Transient);
        }
        if bytes.len() as u64 > MAX_BYTES {
            return Outcome::Unavailable(Failure::InvalidResponse);
        }
        match parse_usage(&bytes, wall_now) {
            Ok(usage) => Outcome::Weekly(usage),
            Err(failure) => Outcome::Unavailable(failure),
        }
    }
}

/// Non-secret file identity; a changed credential or OAuth selector invalidates a sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceStamp {
    credential: FileStamp,
    selector: Option<FileStamp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    device: u64,
    inode: u64,
    mode: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    Some(FileStamp {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        size: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
    })
}

pub(crate) fn source_stamp(auth_file: &Path) -> Option<SourceStamp> {
    let home = auth_file.parent()?.parent()?;
    if !std::fs::symlink_metadata(auth_file)
        .ok()?
        .file_type()
        .is_file()
    {
        return None;
    }
    let credential = file_stamp(auth_file)?;
    Some(SourceStamp {
        credential,
        selector: file_stamp(&home.join("config.toml")),
    })
}

#[derive(Deserialize)]
struct Credential {
    access_token: String,
    expires_at: i64,
}

fn validate_slot(path: &Path) -> Result<(), Failure> {
    let home = path
        .parent()
        .and_then(Path::parent)
        .ok_or(Failure::AuthShape)?;
    let config = home.join("config.toml");
    let metadata = match std::fs::symlink_metadata(&config) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Failure::AuthIo),
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_BYTES {
        return Err(Failure::AuthShape);
    }
    let mut bytes = Vec::new();
    open_regular(&config)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::AuthIo)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Failure::AuthShape);
    }
    let value: toml::Value =
        toml::from_str(std::str::from_utf8(&bytes).map_err(|_| Failure::AuthShape)?)
            .map_err(|_| Failure::AuthShape)?;
    let selected = value
        .get("providers")
        .and_then(|providers| providers.get("managed:kimi-code"))
        .and_then(|managed| managed.get("oauth"))
        .and_then(|oauth| oauth.get("key"));
    if selected.is_some_and(|key| key.as_str() != Some("oauth/kimi-code")) {
        return Err(Failure::AuthShape);
    }
    Ok(())
}

fn read_credentials(path: &Path, now: SystemTime) -> Result<Credential, Failure> {
    let parent = path.parent().ok_or(Failure::AuthShape)?;
    let directory = std::fs::symlink_metadata(parent).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Failure::AuthMissing
        } else {
            Failure::AuthIo
        }
    })?;
    if !directory.file_type().is_dir() {
        return Err(Failure::AuthShape);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Failure::AuthMissing
        } else {
            Failure::AuthIo
        }
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_BYTES {
        return Err(Failure::AuthShape);
    }
    let mut bytes = Vec::new();
    open_regular(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::AuthIo)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Failure::AuthShape);
    }
    let credential: Credential = serde_json::from_slice(&bytes).map_err(|_| Failure::AuthShape)?;
    let expiry = u64::try_from(credential.expires_at).map_err(|_| Failure::AuthShape)?;
    if credential.access_token.is_empty()
        || !credential
            .access_token
            .bytes()
            .all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err(Failure::AuthShape);
    }
    if UNIX_EPOCH
        .checked_add(Duration::from_secs(expiry))
        .is_none_or(|time| time <= now)
    {
        return Err(Failure::AuthExpired);
    }
    Ok(credential)
}

fn open_regular(path: &Path) -> Result<std::fs::File, Failure> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Failure::AuthIo)?;
    let metadata = file.metadata().map_err(|_| Failure::AuthIo)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(Failure::AuthShape);
    }
    Ok(file)
}

#[derive(Deserialize)]
struct Response {
    usages: Usages,
}
#[derive(Deserialize)]
struct Usages {
    limit_7d: Window,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Window {
    used_ratio: Ratio,
    #[serde(default, deserialize_with = "deserialize_reset")]
    reset_time: Option<String>,
}

fn deserialize_reset<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Ratio {
    Number(f64),
    String(String),
}

fn parse_usage(bytes: &[u8], now: SystemTime) -> Result<WeeklyUsage, Failure> {
    let response: Response = serde_json::from_slice(bytes).map_err(|_| Failure::InvalidResponse)?;
    let ratio = match response.usages.limit_7d.used_ratio {
        Ratio::Number(value) => value,
        Ratio::String(value) => value.parse::<f64>().map_err(|_| Failure::InvalidResponse)?,
    };
    if !ratio.is_finite() || !(0.0..=1.0).contains(&ratio) {
        return Err(Failure::InvalidResponse);
    }
    let resets_at = response
        .usages
        .limit_7d
        .reset_time
        .map(|value| {
            chrono::DateTime::parse_from_rfc3339(&value)
                .map(|date| SystemTime::from(date.with_timezone(&chrono::Utc)))
                .map_err(|_| Failure::InvalidResponse)
        })
        .transpose()?;
    if resets_at.is_some_and(|reset| reset <= now) {
        return Err(Failure::InvalidResponse);
    }
    Ok(WeeklyUsage {
        used_percent: ratio * 100.0,
        resets_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    const WEEK: &str = r#"{"usages":{"limit_7d":{"used_ratio":0.25,"reset_time":"2099-01-01T00:00:00Z"},"limit_5h":{"used_ratio":0.9}}}"#;

    #[test]
    fn paths_require_absolute_home_and_default_slot() {
        assert_eq!(
            auth_path(None, Some("/tmp/test-home".into())).unwrap(),
            PathBuf::from("/tmp/test-home/.kimi-code/credentials/kimi-code.json")
        );
        assert_eq!(
            auth_path(Some("relative".into()), None),
            Err(Failure::AuthShape)
        );
        assert_eq!(validate_base_url(None), Ok(()));
        assert_eq!(validate_base_url(Some(BASE_URL.into())), Ok(()));
        for base in [
            "https://api.kimi.ai/coding/v1",
            "https://api.kimi.com/coding/v1/",
            "https://evil.invalid/coding/v1",
            "",
        ] {
            assert_eq!(validate_base_url(Some(base.into())), Err(Failure::Request));
        }
    }

    #[test]
    fn ratio_and_future_reset_are_validated() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let sample = parse_usage(WEEK.as_bytes(), now).unwrap();
        assert_eq!(sample.used_percent, 25.0);
        assert_eq!(100.0 - sample.used_percent, 75.0);
        for body in [
            r#"{"usages":{"limit_5h":{"used_ratio":0.25}}}"#,
            r#"{"usages":{"limit_7d":{"used_ratio":1.1}}}"#,
            r#"{"usages":{"limit_7d":{"used_ratio":0.1,"reset_time":"2000-01-01T00:00:00Z"}}}"#,
            r#"{"usages":{"limit_7d":{"used_ratio":0.1,"reset_time":null}}}"#,
            r#"{"usages":{"limit_7d":{"used_ratio":0.1,"unexpected":1}}}"#,
        ] {
            assert_eq!(
                parse_usage(body.as_bytes(), now),
                Err(Failure::InvalidResponse)
            );
        }
        assert_eq!(
            parse_usage(br#"{"usages":{"limit_7d":{"used_ratio":"0.5"}}}"#, now)
                .unwrap()
                .used_percent,
            50.0
        );
    }

    #[test]
    fn credentials_are_bounded_and_expiry_checked_without_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kimi-code.json");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert!(matches!(
            read_credentials(&path, now),
            Err(Failure::AuthMissing)
        ));
        std::fs::write(
            &path,
            r#"{"access_token":"fake-token","expires_at":4102444800,"refresh_token":"ignored"}"#,
        )
        .unwrap();
        assert_eq!(
            read_credentials(&path, now).unwrap().access_token,
            "fake-token"
        );
        std::fs::write(
            &path,
            r#"{"access_token":"fake-token","expires_at":1600000000}"#,
        )
        .unwrap();
        assert!(matches!(
            read_credentials(&path, now),
            Err(Failure::AuthExpired)
        ));
        std::fs::write(&path, "x".repeat(65537)).unwrap();
        assert!(matches!(
            read_credentials(&path, now),
            Err(Failure::AuthShape)
        ));
        let link = dir.path().join("linked.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(matches!(
            read_credentials(&link, now),
            Err(Failure::AuthShape)
        ));
        std::fs::write(
            &path,
            r#"{"access_token":"fake-token","expires_at":4102444800}"#,
        )
        .unwrap();
        let linked_dir = dir.path().join("linked-dir");
        std::os::unix::fs::symlink(dir.path(), &linked_dir).unwrap();
        assert!(matches!(
            read_credentials(&linked_dir.join("kimi-code.json"), now),
            Err(Failure::AuthShape)
        ));
    }

    #[test]
    fn configured_other_credential_slot_is_unavailable_before_request() {
        let dir = tempfile::tempdir().unwrap();
        let credentials = dir.path().join("credentials");
        std::fs::create_dir(&credentials).unwrap();
        let path = credentials.join("kimi-code.json");
        std::fs::write(
            &path,
            r#"{"access_token":"fake-token","expires_at":4102444800}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "[providers.\"managed:kimi-code\".oauth]\nkey = \"oauth/other\"\n",
        )
        .unwrap();
        assert_eq!(validate_slot(&path), Err(Failure::AuthShape));
        std::fs::write(
            dir.path().join("config.toml"),
            "[providers.\"managed:kimi-code\".oauth]\nkey = \"oauth/kimi-code\"\n",
        )
        .unwrap();
        assert_eq!(validate_slot(&path), Ok(()));
    }

    #[test]
    fn constructing_client_does_not_contact_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/usages", listener.local_addr().unwrap());
        let mut client = Client::with_endpoint(
            PathBuf::from("/missing/credentials/kimi-code.json"),
            Settings::default(),
            url,
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
        assert_eq!(client.poll(), Outcome::Unavailable(Failure::AuthMissing));
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }

    #[test]
    fn expired_credential_is_distinct_and_never_sent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kimi-code.json");
        std::fs::write(
            &path,
            r#"{"access_token":"fake-token","expires_at":1600000000}"#,
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut client = Client::with_endpoint(
            path,
            Settings::default(),
            format!("http://{}/usages", listener.local_addr().unwrap()),
        );
        assert_eq!(client.poll(), Outcome::Unavailable(Failure::AuthExpired));
        assert_eq!(
            client.last_result(),
            crate::doctor::WeeklyResult::AuthExpired
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }

    fn server(status: u16, body: String) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/usages", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            String::from_utf8(request).unwrap()
        });
        (url, handle)
    }

    #[test]
    fn poll_sends_only_bounded_request_and_redacts_failures() {
        for (status, body, expected) in [
            (200, WEEK.to_owned(), None),
            (401, "fake-token".into(), Some(Failure::Unauthorized)),
            (500, "fake-token".into(), Some(Failure::Transient)),
            (302, "fake-token".into(), Some(Failure::Request)),
            (200, "x".repeat(65537), Some(Failure::InvalidResponse)),
        ] {
            let (url, handle) = server(status, body);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("kimi-code.json");
            std::fs::write(
                &path,
                r#"{"access_token":"fake-token","expires_at":4102444800}"#,
            )
            .unwrap();
            let mut client = Client::with_endpoint(path, Settings::default(), url);
            let now = Instant::now();
            let outcome = client.poll_at(now, SystemTime::now());
            let request = handle.join().unwrap();
            assert!(request.contains("GET /usages HTTP/1.1"));
            assert!(
                request.contains("Authorization: Bearer fake-token")
                    || request.contains("authorization: Bearer fake-token")
            );
            if let Some(failure) = expected {
                assert_eq!(outcome, Outcome::Unavailable(failure));
            } else {
                assert!(matches!(outcome, Outcome::Weekly(_)));
            }
            assert!(!format!("{outcome:?}").contains("fake-token"));
            assert_eq!(client.poll_at(now, SystemTime::now()), Outcome::Throttled);
        }
    }
}
