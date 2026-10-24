//! Pure sidebar presentation and metadata patch planning. No socket or disk I/O.

use std::collections::{BTreeMap, BTreeSet};

pub type Tokens = BTreeMap<String, Option<String>>;
pub const SOURCE: &str = "herdr-lineup";
pub const PANE_KEYS: [&str; 14] = [
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
    "lineup_group",
    "lineup_group_ship_text",
];
pub const WORKSPACE_KEYS: [&str; 3] = ["lineup_repo", "lineup_ship_unix", "lineup_ship_text"];

// Visible sidebar fields are required whenever populated. Optional fields are
// ranked from most to least preferred; capacity pressure drops them in reverse.
const PANE_CAPACITY_PRIORITY: [(&str, bool); 14] = [
    ("lineup_logo", true),
    ("lineup_state_idle", true),
    ("lineup_state_working", true),
    ("lineup_state_blocked", true),
    ("lineup_state_done", true),
    ("lineup_state_unknown", true),
    ("lineup_context_text", true),
    ("lineup_weekly_text", true),
    ("lineup_group", true),
    ("lineup_group_ship_text", true),
    ("lineup_agent", false),
    ("lineup_state", false),
    ("lineup_context_pct", false),
    ("lineup_weekly_pct", false),
];

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
pub(crate) struct AgentInfo {
    pub(crate) agent: Agent,
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) glyph: &'static str,
}

impl Agent {
    pub(crate) const ALL: [AgentInfo; 6] = [
        AgentInfo {
            agent: Agent::Claude,
            id: "claude",
            label: "Claude",
            glyph: "\u{e1a0}",
        },
        AgentInfo {
            agent: Agent::Codex,
            id: "codex",
            label: "Codex",
            glyph: "\u{e1a1}",
        },
        AgentInfo {
            agent: Agent::Grok,
            id: "grok",
            label: "Grok",
            glyph: "\u{e1b1}",
        },
        AgentInfo {
            agent: Agent::Agy,
            id: "agy",
            label: "Antigravity",
            glyph: "\u{e1b2}",
        },
        AgentInfo {
            agent: Agent::Kimi,
            id: "kimi",
            label: "Kimi",
            glyph: "\u{e1a6}",
        },
        AgentInfo {
            agent: Agent::Hermes,
            id: "hermes",
            label: "Hermes",
            glyph: "\u{e1aa}",
        },
    ];

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.agent)
    }

    pub(crate) fn info(self) -> &'static AgentInfo {
        Self::ALL
            .iter()
            .find(|entry| entry.agent == self)
            .expect("every Agent has a catalog entry")
    }
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
    pub binding: Option<crate::model::UsageBinding>,
    pub occupant: Occupant,
    pub context_pct: Option<f64>,
    pub weekly_remaining_pct: Option<f64>,
    pub weekly_reset_unix: Option<i64>,
}
#[derive(Clone, Debug, Default)]
pub struct PaneInput {
    pub pane_id: String,
    pub occupant: Option<Occupant>,
    pub state: Option<String>,
    pub usage: Option<Usage>,
}

