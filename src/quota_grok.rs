//! Opt-in Grok billing client. Not connected to the daemon or reader yet.
//!
//! Keep one client per configured account. Outcomes contain no credentials or
//! upstream error text. No network request occurs until `Client::poll` is called.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;

const BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const MAX_BYTES: u64 = 64 * 1024;

/// Bounded request cadence, independent of the daemon's refresh cadence.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    interval: Duration,
    timeout: Duration,
}

impl Settings {
    /// Clamp cadence to 60..=3600 seconds and request timeout to 1..=30 seconds.
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

/// Safe fixed failure categories. Never retain a URL, response body or auth error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Credentials,
    Request,
    Transient,
    InvalidResponse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeeklyUsage {
    pub used_percent: f64,
    pub resets_at: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Weekly(WeeklyUsage),
    /// A MONTHLY allowance must not be displayed as 7d. Weekly callers show missing.
    NonWeekly,
    Unavailable(Failure),
}

/// Resolve precedence without changing the process environment. No file is read.
pub fn auth_path(
    auth_file: Option<OsString>,
    grok_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, Failure> {
    if let Some(path) = auth_file {
        return Ok(PathBuf::from(path));
    }
    let base = match grok_home {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(home.ok_or(Failure::Credentials)?).join(".grok"),
    };
    Ok(base.join("auth.json"))
}

/// Memory-only cache. Deliberately has no Debug/Serialize implementation.
pub struct Client {
    auth_file: PathBuf,
    settings: Settings,
    endpoint: String,
    agent: ureq::Agent,
    next_request: Option<Instant>,
    cached: Outcome,
}

impl Client {
    /// Resolve GROK_AUTH_FILE, GROK_HOME and HOME. Does not read credentials yet.
    pub fn from_env(settings: Settings) -> Result<Self, Failure> {
        let path = auth_path(
            std::env::var_os("GROK_AUTH_FILE"),
            std::env::var_os("GROK_HOME"),
            std::env::var_os("HOME"),
        )?;
        Ok(Self::new(path, settings))
    }

    /// Use an explicit credential path and the fixed HTTPS billing endpoint.
    pub fn new(auth_file: PathBuf, settings: Settings) -> Self {
        Self::with_endpoint(
            auth_file,
            settings,
            BILLING_URL.to_owned(),
            settings.timeout,
        )
    }

    fn with_endpoint(
        auth_file: PathBuf,
        settings: Settings,
        endpoint: String,
        timeout: Duration,
    ) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .max_redirects(0)
            .https_only(!cfg!(test))
            .proxy(None)
            .build()
            .new_agent();
        Self {
            auth_file,
            settings,
            endpoint,
            agent,
            next_request: None,
            cached: Outcome::Unavailable(Failure::Credentials),
        }
    }

    /// At most one request per configured interval, including after failures.
    /// Transient network/5xx failures keep the last weekly result until reset;
    /// authentication and schema failures clear it. Without success, return missing.
    /// This synchronous call takes at most the configured HTTP timeout; callers
    /// must schedule it independently if their daemon refresh must not wait.
    pub fn poll(&mut self) -> Outcome {
        self.poll_at(Instant::now(), SystemTime::now())
    }

    fn poll_at(&mut self, now: Instant, wall_now: SystemTime) -> Outcome {
        if self.next_request.is_none_or(|next| now >= next) {
            self.next_request = Some(now + self.settings.interval);
            self.cached = match self.fetch() {
                Ok(outcome) => outcome,
                Err(Failure::Transient) if matches!(self.cached, Outcome::Weekly(usage) if usage.resets_at > wall_now) => {
                    self.cached
                }
                Err(failure) => Outcome::Unavailable(failure),
            };
        }
        if matches!(self.cached, Outcome::Weekly(usage) if usage.resets_at <= wall_now) {
            self.cached = Outcome::Unavailable(Failure::InvalidResponse);
        }
        self.cached
    }

    fn fetch(&self) -> Result<Outcome, Failure> {
        let credentials = read_credentials(&self.auth_file)?;
        let mut request = self
            .agent
            .get(&self.endpoint)
            .header("Authorization", format!("Bearer {}", credentials.key))
            .header("X-XAI-Token-Auth", "xai-grok-cli")
            .header("Accept", "application/json");
        if let Some(user_id) = credentials.user_id {
            request = request.header("x-userid", user_id);
        }
        // Never format, store, log or return the HTTP library's errors.
        let mut response = request.call().map_err(|error| match error {
            ureq::Error::StatusCode(401 | 403) => Failure::Credentials,
            ureq::Error::StatusCode(500..=599) => Failure::Transient,
            ureq::Error::StatusCode(_) => Failure::Request,
            _ => Failure::Transient,
        })?;
        if response.status() != 200 {
            return Err(Failure::Request);
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()
            .map_err(|_| Failure::Transient)?;
        parse_billing(&bytes)
    }
}

#[derive(Deserialize)]
struct Billing {
    config: BillingConfig,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingConfig {
    credit_usage_percent: Option<f64>,
    current_period: Period,
}
#[derive(Deserialize)]
struct Period {
    r#type: String,
    end: Option<String>,
}

fn parse_billing(bytes: &[u8]) -> Result<Outcome, Failure> {
    let billing: Billing = serde_json::from_slice(bytes).map_err(|_| Failure::InvalidResponse)?;
    match billing.config.current_period.r#type.as_str() {
        "MONTHLY" => Ok(Outcome::NonWeekly),
        "WEEKLY" => {
            // Grok's proto3 schema omits the scalar for a new unused period.
            let used_percent = billing.config.credit_usage_percent.unwrap_or(0.0);
            if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
                return Err(Failure::InvalidResponse);
            }
            let end = billing
                .config
                .current_period
                .end
                .ok_or(Failure::InvalidResponse)?;
            let resets_at = chrono::DateTime::parse_from_rfc3339(&end)
                .map_err(|_| Failure::InvalidResponse)?
                .with_timezone(&chrono::Utc)
                .into();
            Ok(Outcome::Weekly(WeeklyUsage {
                used_percent,
                resets_at,
            }))
        }
        _ => Err(Failure::InvalidResponse),
    }
}

// Only key and optional user_id survive deserialization. Unknown scalar fields
// are discarded by the visitor, including creation, expiry and session content.
// Multiple credential candidates are ambiguous: fail closed instead of guessing.
struct Credentials {
    key: String,
    user_id: Option<String>,
}
struct AuthNode(Vec<Credentials>);

impl<'de> Deserialize<'de> for AuthNode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = AuthNode;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("auth data")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<AuthNode, M::Error> {
                let mut key = None;
                let mut user_id = None;
                let mut candidates = Vec::new();
                while let Some(name) = map.next_key::<String>()? {
                    match name.as_str() {
                        "key" => {
                            if key.is_some() {
                                return Err(serde::de::Error::custom("duplicate key"));
                            }
                            key = map.next_value::<Option<String>>()?;
                        }
                        "user_id" => user_id = map.next_value::<Option<String>>()?,
                        _ => candidates.extend(map.next_value::<AuthNode>()?.0),
                    }
                }
                if let Some(key) = key.filter(|key| !key.trim().is_empty()) {
                    candidates.push(Credentials {
                        key,
                        user_id: user_id.filter(|id| !id.is_empty()),
                    });
                }
                Ok(AuthNode(candidates))
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> Result<AuthNode, S::Error> {
                let mut candidates = Vec::new();
                while let Some(node) = seq.next_element::<AuthNode>()? {
                    candidates.extend(node.0);
                }
                Ok(AuthNode(candidates))
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<AuthNode, E> {
                Ok(AuthNode(Vec::new()))
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<AuthNode, E> {
                Ok(AuthNode(Vec::new()))
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<AuthNode, E> {
                Ok(AuthNode(Vec::new()))
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<AuthNode, E> {
                Ok(AuthNode(Vec::new()))
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<AuthNode, E> {
                Ok(AuthNode(Vec::new()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<AuthNode, E> {
                Ok(AuthNode(Vec::new()))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn read_credentials(path: &Path) -> Result<Credentials, Failure> {
    let file = std::fs::File::open(path).map_err(|_| Failure::Credentials)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Credentials)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Failure::Credentials);
    }
    let mut node: AuthNode = serde_json::from_slice(&bytes).map_err(|_| Failure::Credentials)?;
    if node.0.len() != 1 {
        return Err(Failure::Credentials);
    }
    let credentials = node.0.pop().ok_or(Failure::Credentials)?;
    let valid_header = |value: &str| value.bytes().all(|byte| (0x20..=0x7e).contains(&byte));
    if !valid_header(&credentials.key)
        || credentials
            .user_id
            .as_deref()
            .is_some_and(|id| !valid_header(id))
    {
        return Err(Failure::Credentials);
    }
    Ok(credentials)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    const WEEK: &str = r#"{"config":{"creditUsagePercent":25,"currentPeriod":{"type":"WEEKLY","end":"2099-01-01T00:00:00Z"}}}"#;

    fn server(
        status: u16,
        body: &'static str,
        delay: Duration,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/v1/billing?format=credits",
            listener.local_addr().unwrap()
        );
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            thread::sleep(delay);
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            String::from_utf8(request).unwrap()
        });
        (url, handle)
    }

    fn client(url: String, timeout: Duration) -> (tempfile::TempDir, Client) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(
            &path,
            r#"{"key":"fake-test-secret","user_id":"fake-user","ignored":"not-retained"}"#,
        )
        .unwrap();
        (
            dir,
            Client::with_endpoint(path, Settings::default(), url, timeout),
        )
    }

    #[test]
    fn weekly_headers_and_cache() {
        let (url, handle) = server(200, WEEK, Duration::ZERO);
        let (_dir, mut client) = client(url, Duration::from_secs(1));
        let now = Instant::now();
        let result = client.poll_at(now, SystemTime::now());
        assert!(matches!(
            result,
            Outcome::Weekly(WeeklyUsage {
                used_percent: 25.0,
                ..
            })
        ));
        let request = handle.join().unwrap().to_lowercase();
        assert!(request.starts_with("get /v1/billing?format=credits http/1.1"));
        assert!(request.contains("authorization: bearer fake-test-secret"));
        assert!(request.contains("x-xai-token-auth: xai-grok-cli"));
        assert!(request.contains("x-userid: fake-user"));
        assert_eq!(
            client.poll_at(now + Duration::from_secs(59), SystemTime::now()),
            result
        );
        assert!(!format!("{result:?}").contains("fake-test-secret"));
    }

    #[test]
    fn monthly_is_never_weekly() {
        let (url, handle) = server(
            200,
            r#"{"config":{"creditUsagePercent":25,"currentPeriod":{"type":"MONTHLY","end":"2099-01-01T00:00:00Z"}}}"#,
            Duration::ZERO,
        );
        let (_dir, mut client) = client(url, Duration::from_secs(1));
        assert_eq!(client.poll(), Outcome::NonWeekly);
        handle.join().unwrap();
    }

    #[test]
    fn authentication_failure_and_timeout_are_missing_and_redacted() {
        for (status, delay) in [(401, Duration::ZERO), (200, Duration::from_millis(150))] {
            let (url, handle) = server(status, "fake-test-secret", delay);
            let (_dir, mut client) = client(url, Duration::from_millis(40));
            let result = client.poll();
            assert!(matches!(result, Outcome::Unavailable(_)));
            assert!(!format!("{result:?}").contains("fake-test-secret"));
            handle.join().unwrap();
        }
    }

    #[test]
    fn expired_cache_and_failed_refresh_clear_weekly() {
        let (url, handle) = server(200, WEEK, Duration::ZERO);
        let (dir, mut client) = client(url, Duration::from_secs(1));
        let now = Instant::now();
        assert!(matches!(
            client.poll_at(now, SystemTime::now()),
            Outcome::Weekly(_)
        ));
        handle.join().unwrap();
        assert_eq!(
            client.poll_at(
                now,
                SystemTime::UNIX_EPOCH + Duration::from_secs(4_102_444_800)
            ),
            Outcome::Unavailable(Failure::InvalidResponse)
        );
        std::fs::write(dir.path().join("auth.json"), "invalid fake-test-secret").unwrap();
        assert_eq!(
            client.poll_at(now + Duration::from_secs(300), SystemTime::now()),
            Outcome::Unavailable(Failure::Credentials)
        );
    }

    #[test]
    fn transient_failure_retains_success_until_reset_but_auth_failure_clears_it() {
        for (status, delay, retain) in [
            (500, Duration::ZERO, true),
            (200, Duration::from_millis(150), true),
            (401, Duration::ZERO, false),
            (403, Duration::ZERO, false),
        ] {
            let (url, handle) = server(200, WEEK, Duration::ZERO);
            let (_dir, mut client) = client(url, Duration::from_millis(40));
            let now = Instant::now();
            let original = client.poll_at(now, SystemTime::now());
            assert!(matches!(original, Outcome::Weekly(_)));
            handle.join().unwrap();
            let (url, handle) = server(status, "fake-test-secret", delay);
            client.endpoint = url;
            let result = client.poll_at(now + Duration::from_secs(300), SystemTime::now());
            if retain {
                assert_eq!(result, original);
                assert_eq!(
                    client.poll_at(
                        now + Duration::from_secs(301),
                        SystemTime::UNIX_EPOCH + Duration::from_secs(4_102_444_800)
                    ),
                    Outcome::Unavailable(Failure::InvalidResponse)
                );
            } else {
                assert!(matches!(result, Outcome::Unavailable(_)));
            }
            handle.join().unwrap();
        }
    }

    #[test]
    fn wire_logging_is_disabled_at_compile_time() {
        assert_eq!(log::STATIC_MAX_LEVEL, log::LevelFilter::Off);
    }

    #[test]
    fn credentials_allow_only_a_unique_key_and_optional_user_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        std::fs::write(&path, r#"{"account":{"key":"fake-key","user_id":"fake-user","expiry":"ignored","other":[true,2,null,"ignored"]}}"#).unwrap();
        let credentials = read_credentials(&path).unwrap();
        assert_eq!(credentials.key, "fake-key");
        assert_eq!(credentials.user_id.as_deref(), Some("fake-user"));
        for json in [
            r#"{"a":{"key":"one"},"b":{"key":"two"}}"#,
            r#"{"key":"bad\nheader"}"#,
            "{}",
            "not-json fake-key",
        ] {
            std::fs::write(&path, json).unwrap();
            assert!(matches!(read_credentials(&path), Err(Failure::Credentials)));
        }
    }

    #[test]
    fn invalid_billing_never_becomes_a_weekly_observation() {
        for body in [
            r#"{"config":{"creditUsagePercent":101,"currentPeriod":{"type":"WEEKLY","end":"2099-01-01T00:00:00Z"}}}"#,
            r#"{"config":{"currentPeriod":{"type":"WEEKLY"}}}"#,
            r#"{"config":{"currentPeriod":{"type":"ALMOST_WEEKLY","end":"2099-01-01T00:00:00Z"}}}"#,
            "fake-secret",
        ] {
            assert_eq!(
                parse_billing(body.as_bytes()),
                Err(Failure::InvalidResponse)
            );
        }
        assert!(matches!(
            parse_billing(
                br#"{"config":{"currentPeriod":{"type":"WEEKLY","end":"2099-01-01T00:00:00Z"}}}"#
            ),
            Ok(Outcome::Weekly(WeeklyUsage {
                used_percent: 0.0,
                ..
            }))
        ));
    }

    #[test]
    fn settings_are_bounded_and_paths_respect_precedence() {
        assert_eq!(
            Settings::new(Duration::ZERO, Duration::MAX).interval(),
            Duration::from_secs(60)
        );
        assert_eq!(
            Settings::new(Duration::MAX, Duration::MAX).interval(),
            Duration::from_secs(3600)
        );
        assert_eq!(Settings::default().interval(), Duration::from_secs(300));
        assert_eq!(
            auth_path(
                Some("/override".into()),
                Some("/grok".into()),
                Some("/home".into())
            )
            .unwrap(),
            PathBuf::from("/override")
        );
        assert_eq!(
            auth_path(None, Some("/grok".into()), None).unwrap(),
            PathBuf::from("/grok/auth.json")
        );
        assert_eq!(
            auth_path(None, None, Some("/home".into())).unwrap(),
            PathBuf::from("/home/.grok/auth.json")
        );
    }
}
