//! Pure sidebar presentation and metadata patch planning. No socket or disk I/O.

use std::collections::{BTreeMap, BTreeSet};

pub type Tokens = BTreeMap<String, Option<String>>;
pub const SOURCE: &str = "herdr-lineup";
pub const DEFAULT_REFRESH_MS: u64 = 5_000;
pub const PANE_KEYS: [&str; 12] = [
    "lineup_agent",
    "lineup_logo",
    "lineup_state",
    "lineup_state_idle",
    "lineup_state_working",
    "lineup_state_blocked",
    "lineup_state_done",
    "lineup_state_unknown",
    "lineup_context_pct",
    "lineup_context_text",
    "lineup_weekly_pct",
    "lineup_weekly_text",
];
pub const WORKSPACE_KEYS: [&str; 3] = ["lineup_repo", "lineup_ship_unix", "lineup_ship_text"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Agent {
    Claude,
    Codex,
    Grok,
    Agy,
    Kimi,
    Hermes,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogoMode {
    Auto,
    Font,
    Text,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Occupant {
    pub agent: Agent,
    /// Missing native association does not hide the authoritative agent/state.
    pub native_session: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Usage {
    pub occupant: Occupant,
    pub context_pct: Option<f64>,
    pub weekly_remaining_pct: Option<f64>,
    pub weekly_reset_unix: Option<i64>,
}
#[derive(Clone, Debug, Default)]
pub struct PaneInput {
    pub occupant: Option<Occupant>,
    pub state: Option<String>,
    pub usage: Option<Usage>,
}

/// Unsupported or unidentified occupants are represented by `None`.
/// Usage is accepted only when bound to this exact agent and a known native
/// session. `font_verified` means affirmative terminal rendering verification,
/// never mere font presence. Provider readers must remove stale/invalid samples
/// before supplying usage; this module does not read or assess reader freshness.
pub fn pane_tokens(row: &PaneInput, mode: LogoMode, font_verified: bool, now_unix: i64) -> Tokens {
    let mut tokens: Tokens = PANE_KEYS.iter().map(|key| ((*key).into(), None)).collect();
    let Some(occupant) = row.occupant.as_ref() else {
        return tokens;
    };
    let (id, label, glyph) = match occupant.agent {
        Agent::Claude => ("claude", "Claude", "\u{e1a0}"),
        Agent::Codex => ("codex", "Codex", "\u{e1a1}"),
        Agent::Grok => ("grok", "Grok", "\u{e1b1}"),
        Agent::Agy => ("agy", "Antigravity", "\u{e1b2}"),
        Agent::Kimi => ("kimi", "Kimi", "\u{e1a6}"),
        Agent::Hermes => ("hermes", "Hermes", "\u{e1aa}"),
    };
    let logo = if mode == LogoMode::Font || (mode == LogoMode::Auto && font_verified) {
        glyph
    } else {
        label
    };
    let state = row
        .state
        .as_deref()
        .filter(|state| ["idle", "working", "blocked", "done", "unknown"].contains(state))
        .unwrap_or("unknown");
    tokens.insert("lineup_agent".into(), Some(id.into()));
    tokens.insert("lineup_logo".into(), Some(logo.into()));
    tokens.insert("lineup_state".into(), Some(state.into()));
    tokens.insert(format!("lineup_state_{state}"), Some("●".into()));
    let usage = row.usage.as_ref().filter(|usage| {
        usage.occupant == *occupant
            && occupant
                .native_session
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
    });
    let context = percentage(usage.and_then(|usage| usage.context_pct));
    let weekly = percentage(usage.and_then(|usage| usage.weekly_remaining_pct));
    tokens.insert("lineup_context_pct".into(), context.map(decimal));
    tokens.insert(
        "lineup_context_text".into(),
        Some(
            context
                .map(|value| format!("{:02}%", value.round() as u8))
                .unwrap_or_else(|| "—".into()),
        ),
    );
    tokens.insert("lineup_weekly_pct".into(), weekly.map(decimal));
    tokens.insert(
        "lineup_weekly_text".into(),
        Some(weekly_text(
            weekly,
            usage.and_then(|usage| usage.weekly_reset_unix),
            now_unix,
        )),
    );
    tokens
}

fn percentage(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
}

fn decimal(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    }
}

/// Weekly quota presentation is isolated here so later formatting changes do
/// not affect numeric tokens. Seconds are Unix timestamps supplied by callers.
/// Remaining time is floored to minutes/hours and past resets display `0m`.
pub fn weekly_text(remaining: Option<f64>, reset_unix: Option<i64>, now_unix: i64) -> String {
    let Some(remaining) = percentage(remaining) else {
        return "—".into();
    };
    let mut text = format!("7d {}%", remaining.round() as u8);
    if let Some(reset) = reset_unix {
        let seconds = reset.saturating_sub(now_unix).max(0);
        let duration = if seconds >= 86_400 {
            format!("{}d{}h", seconds / 86_400, seconds % 86_400 / 3_600)
        } else if seconds >= 3_600 {
            format!("{}h{}m", seconds / 3_600, seconds % 3_600 / 60)
        } else {
            format!("{}m", seconds / 60)
        };
        text.push(' ');
        text.push_str(&duration);
    }
    text
}

/// Collapse whitespace/control characters and truncate at Unicode scalar
/// boundaries. Empty strings and `None` both encode JSON null clears.
pub fn normalize_value(value: Option<&str>) -> Option<String> {
    let value = value?;
    let cleaned: String = value
        .chars()
        .map(|c| {
            if c.is_control() || c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let normalized: String = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ResourceKind {
    Pane,
    Workspace,
}
/// The endpoint/session identity must change when the socket server is replaced.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ResourceId {
    pub endpoint_session: String,
    pub kind: ResourceKind,
    pub id: String,
}
#[derive(Clone, Debug, Default)]
pub struct ResourceSnapshot {
    pub exists: bool,
    pub stored_keys: BTreeSet<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatchError {
    InvalidName(String),
    UndocumentedKey(String),
    ReportCapacity,
    ResourceCapacity,
    InvalidRefresh,
    SequenceExhausted,
    UnreservedReport,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    pub resource: ResourceId,
    pub source: &'static str,
    pub seq: u64,
    pub ttl_ms: u64,
    pub tokens: Tokens,
    occupant: Option<Occupant>,
}
#[derive(Clone, Debug, Default)]
struct ResourceState {
    high_water: u64,
    accepted: Tokens,
    published_ms: BTreeMap<String, u64>,
    occupant: Option<Occupant>,
    reserved: Option<Report>,
}

/// Pure planner. The daemon must durably persist `checkpoint()` after every
/// successful `prepare()` and BEFORE sending its report. If persistence fails,
/// do not send. Only call `acknowledge()` after the server accepts the report.
/// Restoring a checkpoint intentionally drops cached values and forces a full
/// authoritative republish, retaining per-endpoint/session/resource sequences.
#[derive(Clone, Debug, Default)]
pub struct Publisher {
    resources: BTreeMap<ResourceId, ResourceState>,
}
impl Publisher {
    pub fn restore(checkpoint: BTreeMap<ResourceId, u64>) -> Self {
        Self {
            resources: checkpoint
                .into_iter()
                .map(|(resource, high_water)| {
                    (
                        resource,
                        ResourceState {
                            high_water,
                            ..ResourceState::default()
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn checkpoint(&self) -> BTreeMap<ResourceId, u64> {
        self.resources
            .iter()
            .map(|(resource, state)| (resource.clone(), state.high_water))
            .collect()
    }

    /// `now_ms` is an injected monotonic clock; Unix time is only for displays.
    /// Missing resources never receive reports. Snapshot keys include all
    /// sources, so capacity failures never evict another plugin's metadata.
    /// Omitted desired keys remain unchanged. Supply all twelve pane keys from
    /// `pane_tokens` on reconciliation to clear departed occupants completely.
    pub fn prepare(
        &mut self,
        resource: &ResourceId,
        snapshot: &ResourceSnapshot,
        occupant: Option<&Occupant>,
        desired: &Tokens,
        now_ms: u64,
        refresh_ms: u64,
    ) -> Result<Option<Report>, PatchError> {
        if !snapshot.exists {
            if let Some(state) = self.resources.get_mut(resource) {
                *state = ResourceState {
                    high_water: state.high_water,
                    ..ResourceState::default()
                };
            }
            return Ok(None);
        }
        let ttl_ms = refresh_ms
            .checked_mul(3)
            .filter(|_| refresh_ms > 0)
            .ok_or(PatchError::InvalidRefresh)?;
        if desired.len() > 16 {
            return Err(PatchError::ReportCapacity);
        }
        let allowed: &[&str] = match resource.kind {
            ResourceKind::Pane => &PANE_KEYS,
            ResourceKind::Workspace => &WORKSPACE_KEYS,
        };
        let mut normalized = Tokens::new();
        for (key, value) in desired {
            if !key.starts_with("lineup_")
                || key.len() > 32
                || !key
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                return Err(PatchError::InvalidName(key.clone()));
            }
            if !allowed.contains(&key.as_str()) {
                return Err(PatchError::UndocumentedKey(key.clone()));
            }
            normalized.insert(key.clone(), normalize_value(value.as_deref()));
        }
        let mut stored_keys = snapshot.stored_keys.clone();
        for (key, value) in &normalized {
            if value.is_some() {
                stored_keys.insert(key.clone());
            } else {
                stored_keys.remove(key);
            }
        }
        if stored_keys.len() > 32 {
            return Err(PatchError::ResourceCapacity);
        }
        let state = self.resources.get(resource).cloned().unwrap_or_default();
        let occupant_changed = state.occupant.as_ref() != occupant;
        let tokens: Tokens = normalized
            .into_iter()
            .filter(|(key, value)| {
                occupant_changed
                    || state.accepted.get(key) != Some(value)
                    || (value.is_some()
                        && state
                            .published_ms
                            .get(key)
                            .is_none_or(|last| now_ms < *last || now_ms - *last >= refresh_ms * 2))
            })
            .collect();
        if tokens.is_empty() {
            return Ok(None);
        }
        let seq = state
            .high_water
            .checked_add(1)
            .ok_or(PatchError::SequenceExhausted)?;
        let report = Report {
            resource: resource.clone(),
            source: SOURCE,
            seq,
            ttl_ms,
            tokens,
            occupant: occupant.cloned(),
        };
        let state = self.resources.entry(resource.clone()).or_default();
        state.high_water = seq;
        state.reserved = Some(report.clone());
        Ok(Some(report))
    }

    /// Reject stale, modified, or unreserved reports. Failed sends leave both
    /// cached values and renewal deadlines untouched and retry with a new seq.
    pub fn acknowledge(&mut self, report: &Report, now_ms: u64) -> Result<(), PatchError> {
        let state = self
            .resources
            .get_mut(&report.resource)
            .ok_or(PatchError::UnreservedReport)?;
        if state.reserved.as_ref() != Some(report) {
            return Err(PatchError::UnreservedReport);
        }
        for (key, value) in &report.tokens {
            state.accepted.insert(key.clone(), value.clone());
            if value.is_some() {
                state.published_ms.insert(key.clone(), now_ms);
            } else {
                state.published_ms.remove(key);
            }
        }
        state.occupant = report.occupant.clone();
        state.reserved = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occupant(agent: Agent, session: &str) -> Occupant {
        Occupant {
            agent,
            native_session: if session.is_empty() {
                None
            } else {
                Some(session.into())
            },
        }
    }
    fn row() -> PaneInput {
        let who = occupant(Agent::Claude, "s1");
        PaneInput {
            occupant: Some(who.clone()),
            state: Some("working".into()),
            usage: Some(Usage {
                occupant: who,
                context_pct: Some(7.2),
                weekly_remaining_pct: None,
                weekly_reset_unix: None,
            }),
        }
    }
    fn resource(id: &str) -> ResourceId {
        ResourceId {
            endpoint_session: "socket/session".into(),
            kind: ResourceKind::Pane,
            id: id.into(),
        }
    }
    fn snapshot(unrelated: usize) -> ResourceSnapshot {
        ResourceSnapshot {
            exists: true,
            stored_keys: (0..unrelated).map(|i| format!("other_{i}")).collect(),
        }
    }
    fn value<'a>(tokens: &'a Tokens, key: &str) -> Option<&'a str> {
        tokens.get(key).unwrap().as_deref()
    }

    #[test]
    fn partial_usage_and_numeric_null_contract() {
        let tokens = pane_tokens(&row(), LogoMode::Text, false, 0);
        assert_eq!(tokens.len(), 12);
        assert_eq!(value(&tokens, "lineup_context_pct"), Some("7.2"));
        assert_eq!(value(&tokens, "lineup_context_text"), Some("07%"));
        assert_eq!(value(&tokens, "lineup_weekly_pct"), None);
        assert_eq!(value(&tokens, "lineup_weekly_text"), Some("—"));
        let mut full = row();
        let usage = full.usage.as_mut().unwrap();
        usage.context_pct = Some(100.0);
        usage.weekly_remaining_pct = Some(0.0);
        let tokens = pane_tokens(&full, LogoMode::Text, false, 0);
        assert_eq!(value(&tokens, "lineup_context_text"), Some("100%"));
        assert_eq!(value(&tokens, "lineup_weekly_pct"), Some("0"));
        assert_eq!(value(&tokens, "lineup_weekly_text"), Some("7d 0%"));
    }

    #[test]
    fn states_have_exactly_one_dot_and_unknown_is_conservative() {
        for raw in [
            Some("idle"),
            Some("working"),
            Some("blocked"),
            Some("done"),
            Some("unknown"),
            Some("future"),
            None,
        ] {
            let mut row = row();
            row.state = raw.map(str::to_owned);
            let tokens = pane_tokens(&row, LogoMode::Text, false, 0);
            let expected = raw
                .filter(|s| ["idle", "working", "blocked", "done", "unknown"].contains(s))
                .unwrap_or("unknown");
            assert_eq!(value(&tokens, "lineup_state"), Some(expected));
            for state in ["idle", "working", "blocked", "done", "unknown"] {
                assert_eq!(
                    value(&tokens, &format!("lineup_state_{state}")),
                    if state == expected { Some("●") } else { None }
                );
            }
        }
    }

    #[test]
    fn all_vendor_glyphs_and_auto_text_fallback() {
        for (agent, label, glyph) in [
            (Agent::Claude, "Claude", "\u{e1a0}"),
            (Agent::Codex, "Codex", "\u{e1a1}"),
            (Agent::Grok, "Grok", "\u{e1b1}"),
            (Agent::Agy, "Antigravity", "\u{e1b2}"),
            (Agent::Kimi, "Kimi", "\u{e1a6}"),
            (Agent::Hermes, "Hermes", "\u{e1aa}"),
        ] {
            let mut row = row();
            row.occupant = Some(occupant(agent, "s1"));
            for (mode, verified, expected) in [
                (LogoMode::Text, true, label),
                (LogoMode::Font, false, glyph),
                (LogoMode::Auto, false, label),
                (LogoMode::Auto, true, glyph),
            ] {
                assert_eq!(
                    value(&pane_tokens(&row, mode, verified, 0), "lineup_logo"),
                    Some(expected)
                );
            }
        }
    }

    #[test]
    fn invalid_percentages_clear_and_rounding_is_half_up() {
        for percent in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(-1.0),
            Some(100.1),
        ] {
            let mut row = row();
            let usage = row.usage.as_mut().unwrap();
            usage.context_pct = percent;
            usage.weekly_remaining_pct = percent;
            let tokens = pane_tokens(&row, LogoMode::Text, false, 0);
            for key in ["lineup_context_pct", "lineup_weekly_pct"] {
                assert_eq!(value(&tokens, key), None);
            }
            assert_eq!(value(&tokens, "lineup_context_text"), Some("—"));
        }
        for (percent, expected) in [(0.0, "00%"), (7.49, "07%"), (7.5, "08%"), (99.5, "100%")] {
            let mut row = row();
            row.usage.as_mut().unwrap().context_pct = Some(percent);
            assert_eq!(
                value(
                    &pane_tokens(&row, LogoMode::Text, false, 0),
                    "lineup_context_text"
                ),
                Some(expected)
            );
        }
        let mut row = row();
        row.usage.as_mut().unwrap().context_pct = Some(-0.0);
        assert_eq!(
            value(
                &pane_tokens(&row, LogoMode::Text, false, 0),
                "lineup_context_pct"
            ),
            Some("0")
        );
    }

    #[test]
    fn weekly_reset_boundaries_and_missing_data() {
        for (seconds, suffix) in [
            (0, "0m"),
            (59, "0m"),
            (60, "1m"),
            (3599, "59m"),
            (3600, "1h0m"),
            (12360, "3h26m"),
            (86399, "23h59m"),
            (86400, "1d0h"),
            (552000, "6d9h"),
        ] {
            assert_eq!(
                weekly_text(Some(67.0), Some(100 + seconds), 100),
                format!("7d 67% {suffix}")
            );
        }
        assert_eq!(weekly_text(Some(67.5), None, 100), "7d 68%");
        assert_eq!(weekly_text(None, Some(1000), 100), "—");
        assert_eq!(weekly_text(Some(0.0), Some(99), 100), "7d 0% 0m");
        assert_eq!(
            weekly_text(Some(100.0), Some(i64::MAX), i64::MIN),
            "7d 100% 106751991167300d15h"
        );
    }

    #[test]
    fn missing_identity_and_session_mismatch_never_reuse_usage() {
        let mut row = row();
        row.occupant = Some(occupant(Agent::Codex, "s2"));
        let tokens = pane_tokens(&row, LogoMode::Text, false, 0);
        assert_eq!(value(&tokens, "lineup_agent"), Some("codex"));
        assert_eq!(value(&tokens, "lineup_context_pct"), None);
        row.occupant = None;
        assert!(
            pane_tokens(&row, LogoMode::Text, false, 0)
                .values()
                .all(Option::is_none)
        );
    }

    #[test]
    fn supported_agent_without_native_session_keeps_logo_but_clears_usage() {
        let mut row = row();
        let who = occupant(Agent::Kimi, "");
        row.occupant = Some(who.clone());
        row.usage.as_mut().unwrap().occupant = who;
        row.usage.as_mut().unwrap().weekly_remaining_pct = Some(67.0);
        let tokens = pane_tokens(&row, LogoMode::Text, false, 0);
        assert_eq!(value(&tokens, "lineup_agent"), Some("kimi"));
        assert_eq!(value(&tokens, "lineup_logo"), Some("Kimi"));
        assert_eq!(value(&tokens, "lineup_state"), Some("working"));
        assert_eq!(value(&tokens, "lineup_context_pct"), None);
        assert_eq!(value(&tokens, "lineup_weekly_pct"), None);
        assert_eq!(value(&tokens, "lineup_context_text"), Some("—"));
        assert_eq!(value(&tokens, "lineup_weekly_text"), Some("—"));
    }

    #[test]
    fn normalization_obeys_unicode_length_and_single_line() {
        assert_eq!(
            normalize_value(Some("  a\r\nb\t c  ")),
            Some("a b c".into())
        );
        assert_eq!(normalize_value(Some(" \n\t")), None);
        assert_eq!(normalize_value(None), None);
        assert_eq!(
            normalize_value(Some(&"界".repeat(81))).unwrap(),
            "界".repeat(80)
        );
    }

    #[test]
    fn dirty_keys_and_ttl_renewal_use_successful_publication_time() {
        let resource = resource("p1");
        let row = row();
        let desired = pane_tokens(&row, LogoMode::Text, false, 0);
        let mut publisher = Publisher::default();
        let first = publisher
            .prepare(
                &resource,
                &snapshot(0),
                row.occupant.as_ref(),
                &desired,
                0,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(first.tokens.len(), 12);
        assert_eq!(first.seq, 1);
        assert_eq!(first.ttl_ms, 15000);
        assert_eq!(first.source, SOURCE);
        publisher.acknowledge(&first, 1000).unwrap();
        assert!(
            publisher
                .prepare(
                    &resource,
                    &snapshot(0),
                    row.occupant.as_ref(),
                    &desired,
                    10999,
                    5000
                )
                .unwrap()
                .is_none()
        );
        let renewal = publisher
            .prepare(
                &resource,
                &snapshot(0),
                row.occupant.as_ref(),
                &desired,
                11000,
                5000,
            )
            .unwrap()
            .unwrap();
        assert!(renewal.tokens.values().all(Option::is_some));
        let retry = publisher
            .prepare(
                &resource,
                &snapshot(0),
                row.occupant.as_ref(),
                &desired,
                11001,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(retry.tokens, renewal.tokens);
        assert!(retry.seq > renewal.seq);
        publisher.acknowledge(&retry, 11001).unwrap();
        assert_eq!(
            publisher.acknowledge(&renewal, 11002),
            Err(PatchError::UnreservedReport)
        );
        let mut changed = desired.clone();
        changed.insert("lineup_context_pct".into(), None);
        changed.insert("lineup_context_text".into(), Some("—".into()));
        let patch = publisher
            .prepare(
                &resource,
                &snapshot(0),
                row.occupant.as_ref(),
                &changed,
                12000,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(patch.tokens.len(), 2);
        assert_eq!(value(&patch.tokens, "lineup_context_pct"), None);
    }

    #[test]
    fn replacement_and_departure_clear_all_owned_keys_only() {
        let resource = resource("p1");
        let mut row = row();
        let mut publisher = Publisher::default();
        let first = publisher
            .prepare(
                &resource,
                &snapshot(3),
                row.occupant.as_ref(),
                &pane_tokens(&row, LogoMode::Text, false, 0),
                0,
                5000,
            )
            .unwrap()
            .unwrap();
        publisher.acknowledge(&first, 0).unwrap();
        row.occupant = Some(occupant(Agent::Claude, "s2"));
        let replacement = publisher
            .prepare(
                &resource,
                &snapshot(3),
                row.occupant.as_ref(),
                &pane_tokens(&row, LogoMode::Text, false, 0),
                1,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(replacement.tokens.len(), 12);
        assert_eq!(value(&replacement.tokens, "lineup_context_pct"), None);
        publisher.acknowledge(&replacement, 1).unwrap();
        row.occupant = None;
        let clear = publisher
            .prepare(
                &resource,
                &snapshot(3),
                None,
                &pane_tokens(&row, LogoMode::Text, false, 0),
                2,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(clear.tokens.len(), 12);
        assert!(clear.tokens.values().all(Option::is_none));
        publisher.acknowledge(&clear, 2).unwrap();
        assert!(
            publisher
                .prepare(&resource, &snapshot(3), None, &clear.tokens, 20000, 5000)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn sequence_restart_and_resource_isolation() {
        let row = row();
        let desired = pane_tokens(&row, LogoMode::Text, false, 0);
        let mut publisher = Publisher::default();
        let first = publisher
            .prepare(
                &resource("p1"),
                &snapshot(0),
                row.occupant.as_ref(),
                &desired,
                0,
                5000,
            )
            .unwrap()
            .unwrap();
        let mut restarted = Publisher::restore(publisher.checkpoint());
        assert_eq!(
            restarted
                .prepare(
                    &resource("p1"),
                    &snapshot(0),
                    row.occupant.as_ref(),
                    &desired,
                    0,
                    5000
                )
                .unwrap()
                .unwrap()
                .seq,
            first.seq + 1
        );
        assert_eq!(
            restarted
                .prepare(
                    &resource("p2"),
                    &snapshot(0),
                    row.occupant.as_ref(),
                    &desired,
                    0,
                    5000
                )
                .unwrap()
                .unwrap()
                .seq,
            1
        );
        let mut other = resource("p1");
        other.endpoint_session = "new/socket/session".into();
        assert_eq!(
            restarted
                .prepare(
                    &other,
                    &snapshot(0),
                    row.occupant.as_ref(),
                    &desired,
                    0,
                    5000
                )
                .unwrap()
                .unwrap()
                .seq,
            1
        );
        let mut exhausted = Publisher::restore(BTreeMap::from([(resource("p1"), u64::MAX)]));
        assert_eq!(
            exhausted
                .prepare(&resource("p1"), &snapshot(0), None, &desired, 0, 5000)
                .unwrap_err(),
            PatchError::SequenceExhausted
        );
    }

    #[test]
    fn limits_skip_resource_without_allocating_sequence() {
        let mut row = row();
        row.usage.as_mut().unwrap().weekly_remaining_pct = Some(67.0);
        let desired = pane_tokens(&row, LogoMode::Text, false, 0);
        let mut publisher = Publisher::default();
        let id = resource("p1");
        assert!(
            publisher
                .prepare(&id, &snapshot(25), row.occupant.as_ref(), &desired, 0, 5000)
                .is_err()
        );
        assert!(publisher.checkpoint().is_empty());
        assert!(
            publisher
                .prepare(&id, &ResourceSnapshot::default(), None, &desired, 0, 5000)
                .unwrap()
                .is_none()
        );
        assert!(
            publisher
                .prepare(&id, &snapshot(0), None, &desired, 0, 0)
                .is_err()
        );
        for name in [
            "other",
            "lineup_bad.name",
            "lineup_界",
            &format!("lineup_{}", "x".repeat(26)),
        ] {
            assert!(
                publisher
                    .prepare(
                        &id,
                        &snapshot(0),
                        None,
                        &BTreeMap::from([(name.into(), Some("v".into()))]),
                        0,
                        5000
                    )
                    .is_err()
            );
        }
        let too_many = (0..17)
            .map(|i| (format!("lineup_{i}"), Some("v".into())))
            .collect();
        assert_eq!(
            publisher
                .prepare(&id, &snapshot(0), None, &too_many, 0, 5000)
                .unwrap_err(),
            PatchError::ReportCapacity
        );
        assert!(
            publisher
                .prepare(&id, &snapshot(24), None, &desired, 0, 5000)
                .unwrap()
                .is_some()
        );
    }
}