/// Unsupported or unidentified occupants are represented by `None`.
/// Usage is accepted only when bound to this exact occupant and a matching
/// native session or pane ID. `font_verified` means affirmative terminal rendering verification,
/// never mere font presence. Provider readers must remove stale/invalid samples
/// before supplying usage; this module does not read or assess reader freshness.
pub fn pane_tokens(row: &PaneInput, mode: LogoMode, font_verified: bool, now_unix: i64) -> Tokens {
    let mut tokens: Tokens = PANE_KEYS.iter().map(|key| ((*key).into(), None)).collect();
    let Some(occupant) = row.occupant.as_ref() else {
        return tokens;
    };
    let info = occupant.agent.info();
    let logo = if mode == LogoMode::Font || (mode == LogoMode::Auto && font_verified) {
        info.glyph
    } else {
        info.label
    };
    let state = row
        .state
        .as_deref()
        .filter(|state| ["idle", "working", "blocked", "done", "unknown"].contains(state))
        .unwrap_or("unknown");
    tokens.insert("lineup_agent".into(), Some(info.id.into()));
    tokens.insert("lineup_logo".into(), Some(logo.into()));
    tokens.insert("lineup_state".into(), Some(state.into()));
    tokens.insert(format!("lineup_state_{state}"), Some("●".into()));
    let usage = row.usage.as_ref().filter(|usage| {
        usage.occupant == *occupant
            && match usage.binding.as_ref() {
                Some(crate::model::UsageBinding::NativeSession(id)) => {
                    !id.trim().is_empty() && occupant.native_session.as_ref() == Some(id)
                }
                Some(crate::model::UsageBinding::Pane(id)) => {
                    !id.trim().is_empty() && row.pane_id == *id
                }
                None => false,
            }
    });
    let context = percentage(usage.and_then(|usage| usage.context_pct));
    let weekly = percentage(usage.and_then(|usage| usage.weekly_remaining_pct));
    tokens.insert("lineup_context_pct".into(), context.map(decimal));
    tokens.insert(
        "lineup_context_text".into(),
        Some(
            context
                .map(|value| format!("{:02}%", value.round() as u8))
                .unwrap_or_else(|| "—\u{2800}\u{2800}".into()),
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
    let mut text = format!("7d {:02}%", remaining.round() as u8);
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

/// Pure planner. Before sending, the daemon must durably reserve and confirm
/// an endpoint sequence window covering the prepared report. Reports within
/// that window need no per-prepare persistence. If reservation persistence
/// fails, do not send. Only call `acknowledge()` after server acceptance.
/// Restoring a checkpoint intentionally drops cached values and forces a full
/// authoritative republish, retaining per-endpoint/session/resource sequences.
#[derive(Clone, Debug, Default)]
pub struct Publisher {
    resources: BTreeMap<ResourceId, ResourceState>,
    endpoint_floors: BTreeMap<String, u64>,
    endpoint_cursors: BTreeMap<String, u64>,
}
impl Publisher {
    pub fn restore(checkpoint: BTreeMap<ResourceId, u64>) -> Self {
        Self::restore_with_floors(checkpoint, BTreeMap::new())
    }

    pub fn restore_with_floors(
        checkpoint: BTreeMap<ResourceId, u64>,
        endpoint_floors: BTreeMap<String, u64>,
    ) -> Self {
        let mut endpoint_cursors = BTreeMap::new();
        for (resource, high_water) in &checkpoint {
            let entry = endpoint_cursors
                .entry(resource.endpoint_session.clone())
                .or_insert(0);
            *entry = (*entry).max(*high_water);
        }
        for (endpoint, floor) in &endpoint_floors {
            let entry = endpoint_cursors.entry(endpoint.clone()).or_insert(0);
            *entry = (*entry).max(*floor);
        }
        let mut floors = endpoint_floors;
        for (endpoint, &base) in &endpoint_cursors {
            let f = floors.entry(endpoint.clone()).or_insert(0);
            *f = (*f).max(base);
        }
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
            endpoint_floors: floors,
            endpoint_cursors,
        }
    }

    pub fn checkpoint(&self) -> BTreeMap<ResourceId, u64> {
        self.resources
            .iter()
            .map(|(resource, state)| (resource.clone(), state.high_water))
            .collect()
    }

    pub fn checkpoint_floors(&self) -> BTreeMap<String, u64> {
        self.endpoint_floors.clone()
    }

    pub fn endpoint_window(&self, endpoint: &str) -> (u64, u64) {
        let cursor = self.endpoint_cursors.get(endpoint).copied().unwrap_or(0);
        let upper_bound = self.endpoint_floors.get(endpoint).copied().unwrap_or(0);
        (cursor, upper_bound)
    }

    pub fn has_available_reservation(&self, endpoint: &str) -> bool {
        let (cursor, upper_bound) = self.endpoint_window(endpoint);
        upper_bound > 0 && cursor < upper_bound
    }

    pub fn plan_reservation(&self, endpoint: &str, window_size: u64) -> Result<u64, PatchError> {
        let (cursor, upper_bound) = self.endpoint_window(endpoint);
        let base = cursor.max(upper_bound);
        base.checked_add(window_size)
            .ok_or(PatchError::SequenceExhausted)
    }

    pub fn confirm_reservation(
        &mut self,
        endpoint: &str,
        new_upper_bound: u64,
    ) -> Result<(), PatchError> {
        let (cursor, upper_bound) = self.endpoint_window(endpoint);
        if new_upper_bound < upper_bound || new_upper_bound < cursor {
            return Err(PatchError::SequenceExhausted);
        }
        self.endpoint_floors
            .insert(endpoint.into(), new_upper_bound);
        self.endpoint_cursors
            .entry(endpoint.into())
            .or_insert(cursor);
        Ok(())
    }

    #[cfg(test)]
    pub fn set_cursor_for_test(&mut self, endpoint: &str, cursor: u64) {
        self.endpoint_cursors.insert(endpoint.into(), cursor);
    }

    /// The authoritative snapshot permits removal of absent resource rows.
    /// Their maximum reserved sequence remains as a durable endpoint floor,
    /// so a resource ID reused by this same server cannot restart at seq 1.
    /// A different endpoint has no access to the prior server's metadata.
    pub fn prune_for_snapshot(&mut self, endpoint: &str, live: &BTreeSet<ResourceId>) -> bool {
        let mut changed = false;
        let mut floor = self
            .endpoint_floors
            .get(endpoint)
            .copied()
            .unwrap_or_default();
        let cursor = self
            .endpoint_cursors
            .get(endpoint)
            .copied()
            .unwrap_or_default();
        floor = floor.max(cursor);
        self.resources.retain(|resource, state| {
            if resource.endpoint_session != endpoint {
                changed = true;
                return false;
            }
            if !live.contains(resource) {
                floor = floor.max(state.high_water);
                changed = true;
                return false;
            }
            true
        });
        self.endpoint_floors.retain(|key, _| {
            let keep = key == endpoint;
            changed |= !keep;
            keep
        });
        self.endpoint_cursors.retain(|key, _| key == endpoint);
        if floor > 0 {
            changed |= self.endpoint_floors.insert(endpoint.into(), floor) != Some(floor);
        }
        changed
    }

    pub fn needs_report(
        &self,
        resource: &ResourceId,
        snapshot: &ResourceSnapshot,
        occupant: Option<&Occupant>,
        desired: &Tokens,
        now_ms: u64,
        refresh_ms: u64,
    ) -> Result<bool, PatchError> {
        let Some((tokens, _)) =
            self.plan_tokens(resource, snapshot, occupant, desired, now_ms, refresh_ms)?
        else {
            return Ok(false);
        };
        Ok(!tokens.is_empty())
    }

    fn plan_tokens(
        &self,
        resource: &ResourceId,
        snapshot: &ResourceSnapshot,
        occupant: Option<&Occupant>,
        desired: &Tokens,
        now_ms: u64,
        refresh_ms: u64,
    ) -> Result<Option<(Tokens, u64)>, PatchError> {
        if !snapshot.exists {
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
        if stored_keys.len() > 32 && resource.kind == ResourceKind::Pane {
            for (key, required) in PANE_CAPACITY_PRIORITY.iter().rev() {
                if stored_keys.len() <= 32 {
                    break;
                }
                if !required && normalized.get(*key).is_some_and(Option::is_some) {
                    normalized.insert((*key).into(), None);
                    stored_keys.remove(*key);
                }
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
                        && (!snapshot.stored_keys.contains(key)
                            || state
                                .published_ms
                                .get(key)
                                .is_none_or(|last| now_ms < *last || now_ms - *last >= refresh_ms)))
            })
            .collect();
        if tokens.is_empty() {
            Ok(None)
        } else {
            Ok(Some((tokens, ttl_ms)))
        }
    }

    /// `now_ms` is an injected monotonic clock; Unix time is only for displays.
    /// Missing resources never receive reports. Snapshot keys include all
    /// sources, so capacity failures never evict another plugin's metadata.
    /// Renew after one interval to leave room for cycle duration and jitter.
    /// Authoritatively missing live keys are repaired even before renewal.
    /// Omitted desired keys remain unchanged. Supply all fourteen pane keys from
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
        let Some((tokens, ttl_ms)) =
            self.plan_tokens(resource, snapshot, occupant, desired, now_ms, refresh_ms)?
        else {
            return Ok(None);
        };
        let (cursor, upper_bound) = self.endpoint_window(&resource.endpoint_session);
        let seq = if upper_bound > 0 {
            if cursor >= upper_bound {
                return Err(PatchError::SequenceExhausted);
            }
            cursor.checked_add(1).ok_or(PatchError::SequenceExhausted)?
        } else {
            let current = self
                .resources
                .get(resource)
                .map(|s| s.high_water)
                .unwrap_or(0);
            current
                .checked_add(1)
                .ok_or(PatchError::SequenceExhausted)?
        };
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
        if upper_bound > 0 {
            self.endpoint_cursors
                .insert(resource.endpoint_session.clone(), seq);
        }
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
            pane_id: "pane-1".into(),
            occupant: Some(who.clone()),
            state: Some("working".into()),
            usage: Some(Usage {
                binding: Some(crate::model::UsageBinding::NativeSession("s1".into())),
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

    // Unicode 17 EastAsianWidth.txt: U+2014 is A (ambiguous), U+2800 is N
    // (neutral), and ASCII digits/% are Na (narrow). Use the narrow-ambiguous
    // terminal policy confirmed for the sidebar, independent of host locale.
    // https://www.unicode.org/Public/17.0.0/ucd/EastAsianWidth.txt
    // This table covers our display alphabet, not arbitrary Unicode strings.
    fn terminal_cells(text: &str) -> usize {
        text.chars()
            .map(|character| match character {
                '—' | '\u{2800}' | '0'..='9' | '%' | ' ' | 'd' | 'h' | 'm' | 'e' => 1,
                '界' => 2,
                '\u{301}' => 0,
                _ => panic!("character outside the cell-width fixture: {character:?}"),
            })
            .sum()
    }

    #[test]
    fn context_missing_and_two_digit_values_have_equal_terminal_cells() {
        assert_eq!(terminal_cells("—"), 1);
        assert_eq!(terminal_cells("\u{2800}"), 1);
        // Controls for the oracle: cell width is not Unicode scalar count.
        assert_eq!(terminal_cells("界"), 2);
        assert_eq!(terminal_cells("e\u{301}"), 1);
        let mut missing_row = row();
        missing_row.usage = None;
        let missing = pane_tokens(&missing_row, LogoMode::Text, false, 0);
        let text = value(&missing, "lineup_context_text").unwrap();
        assert_eq!(terminal_cells(text), 3);
        assert_eq!(text, "—\u{2800}\u{2800}");
        let normalized = normalize_value(Some(text)).unwrap();
        assert_eq!(normalized, text);
        assert_eq!(terminal_cells(&normalized), 3);
        for percent in [0.0, 7.2, 42.0, 99.49] {
            let mut row = row();
            row.usage.as_mut().unwrap().context_pct = Some(percent);
            let tokens = pane_tokens(&row, LogoMode::Text, false, 0);
            assert_eq!(
                terminal_cells(value(&tokens, "lineup_context_text").unwrap()),
                3
            );
            assert_eq!(
                value(&tokens, "lineup_context_pct"),
                Some(decimal(percent).as_str())
            );
        }
        for percent in [99.5, 100.0] {
            let mut row = row();
            row.usage.as_mut().unwrap().context_pct = Some(percent);
            let tokens = pane_tokens(&row, LogoMode::Text, false, 0);
            assert_eq!(value(&tokens, "lineup_context_text"), Some("100%"));
            assert_eq!(
                terminal_cells(value(&tokens, "lineup_context_text").unwrap()),
                4
            );
        }
    }

    #[test]
    fn weekly_two_digit_remaining_aligns_countdown_and_preserves_numeric() {
        for (percent, display) in [
            (0.0, "00"),
            (7.2, "07"),
            (7.5, "08"),
            (42.0, "42"),
            (99.49, "99"),
        ] {
            let mut row = row();
            let usage = row.usage.as_mut().unwrap();
            usage.weekly_remaining_pct = Some(percent);
            usage.weekly_reset_unix = Some(160);
            let tokens = pane_tokens(&row, LogoMode::Text, false, 100);
            let text = value(&tokens, "lineup_weekly_text").unwrap();
            assert_eq!(text, format!("7d {display}% 1m"));
            assert_eq!(terminal_cells(text.split("1m").next().unwrap()), 7);
            assert_eq!(
                value(&tokens, "lineup_weekly_pct"),
                Some(decimal(percent).as_str())
            );
            assert_eq!(
                weekly_text(Some(percent), None, 100),
                format!("7d {display}%")
            );
        }
        assert_eq!(weekly_text(Some(99.5), Some(160), 100), "7d 100% 1m");
        assert_eq!(weekly_text(None, Some(160), 100), "—");
    }

    #[test]
    fn partial_usage_and_numeric_null_contract() {
        let tokens = pane_tokens(&row(), LogoMode::Text, false, 0);
        assert_eq!(tokens.len(), 14);
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
        assert_eq!(value(&tokens, "lineup_weekly_text"), Some("7d 00%"));
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
    fn agent_catalog_preserves_ids_labels_glyphs_and_order() {
        let expected = [
            (Agent::Claude, "claude", "Claude", "\u{e1a0}"),
            (Agent::Codex, "codex", "Codex", "\u{e1a1}"),
            (Agent::Grok, "grok", "Grok", "\u{e1b1}"),
            (Agent::Agy, "agy", "Antigravity", "\u{e1b2}"),
            (Agent::Kimi, "kimi", "Kimi", "\u{e1a6}"),
            (Agent::Hermes, "hermes", "Hermes", "\u{e1aa}"),
        ];
        assert_eq!(Agent::ALL.len(), expected.len());
        for (entry, (agent, id, label, glyph)) in Agent::ALL.iter().zip(expected) {
            assert_eq!(entry.agent, agent);
            assert_eq!(entry.id, id);
            assert_eq!(entry.label, label);
            assert_eq!(entry.glyph, glyph);
            assert_eq!(Agent::from_id(id), Some(agent));
        }
        assert_eq!(Agent::from_id("unknown"), None);
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
            assert_eq!(
                value(&tokens, "lineup_context_text"),
                Some("—\u{2800}\u{2800}")
            );
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
        assert_eq!(weekly_text(Some(0.0), Some(99), 100), "7d 00% 0m");
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
    fn pane_binding_accepts_exact_pane_without_native_session() {
        let mut r = row();
        r.occupant.as_mut().unwrap().native_session = None;
        r.usage.as_mut().unwrap().occupant = r.occupant.clone().unwrap();
        r.usage.as_mut().unwrap().binding =
            Some(crate::model::UsageBinding::Pane(r.pane_id.clone()));
        let t = pane_tokens(&r, LogoMode::Text, false, 0);
        assert_eq!(t["lineup_context_pct"], Some("7.2".into()));
        r.usage.as_mut().unwrap().binding =
            Some(crate::model::UsageBinding::Pane("different".into()));
        assert_eq!(
            pane_tokens(&r, LogoMode::Text, false, 0)["lineup_context_pct"],
            None
        );
    }
    #[test]
    fn native_binding_requires_exact_nonempty_native_session() {
        let mut r = row();
        r.usage.as_mut().unwrap().binding =
            Some(crate::model::UsageBinding::NativeSession("other".into()));
        assert_eq!(
            pane_tokens(&r, LogoMode::Text, false, 0)["lineup_context_pct"],
            None
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
        assert_eq!(
            value(&tokens, "lineup_context_text"),
            Some("—\u{2800}\u{2800}")
        );
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
    fn crowded_pane_preserves_visible_tokens_and_clears_optional_keys() {
        let r = row();
        let resource = resource("p1");
        let desired = pane_tokens(&r, LogoMode::Text, false, 0);
        let mut publisher = Publisher::default();
        let first = publisher
            .prepare(
                &resource,
                &snapshot(0),
                r.occupant.as_ref(),
                &desired,
                0,
                5000,
            )
            .unwrap()
            .unwrap();
        publisher.acknowledge(&first, 0).unwrap();
        let mut crowded = snapshot(28);
        crowded.stored_keys.extend(
            desired
                .iter()
                .filter(|(_, v)| v.is_some())
                .map(|(k, _)| k.clone()),
        );
        let fallback = publisher
            .prepare(
                &resource,
                &crowded,
                r.occupant.as_ref(),
                &desired,
                10000,
                5000,
            )
            .unwrap()
            .unwrap();
        for key in [
            "lineup_logo",
            "lineup_state_working",
            "lineup_context_text",
            "lineup_weekly_text",
        ] {
            assert_eq!(fallback.tokens.get(key), desired.get(key));
        }
        for key in ["lineup_agent", "lineup_state", "lineup_context_pct"] {
            assert_eq!(fallback.tokens.get(key), Some(&None));
        }
        assert!(fallback.tokens.keys().all(|k| k.starts_with("lineup_")));
        assert!(fallback.tokens.len() <= 16);
        publisher.acknowledge(&fallback, 10000).unwrap();
        let recovered = publisher
            .prepare(
                &resource,
                &snapshot(0),
                r.occupant.as_ref(),
                &desired,
                10001,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            recovered.tokens["lineup_context_pct"],
            desired["lineup_context_pct"]
        );
        assert_eq!(
            publisher.prepare(
                &resource,
                &snapshot(29),
                r.occupant.as_ref(),
                &desired,
                11000,
                5000
            ),
            Err(PatchError::ResourceCapacity)
        );
    }
    #[test]
    fn group_heading_full_contract_respects_report_and_resource_capacity() {
        let mut r = row();
        r.usage.as_mut().unwrap().weekly_remaining_pct = Some(67.0);
        let mut desired = pane_tokens(&r, LogoMode::Text, false, 0);
        desired.insert("lineup_group".into(), Some("example-repo".into()));
        desired.insert("lineup_group_ship_text".into(), Some("09:05".into()));
        // The full fourteen-key report includes clears for inactive state dots.
        let live = desired.values().filter(|v| v.is_some()).count();
        let resource = resource("p1");
        let report = Publisher::default()
            .prepare(
                &resource,
                &snapshot(32 - live),
                r.occupant.as_ref(),
                &desired,
                0,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(report.tokens.len(), 14);
        assert!(report.tokens.len() <= 16);
        assert!(crate::protocol::metadata_request("full-heading", &report).is_ok());
        let priority_names: BTreeSet<_> =
            PANE_CAPACITY_PRIORITY.iter().map(|(key, _)| *key).collect();
        assert_eq!(priority_names, PANE_KEYS.into_iter().collect());
        for unrelated in (32 - live)..=26 {
            let report = Publisher::default()
                .prepare(
                    &resource,
                    &snapshot(unrelated),
                    r.occupant.as_ref(),
                    &desired,
                    0,
                    5000,
                )
                .unwrap()
                .unwrap();
            let published = report
                .tokens
                .values()
                .filter(|value| value.is_some())
                .count();
            assert_eq!(unrelated + published, 32);
            for (key, required) in PANE_CAPACITY_PRIORITY {
                if required {
                    assert_eq!(report.tokens.get(key), desired.get(key));
                }
            }
        }
        assert_eq!(
            Publisher::default().prepare(
                &resource,
                &snapshot(27),
                r.occupant.as_ref(),
                &desired,
                0,
                5000
            ),
            Err(PatchError::ResourceCapacity)
        );
    }

    #[test]
    fn dirty_keys_and_ttl_renewal_use_successful_publication_time() {
        let resource = resource("p1");
        let row = row();
        let desired = pane_tokens(&row, LogoMode::Text, false, 0);
        let live = ResourceSnapshot {
            exists: true,
            stored_keys: desired
                .iter()
                .filter(|(_, value)| value.is_some())
                .map(|(key, _)| key.clone())
                .collect(),
        };
        let mut publisher = Publisher::default();
        let first = publisher
            .prepare(&resource, &live, row.occupant.as_ref(), &desired, 0, 5000)
            .unwrap()
            .unwrap();
        assert_eq!(first.tokens.len(), 14);
        assert_eq!(first.seq, 1);
        assert_eq!(first.ttl_ms, 15000);
        assert_eq!(first.source, SOURCE);
        publisher.acknowledge(&first, 1000).unwrap();
        assert!(
            publisher
                .prepare(
                    &resource,
                    &live,
                    row.occupant.as_ref(),
                    &desired,
                    5999,
                    5000
                )
                .unwrap()
                .is_none()
        );
        let renewal = publisher
            .prepare(
                &resource,
                &live,
                row.occupant.as_ref(),
                &desired,
                6000,
                5000,
            )
            .unwrap()
            .unwrap();
        assert!(renewal.tokens.values().all(Option::is_some));
        let retry = publisher
            .prepare(
                &resource,
                &live,
                row.occupant.as_ref(),
                &desired,
                6001,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(retry.tokens, renewal.tokens);
        assert!(retry.seq > renewal.seq);
        publisher.acknowledge(&retry, 6001).unwrap();
        assert_eq!(
            publisher.acknowledge(&renewal, 6002),
            Err(PatchError::UnreservedReport)
        );
        let mut changed = desired.clone();
        changed.insert("lineup_context_pct".into(), None);
        changed.insert("lineup_context_text".into(), Some("—".into()));
        let patch = publisher
            .prepare(
                &resource,
                &live,
                row.occupant.as_ref(),
                &changed,
                7000,
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
        assert_eq!(replacement.tokens.len(), 14);
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
        assert_eq!(clear.tokens.len(), 14);
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
        restarted
            .confirm_reservation(&resource("p1").endpoint_session, 32)
            .unwrap();
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
            first.seq + 2
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
    fn missing_resources_compact_without_reusing_sequence_after_restart() {
        let pane = resource("gone");
        let live = resource("live");
        let mut old = resource("old-endpoint");
        old.endpoint_session = "replaced/socket/session".into();
        let mut publisher = Publisher::restore(BTreeMap::from([
            (pane.clone(), 17),
            (live.clone(), 4),
            (old.clone(), 99),
        ]));
        assert!(
            publisher.prune_for_snapshot(&pane.endpoint_session, &BTreeSet::from([live.clone()]))
        );
        assert_eq!(publisher.checkpoint().len(), 1);
        assert!(!publisher.checkpoint().contains_key(&pane));
        assert!(!publisher.checkpoint().contains_key(&old));

        let mut restarted =
            Publisher::restore_with_floors(publisher.checkpoint(), publisher.checkpoint_floors());
        restarted
            .confirm_reservation(&pane.endpoint_session, 32)
            .unwrap();
        let report = restarted
            .prepare(
                &pane,
                &snapshot(0),
                None,
                &Tokens::from([("lineup_agent".into(), Some("codex".into()))]),
                0,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(report.seq, 18);
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
                .prepare(&id, &snapshot(29), row.occupant.as_ref(), &desired, 0, 5000)
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

    #[test]
    fn checked_overflow_fails_closed_in_reservation_and_allocation() {
        let mut publisher = Publisher::default();
        let endpoint = "test/endpoint";
        assert_eq!(
            publisher.plan_reservation(endpoint, u64::MAX).unwrap(),
            u64::MAX
        );
        publisher.confirm_reservation(endpoint, u64::MAX).unwrap();
        assert_eq!(
            publisher.plan_reservation(endpoint, 1).unwrap_err(),
            PatchError::SequenceExhausted
        );

        let mut max_publisher = Publisher::default();
        max_publisher
            .confirm_reservation(endpoint, u64::MAX)
            .unwrap();
        max_publisher.set_cursor_for_test(endpoint, u64::MAX);
        let id = ResourceId {
            endpoint_session: endpoint.into(),
            kind: ResourceKind::Pane,
            id: "p1".into(),
        };
        let desired = BTreeMap::from([("lineup_agent".into(), Some("codex".into()))]);
        assert_eq!(
            max_publisher
                .prepare(&id, &snapshot(0), None, &desired, 0, 5000)
                .unwrap_err(),
            PatchError::SequenceExhausted
        );
    }

    #[test]
    fn legacy_checkpoint_without_endpoint_rows_migrates_safely() {
        let id1 = resource("p1");
        let id2 = resource("p2");
        let legacy = BTreeMap::from([(id1.clone(), 10), (id2.clone(), 25)]);
        let mut publisher = Publisher::restore(legacy);
        assert_eq!(publisher.endpoint_window(&id1.endpoint_session), (25, 25));
        assert!(!publisher.has_available_reservation(&id1.endpoint_session));
        let next_bound = publisher
            .plan_reservation(&id1.endpoint_session, 32)
            .unwrap();
        assert_eq!(next_bound, 25 + 32);
        publisher
            .confirm_reservation(&id1.endpoint_session, next_bound)
            .unwrap();
        assert!(publisher.has_available_reservation(&id1.endpoint_session));

        let desired = BTreeMap::from([("lineup_agent".into(), Some("codex".into()))]);
        let rep1 = publisher
            .prepare(&id1, &snapshot(0), None, &desired, 0, 5000)
            .unwrap()
            .unwrap();
        assert_eq!(rep1.seq, 26);
        let rep2 = publisher
            .prepare(&id2, &snapshot(0), None, &desired, 0, 5000)
            .unwrap()
            .unwrap();
        assert_eq!(rep2.seq, 27);
    }

    #[test]
    fn empty_patch_and_capacity_skip_preserve_reservation_cursor() {
        let mut publisher = Publisher::default();
        let endpoint = "test/endpoint";
        publisher.confirm_reservation(endpoint, 10).unwrap();
        let id = ResourceId {
            endpoint_session: endpoint.into(),
            kind: ResourceKind::Pane,
            id: "p1".into(),
        };
        let desired = BTreeMap::from([("lineup_agent".into(), Some("codex".into()))]);
        let rep1 = publisher
            .prepare(&id, &snapshot(0), None, &desired, 0, 5000)
            .unwrap()
            .unwrap();
        assert_eq!(rep1.seq, 1);
        publisher.acknowledge(&rep1, 0).unwrap();

        let mut stored_snap = snapshot(0);
        stored_snap.stored_keys.insert("lineup_agent".into());
        assert!(
            publisher
                .prepare(&id, &stored_snap, None, &desired, 0, 5000)
                .unwrap()
                .is_none()
        );
        assert_eq!(publisher.endpoint_window(endpoint), (1, 10));

        let crowded = snapshot(33);
        assert!(
            publisher
                .prepare(&id, &crowded, None, &desired, 0, 5000)
                .is_err()
        );
        assert_eq!(publisher.endpoint_window(endpoint), (1, 10));
    }
}
