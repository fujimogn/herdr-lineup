//! Allowlisted statusLine observations and transparent command chaining.
use crate::model;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, DirBuilder, File},
    io::{self, Read, Write},
    os::unix::{fs::DirBuilderExt, process::ExitStatusExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, clap::ValueEnum, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Agy,
}
const MAX_INPUT: usize = 1024 * 1024;
pub(crate) const MAX_RECORD: u64 = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub version: u8,
    pub agent: Agent,
    pub key: String,
    pub observed_ms: u64,
    pub context: Option<f64>,
    pub weekly_remaining: Option<f64>,
    pub weekly_reset_ms: Option<u64>,
}

pub(crate) fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
}
pub(crate) fn record_path(state: &Path, agent: Agent, key: &str) -> PathBuf {
    // Encode opaque identifiers; neither session nor pane IDs become path components.
    let encoded: String = key.bytes().map(|b| format!("{b:02x}")).collect();
    state.join("statusline").join(format!(
        "{}-{encoded}.json",
        match agent {
            Agent::Claude => "claude",
            Agent::Agy => "agy",
        }
    ))
}
fn percent(value: Option<&Value>) -> Option<f64> {
    let value = value?.as_f64()?;
    (value.is_finite() && (0.0..=100.0).contains(&value)).then_some(value)
}
fn field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| value.get(*key))
}
fn context(value: &Value) -> Option<f64> {
    let window = field(value, &["context_window", "contextWindow"])?;
    if let Some(used) = field(window, &["used_percentage", "usedPercentage"]) {
        percent(Some(used))
    } else {
        percent(field(
            window,
            &["remaining_percentage", "remainingPercentage"],
        ))
        .map(|v| 100.0 - v)
    }
}
fn millis(now: SystemTime) -> Option<u64> {
    u64::try_from(now.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()
}
fn absolute_reset(value: &Value) -> Option<SystemTime> {
    value
        .as_u64()
        .and_then(|v| UNIX_EPOCH.checked_add(Duration::from_secs(v)))
        .or_else(|| model::timestamp(value))
}
fn agy_bucket(value: &Value) -> Option<&Value> {
    let quota = value.get("quota")?;
    let model = value.get("model");
    let name = model
        .and_then(|m| {
            m.as_str().or_else(|| {
                field(m, &["display_name", "displayName", "id"]).and_then(Value::as_str)
            })
        })
        .unwrap_or("")
        .to_ascii_lowercase();
    let gemini = name.contains("gemini") || name.contains("flash") || name.contains("learnlm");
    let third = ["claude", "sonnet", "haiku", "opus", "gpt"]
        .iter()
        .any(|part| name.contains(part))
        || ["o1", "o3", "o4"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
    let has_gemini = ["gemini-5h", "gemini-weekly"]
        .iter()
        .any(|k| quota.get(*k).is_some_and(|v| !v.is_null()));
    let has_third = ["3p-5h", "3p-weekly"]
        .iter()
        .any(|k| quota.get(*k).is_some_and(|v| !v.is_null()));
    let key = if gemini {
        "gemini-weekly"
    } else if third {
        "3p-weekly"
    } else {
        match (has_gemini, has_third) {
            (true, false) => "gemini-weekly",
            (false, true) => "3p-weekly",
            _ => return None,
        }
    };
    quota.get(key)
}
fn weekly(agent: Agent, value: &Value, now: SystemTime) -> (Option<f64>, Option<u64>) {
    match agent {
        Agent::Claude => {
            let Some(bucket) = value.get("rate_limits").and_then(|v| v.get("seven_day")) else {
                return (None, None);
            };
            let remaining = percent(bucket.get("used_percentage")).map(|v| 100.0 - v);
            let reset = bucket
                .get("resets_at")
                .and_then(absolute_reset)
                .and_then(millis);
            (remaining, reset)
        }
        Agent::Agy => {
            let Some(bucket) = agy_bucket(value) else {
                return (None, None);
            };
            let remaining = if let Some(fraction) =
                field(bucket, &["remaining_fraction", "remainingFraction"])
            {
                fraction
                    .as_f64()
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    .map(|v| v * 100.0)
            } else {
                percent(field(bucket, &["remaining_percent", "remainingPercentage"]))
            };
            let reset = field(bucket, &["reset_time", "resetTime"])
                .and_then(model::timestamp)
                .or_else(|| {
                    field(bucket, &["reset_in_seconds", "resetInSeconds"])
                        .and_then(Value::as_u64)
                        .and_then(|v| now.checked_add(Duration::from_secs(v)))
                })
                .and_then(millis);
            (remaining, reset)
        }
    }
}
fn capture(agent: Agent, input: &[u8], state: &Path, pane: Option<&str>, now: SystemTime) -> bool {
    if input.len() > MAX_INPUT || !state.is_absolute() {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(input) else {
        return false;
    };
    if !value.is_object() {
        return false;
    }
    let key = match agent {
        Agent::Claude => value.get("session_id").and_then(Value::as_str),
        Agent::Agy => pane,
    };
    let Some(key) = key.filter(|key| valid_key(key)) else {
        return false;
    };
    let Some(observed_ms) = millis(now) else {
        return false;
    };
    let (weekly_remaining, weekly_reset_ms) = weekly(agent, &value, now);
    let record = Record {
        version: 1,
        agent,
        key: key.into(),
        observed_ms,
        context: context(&value),
        weekly_remaining,
        weekly_reset_ms,
    };
    persist(state, &record).is_ok()
}
fn persist(state: &Path, record: &Record) -> io::Result<()> {
    let directory = state.join("statusline");
    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700).create(&directory)?;
    for path in [state, &directory] {
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(io::Error::other("invalid observation directory"));
        }
    }
    let mut temp = tempfile::NamedTempFile::new_in(&directory)?;
    serde_json::to_writer(temp.as_file_mut(), record)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(record_path(state, record.agent, &record.key))
        .map_err(|_| io::Error::other("cannot persist observation"))?;
    File::open(directory)?.sync_all()
}
/// A record failure is silent and successful. With --then, stdout is copied byte
/// for byte and the child status wins (signals become 128 + signal, capped at 255).
pub fn run(agent: Agent, then: Option<&str>) -> u8 {
    let state = std::env::var_os("HERDR_PLUGIN_STATE_DIR").map(PathBuf::from);
    let pane = std::env::var("HERDR_PANE_ID").ok();
    run_hook(
        agent,
        io::stdin(),
        &mut io::stdout().lock(),
        state.as_deref(),
        pane.as_deref(),
        then,
    )
}
fn run_hook<R: Read + Send + 'static, W: Write>(
    agent: Agent,
    mut input: R,
    output: &mut W,
    state: Option<&Path>,
    pane: Option<&str>,
    then: Option<&str>,
) -> u8 {
    let mut prefix = Vec::new();
    let read_ok = input
        .by_ref()
        .take(MAX_INPUT as u64 + 1)
        .read_to_end(&mut prefix)
        .is_ok();
    if read_ok && let Some(state) = state {
        let _ = capture(agent, &prefix, state, pane, SystemTime::now());
    }
    let Some(then) = then else { return 0 };
    let Ok(mut child) = Command::new("sh")
        .args(["-c", then])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    else {
        return 1;
    };
    let mut stdin = child.stdin.take().expect("piped stdin");
    // Feed concurrently: a hook may emit more than a pipe buffer before reading stdin.
    let writer = std::thread::spawn(move || {
        let _ = stdin
            .write_all(&prefix)
            .and_then(|()| io::copy(&mut input, &mut stdin).map(|_| ()));
    });
    let copied = io::copy(&mut child.stdout.take().expect("piped stdout"), output).is_ok();
    let status = child.wait();
    // A child can exit without consuming input; do not wait on a stalled parent stdin.
    drop(writer);
    match status {
        Ok(status) if copied => status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
            .clamp(0, 255) as u8,
        _ => 1,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::{
        io::Cursor,
        time::{Duration, UNIX_EPOCH},
    };
    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1791763200)
    }
    fn input() -> Vec<u8> {
        serde_json::to_vec(&json!({
        "session_id":"session-1", "context_window":{"used_percentage":23.5},
        "rate_limits":{"seven_day":{"used_percentage":27,"resets_at":1791800000}},
        "transcript_path":"DO-NOT-STORE", "credentials":"DO-NOT-STORE", "messages":["DO-NOT-STORE"]
    })).unwrap()
    }
    fn record(dir: &Path) -> Value {
        let files: Vec<_> = std::fs::read_dir(dir.join("statusline"))
            .unwrap()
            .map(|f| f.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap()
    }
    #[test]
    fn claude_allowlisted_atomic_record() {
        let dir = tempfile::tempdir().unwrap();
        assert!(capture(Agent::Claude, &input(), dir.path(), None, now()));
        let r = record(dir.path());
        assert_eq!(r["key"], "session-1");
        assert_eq!(r["context"], 23.5);
        assert_eq!(r["weekly_remaining"], 73.0);
        assert_eq!(r["weekly_reset_ms"], 1791800000000u64);
        assert!(!r.to_string().contains("DO-NOT-STORE"));
        assert!(capture(
            Agent::Claude,
            b"{\"session_id\":\"session-1\"}",
            dir.path(),
            None,
            now()
        ));
        assert!(record(dir.path())["weekly_remaining"].is_null());
    }
    #[test]
    fn agy_scale_and_pane_binding() {
        for (key, value, want) in [
            ("remaining_percent", 1.0, 1.0),
            ("remaining_fraction", 1.0, 100.0),
            ("remainingPercentage", 25.0, 25.0),
            ("remainingFraction", 0.25, 25.0),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let v = json!({"session_id":"wrong-native", "contextWindow":{"remainingPercentage":60}, "quota":{"gemini-weekly":{key:value,"reset_in_seconds":3600}}});
            assert!(capture(
                Agent::Agy,
                v.to_string().as_bytes(),
                dir.path(),
                Some("w58:p3"),
                now()
            ));
            let r = record(dir.path());
            assert_eq!(r["key"], "w58:p3");
            assert_eq!(r["context"], 40.0);
            assert_eq!(r["weekly_remaining"], want);
            assert_eq!(r["weekly_reset_ms"], 1791766800000u64);
        }
    }
    #[test]
    fn agy_active_model_and_ambiguous_pool() {
        for (model, want) in [
            ("Gemini Flash", Some(80.0)),
            ("Claude Sonnet", Some(5.0)),
            ("unknown", None),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let v = json!({"model":{"display_name":model},"quota":{"gemini-weekly":{"remaining_fraction":0.8},"3p-weekly":{"remaining_percent":5}}});
            assert!(capture(
                Agent::Agy,
                v.to_string().as_bytes(),
                dir.path(),
                Some("pane-1"),
                now()
            ));
            assert_eq!(record(dir.path())["weekly_remaining"].as_f64(), want);
        }
    }
    #[test]
    fn invalid_values_are_missing_independently() {
        let dir = tempfile::tempdir().unwrap();
        let v = json!({"session_id":"session-1","context_window":{"used_percentage":120},"rate_limits":{"seven_day":{"used_percentage":20}}});
        assert!(capture(
            Agent::Claude,
            v.to_string().as_bytes(),
            dir.path(),
            None,
            now()
        ));
        let r = record(dir.path());
        assert!(r["context"].is_null());
        assert_eq!(r["weekly_remaining"], 80.0);
        let v = json!({"quota":{"gemini-weekly":{"remaining_fraction":1.2}}});
        assert!(capture(
            Agent::Agy,
            v.to_string().as_bytes(),
            dir.path(),
            Some("pane-1"),
            now()
        ));
        let rpath = dir.path().join("statusline");
        assert_eq!(std::fs::read_dir(rpath).unwrap().count(), 2);
    }
    #[test]
    fn unbound_invalid_and_oversized_input_do_not_record() {
        let dir = tempfile::tempdir().unwrap();
        for bytes in [
            b"not-json".to_vec(),
            b"{}".to_vec(),
            vec![b' '; MAX_INPUT + 1],
        ] {
            assert!(!capture(Agent::Claude, &bytes, dir.path(), None, now()));
        }
        assert!(!capture(Agent::Agy, &input(), dir.path(), None, now()));
        assert!(!dir.path().join("statusline").exists());
    }
    #[test]
    fn then_preserves_bytes_and_exit_despite_broken_recording() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("file");
        std::fs::write(&bad, "blocked").unwrap();
        for input in [
            b"not-json\n\0raw".to_vec(),
            self::input(),
            vec![b'x'; MAX_INPUT + 17],
        ] {
            let mut out = Vec::new();
            let code = run_hook(
                Agent::Claude,
                Cursor::new(input.clone()),
                &mut out,
                Some(&bad),
                None,
                Some("cat; exit 7"),
            );
            assert_eq!(out, input);
            assert_eq!(code, 7);
        }
    }
    #[test]
    fn no_then_is_silent_and_success_on_record_failure() {
        let mut out = Vec::new();
        assert_eq!(
            run_hook(
                Agent::Claude,
                Cursor::new(b"not-json"),
                &mut out,
                None,
                None,
                None
            ),
            0
        );
        assert!(out.is_empty());
    }
    #[test]
    fn then_output_before_reading_large_stdin_does_not_deadlock() {
        let mut out = Vec::new();
        let bytes = vec![b'x'; MAX_INPUT + 50];
        assert_eq!(
            run_hook(
                Agent::Claude,
                Cursor::new(bytes.clone()),
                &mut out,
                None,
                None,
                Some("head -c 131072 /dev/zero; cat")
            ),
            0
        );
        assert_eq!(&out[..131072], vec![0; 131072]);
        assert_eq!(&out[131072..], bytes);
    }
}
