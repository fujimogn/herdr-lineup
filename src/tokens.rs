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
    /// In-memory cursor: the last sequence reserved for this resource.
    high_water: u64,
    accepted: Tokens,
    published_ms: BTreeMap<String, u64>,
    occupant: Option<Occupant>,
    reserved: Option<Report>,
}

/// Per-endpoint sequence bookkeeping. `floor` is the prune/restart floor: no
/// resource on the endpoint allocates at or below it. `bound` is the durable
/// upper bound U: every sequence ever reserved on the endpoint is `<= bound`,
/// and a raised bound must reach disk before any sequence above the previously
/// persisted bound is sent. `floor <= bound` always holds.
#[derive(Clone, Copy, Debug, Default)]
struct EndpointState {
    floor: u64,
    bound: u64,
}

/// Pure planner with windowed sequence reservation. `prepare()` advances an
/// in-memory per-resource cursor; when the cursor passes the endpoint's durable
/// upper bound it raises that bound by one window and sets `checkpoint_pending`.
/// The daemon must durably persist `checkpoint()` and `checkpoint_bounds()`
/// whenever `checkpoint_pending()` is true, BEFORE sending the report, then
/// call `checkpoint_persisted()`. If persistence fails, do not send anything:
/// the pending flag stays set so the next attempt persists first. Only call
/// `acknowledge()` after the server accepts the report.
/// Restoring a checkpoint discards the unused part of every window: all
/// resources on an endpoint continue strictly above its persisted bound, and
/// cached values are dropped so the next cycle republishes authoritatively.
#[derive(Clone, Debug)]
pub struct Publisher {
    resources: BTreeMap<ResourceId, ResourceState>,
    endpoints: BTreeMap<String, EndpointState>,
    window: u64,
    checkpoint_pending: bool,
}
impl Default for Publisher {
    fn default() -> Self {
        Self {
            resources: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            window: 1,
            checkpoint_pending: false,
        }
    }
}
impl Publisher {
    pub fn restore(checkpoint: BTreeMap<ResourceId, u64>) -> Self {
        Self::restore_with_bounds(checkpoint, BTreeMap::new())
    }

    /// `bounds` are the persisted `endpoint` rows. Older checkpoints persisted
    /// every reserved sequence per resource and wrote an endpoint row only after
    /// pruning, so the durable upper bound of an endpoint is the maximum of its
    /// row and its resource sequences. Every resource restarts above it.
    pub fn restore_with_bounds(
        checkpoint: BTreeMap<ResourceId, u64>,
        bounds: BTreeMap<String, u64>,
    ) -> Self {
        let mut endpoints: BTreeMap<String, EndpointState> = bounds
            .into_iter()
            .map(|(endpoint, bound)| {
                (
                    endpoint,
                    EndpointState {
                        floor: bound,
                        bound,
                    },
                )
            })
            .collect();
        for (resource, high_water) in &checkpoint {
            let state = endpoints
                .entry(resource.endpoint_session.clone())
                .or_default();
            state.bound = state.bound.max(*high_water);
            state.floor = state.bound;
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
            endpoints,
            ..Self::default()
        }
    }

    /// Per-resource cursors. They never exceed their endpoint bound.
    pub fn checkpoint(&self) -> BTreeMap<ResourceId, u64> {
        self.resources
            .iter()
            .map(|(resource, state)| (resource.clone(), state.high_water))
            .collect()
    }

    /// Durable upper bound per endpoint, written as the `endpoint` checkpoint
    /// row. It subsumes the prune floor, so one row per endpoint suffices.
    pub fn checkpoint_bounds(&self) -> BTreeMap<String, u64> {
        self.endpoints
            .iter()
            .filter_map(|(endpoint, state)| {
                let bound = state.bound.max(state.floor);
                (bound > 0).then(|| (endpoint.clone(), bound))
            })
            .collect()
    }

    /// True once a reservation left the persisted window. Stays true across
    /// failed persists and reconnects until `checkpoint_persisted()`.
    pub fn checkpoint_pending(&self) -> bool {
        self.checkpoint_pending
    }

    /// Call only after `checkpoint()`/`checkpoint_bounds()` reached disk.
    pub fn checkpoint_persisted(&mut self) {
        self.checkpoint_pending = false;
    }

    /// Size one reservation window from the authoritative snapshot: one
    /// sequence per reportable pane and workspace. A resource consumes at most
    /// one sequence per report attempt, so the busiest resource completes at
    /// least `reports` attempts per checkpoint write, and a restart discards at
    /// most that many unused sequences per endpoint. No new user setting is
    /// involved; zero resources never report, so the window stays at least 1.
    pub fn set_window(&mut self, reports: usize) {
        self.window = u64::try_from(reports).unwrap_or(u64::MAX).max(1);
    }

    /// The transport reconnected to the same endpoint. Drop cached values so
    /// the next cycle republishes authoritatively, but keep cursors, floors,
    /// bounds and the pending flag: every reservation, sent or not, stays
    /// consumed and the durable window is unchanged.
    pub fn reconnected(&mut self) {
        for state in self.resources.values_mut() {
            *state = ResourceState {
                high_water: state.high_water,
                ..ResourceState::default()
            };
        }
    }

    /// The authoritative snapshot permits removal of absent resource rows.
    /// Their maximum reserved sequence remains as the endpoint floor, so a
    /// resource ID reused by this same server cannot restart at seq 1. The
    /// floor never exceeds the durable bound, which already covers it on disk.
    /// A different endpoint has no access to the prior server's metadata.
    pub fn prune_for_snapshot(&mut self, endpoint: &str, live: &BTreeSet<ResourceId>) -> bool {
        let mut changed = false;
        let mut floor = self.endpoints.get(endpoint).map_or(0, |s| s.floor);
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
        self.endpoints.retain(|key, _| {
            let keep = key == endpoint;
            changed |= !keep;
            keep
        });
        if floor > 0 {
            let state = self.endpoints.entry(endpoint.into()).or_default();
            if state.floor != floor {
                state.floor = floor;
                state.bound = state.bound.max(floor);
                changed = true;
            }
        }
        changed
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
                    // Keep the clear in the patch so earlier optional values do
                    // not survive under pressure or after daemon restoration.
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
            return Ok(None);
        }
        // Every failure above returns before this point, so capacity skips and
        // empty patches consume neither a sequence nor window space.
        let endpoint = self
            .endpoints
            .entry(resource.endpoint_session.clone())
            .or_default();
        let seq = state
            .high_water
            .max(endpoint.floor)
            .checked_add(1)
            .ok_or(PatchError::SequenceExhausted)?;
        if seq > endpoint.bound {
            // Cursors are overflow-checked above, so a final window that is
            // shorter than requested near u64::MAX remains safe.
            endpoint.bound = seq
                .checked_add(self.window.saturating_sub(1))
                .unwrap_or(u64::MAX);
            self.checkpoint_pending = true;
        }
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
        // A restart starts every resource of the endpoint above its durable
        // bound, even one that never reported before.
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
            first.seq + 1
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

        assert_eq!(
            publisher.checkpoint_bounds(),
            BTreeMap::from([(pane.endpoint_session.clone(), 17)])
        );
        let mut restarted =
            Publisher::restore_with_bounds(publisher.checkpoint(), publisher.checkpoint_bounds());
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

    fn single(key: &str) -> Tokens {
        Tokens::from([(key.into(), Some("v".into()))])
    }
    fn try_reserve(
        publisher: &mut Publisher,
        resource: &ResourceId,
        now_ms: u64,
    ) -> Result<Option<Report>, PatchError> {
        let desired = single("lineup_agent");
        publisher.prepare(resource, &snapshot(0), None, &desired, now_ms, 5000)
    }
    fn reserve(publisher: &mut Publisher, id: &str, now_ms: u64) -> Report {
        try_reserve(publisher, &resource(id), now_ms)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn reservation_window_persists_once_per_window_and_restart_skips_unused_part() {
        let mut publisher = Publisher::default();
        publisher.set_window(3);
        assert!(!publisher.checkpoint_pending());
        assert!(publisher.checkpoint_bounds().is_empty());
        let first = reserve(&mut publisher, "p1", 0);
        assert_eq!(first.seq, 1);
        assert!(publisher.checkpoint_pending());
        let endpoint = resource("p1").endpoint_session;
        assert_eq!(
            publisher.checkpoint_bounds(),
            BTreeMap::from([(endpoint.clone(), 3)])
        );
        publisher.checkpoint_persisted();
        // Unacknowledged (failed or ambiguous) sends still consume the cursor.
        for expected in [2, 3] {
            assert_eq!(reserve(&mut publisher, "p1", 0).seq, expected);
            assert!(!publisher.checkpoint_pending());
        }
        // Another resource starts at 1 and shares the endpoint window.
        for expected in [1, 2, 3] {
            assert_eq!(reserve(&mut publisher, "p2", 0).seq, expected);
            assert!(!publisher.checkpoint_pending());
        }
        let fourth = reserve(&mut publisher, "p1", 0);
        assert_eq!(fourth.seq, 4);
        assert!(publisher.checkpoint_pending());
        assert_eq!(
            publisher.checkpoint_bounds(),
            BTreeMap::from([(endpoint.clone(), 6)])
        );
        publisher.checkpoint_persisted();
        // The persisted rows carry the cursors; the bound covers every sent seq.
        assert_eq!(publisher.checkpoint()[&resource("p1")], 4);
        assert_eq!(publisher.checkpoint()[&resource("p2")], 3);

        let mut restarted =
            Publisher::restore_with_bounds(publisher.checkpoint(), publisher.checkpoint_bounds());
        assert!(!restarted.checkpoint_pending());
        assert_eq!(restarted.checkpoint_bounds()[&endpoint], 6);
        // Unused seqs 5..=6 are discarded; every resource, including a new one,
        // continues above the bound and the first allocation opens a window.
        restarted.set_window(2);
        assert_eq!(reserve(&mut restarted, "p2", 0).seq, 7);
        assert!(restarted.checkpoint_pending());
        assert_eq!(restarted.checkpoint_bounds()[&endpoint], 8);
        restarted.checkpoint_persisted();
        assert_eq!(reserve(&mut restarted, "p1", 0).seq, 7);
        assert_eq!(reserve(&mut restarted, "p3", 0).seq, 7);
        assert!(!restarted.checkpoint_pending());
        let mut other = resource("p1");
        other.endpoint_session = "new/socket/session".into();
        let moved = try_reserve(&mut restarted, &other, 0).unwrap().unwrap();
        assert_eq!(moved.seq, 1);
        assert_eq!(restarted.checkpoint_bounds()[&other.endpoint_session], 2);
    }

    #[test]
    fn older_checkpoints_migrate_to_the_maximum_durable_sequence() {
        let endpoint = resource("p1").endpoint_session;
        // Pre-window checkpoints persisted every resource seq and wrote an
        // endpoint floor only after pruning; the floor may lag the resources.
        let mut migrated = Publisher::restore_with_bounds(
            BTreeMap::from([(resource("p1"), 40), (resource("p2"), 12)]),
            BTreeMap::from([(endpoint.clone(), 5)]),
        );
        assert_eq!(
            migrated.checkpoint_bounds(),
            BTreeMap::from([(endpoint.clone(), 40)])
        );
        migrated.set_window(4);
        assert_eq!(reserve(&mut migrated, "p2", 0).seq, 41);
        assert_eq!(reserve(&mut migrated, "p1", 0).seq, 41);
        assert_eq!(migrated.checkpoint_bounds()[&endpoint], 44);
        // Checkpoints without any endpoint row behave the same way.
        let mut legacy = Publisher::restore(BTreeMap::from([(resource("p1"), 40)]));
        assert_eq!(legacy.checkpoint_bounds()[&endpoint], 40);
        assert_eq!(reserve(&mut legacy, "p1", 0).seq, 41);
    }

    #[test]
    fn window_extension_saturates_and_cursor_overflow_is_checked() {
        let endpoint = resource("p1").endpoint_session;
        let mut publisher = Publisher::restore(BTreeMap::from([(resource("p1"), u64::MAX - 1)]));
        publisher.set_window(usize::MAX);
        assert_eq!(reserve(&mut publisher, "p1", 0).seq, u64::MAX);
        assert_eq!(publisher.checkpoint_bounds()[&endpoint], u64::MAX);
        assert!(publisher.checkpoint_pending());
        publisher.checkpoint_persisted();
        assert_eq!(
            try_reserve(&mut publisher, &resource("p1"), 0).unwrap_err(),
            PatchError::SequenceExhausted
        );
        // The floor lifts a fresh resource to the last seq of the saturated
        // window; after that nothing remains on this endpoint.
        assert_eq!(reserve(&mut publisher, "p2", 0).seq, u64::MAX);
        assert!(!publisher.checkpoint_pending());
        assert_eq!(
            try_reserve(&mut publisher, &resource("p2"), 0).unwrap_err(),
            PatchError::SequenceExhausted
        );
        let bounds = BTreeMap::from([(endpoint, u64::MAX)]);
        let mut bounded = Publisher::restore_with_bounds(BTreeMap::new(), bounds);
        assert_eq!(
            try_reserve(&mut bounded, &resource("p9"), 0).unwrap_err(),
            PatchError::SequenceExhausted
        );
    }

    #[test]
    fn capacity_skips_and_empty_patches_leave_window_untouched() {
        let mut publisher = Publisher::default();
        publisher.set_window(4);
        let id = resource("p1");
        let mut row = row();
        row.usage.as_mut().unwrap().weekly_remaining_pct = Some(67.0);
        let desired = pane_tokens(&row, LogoMode::Text, false, 0);
        assert_eq!(
            publisher.prepare(&id, &snapshot(29), row.occupant.as_ref(), &desired, 0, 5000),
            Err(PatchError::ResourceCapacity)
        );
        assert!(!publisher.checkpoint_pending());
        assert!(publisher.checkpoint_bounds().is_empty());
        assert!(publisher.checkpoint().is_empty());
        let report = reserve(&mut publisher, "p1", 0);
        publisher.checkpoint_persisted();
        publisher.acknowledge(&report, 0).unwrap();
        // Unchanged live values before renewal produce no report and no reservation.
        let stored = ResourceSnapshot {
            exists: true,
            stored_keys: BTreeSet::from(["lineup_agent".to_owned()]),
        };
        assert!(
            publisher
                .prepare(&id, &stored, None, &single("lineup_agent"), 1, 5000)
                .unwrap()
                .is_none()
        );
        assert_eq!(publisher.checkpoint()[&id], 1);
        assert!(!publisher.checkpoint_pending());
        // A missing resource never reserves either.
        assert!(
            publisher
                .prepare(
                    &id,
                    &ResourceSnapshot::default(),
                    None,
                    &single("lineup_agent"),
                    1,
                    5000
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(publisher.checkpoint()[&id], 1);
        assert_eq!(publisher.checkpoint_bounds()[&id.endpoint_session], 4);
    }

    #[test]
    fn reconnect_keeps_cursors_window_and_pending_flag_but_republishes() {
        let mut publisher = Publisher::default();
        publisher.set_window(5);
        let id = resource("p1");
        let first = reserve(&mut publisher, "p1", 0);
        publisher.checkpoint_persisted();
        publisher.acknowledge(&first, 0).unwrap();
        let lost = reserve(&mut publisher, "p1", 6000);
        assert_eq!(lost.seq, 2);
        // Response lost: the daemon reconnects without acknowledging.
        publisher.reconnected();
        assert_eq!(
            publisher.acknowledge(&lost, 6001),
            Err(PatchError::UnreservedReport)
        );
        let republished = publisher
            .prepare(&id, &snapshot(0), None, &single("lineup_agent"), 6001, 5000)
            .unwrap()
            .unwrap();
        assert_eq!(republished.seq, 3);
        assert!(!publisher.checkpoint_pending());
        assert_eq!(publisher.checkpoint_bounds()[&id.endpoint_session], 5);
        // A pending window survives the reconnect until the daemon persists it.
        let mut pending = Publisher::default();
        reserve(&mut pending, "p1", 0);
        pending.reconnected();
        assert!(pending.checkpoint_pending());
        assert_eq!(reserve(&mut pending, "p1", 0).seq, 2);
        assert!(pending.checkpoint_pending());
    }

    #[test]
    fn pruned_resource_inside_an_unused_window_keeps_floor_below_bound() {
        let mut publisher = Publisher::default();
        publisher.set_window(8);
        let gone = resource("gone");
        let live = resource("live");
        for _ in 0..5 {
            reserve(&mut publisher, "gone", 0);
        }
        reserve(&mut publisher, "live", 0);
        publisher.checkpoint_persisted();
        assert!(
            publisher.prune_for_snapshot(&gone.endpoint_session, &BTreeSet::from([live.clone()]))
        );
        assert!(!publisher.checkpoint().contains_key(&gone));
        // The bound (8) already covers the pruned cursor (5) on disk.
        assert_eq!(publisher.checkpoint_bounds()[&gone.endpoint_session], 8);
        assert_eq!(reserve(&mut publisher, "gone", 0).seq, 6);
        assert_eq!(reserve(&mut publisher, "live", 0).seq, 2);
        assert!(!publisher.checkpoint_pending());
        let mut restarted =
            Publisher::restore_with_bounds(publisher.checkpoint(), publisher.checkpoint_bounds());
        assert_eq!(reserve(&mut restarted, "gone", 0).seq, 9);
        assert_eq!(reserve(&mut restarted, "live", 0).seq, 9);
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
}
