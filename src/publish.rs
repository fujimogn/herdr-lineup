//! Durable, session-scoped reconciliation from snapshots to owned metadata.

use crate::{
    config::Config,
    daemon,
    model::{self, UsageReader},
    protocol,
    reader_agy::AgyReader,
    reader_claude::ClaudeReader,
    reader_codex::CodexReader,
    reader_grok::GrokReader,
    ship_time,
    socket::Transport,
    tokens::{self, Agent, ResourceId, ResourceKind},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub(crate) struct CapacitySkip {
    pub skipped_resources: u64,
}

impl std::fmt::Display for CapacitySkip {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "metadata capacity exhausted; affected resources skipped"
        )
    }
}

impl std::error::Error for CapacitySkip {}

pub(crate) fn capacity_skip_count(error: &io::Error) -> Option<u64> {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<CapacitySkip>())
        .map(|skip| skip.skipped_resources)
}

#[derive(Clone, Default)]
pub struct Homes {
    pub codex: Option<PathBuf>,
    pub grok: Option<PathBuf>,
    pub statusline_state: Option<PathBuf>,
}
impl Homes {
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        let provider = |key, default| match std::env::var_os(key) {
            Some(path) => Some(PathBuf::from(path)).filter(|p| p.is_absolute()),
            None => home.as_ref().map(|home| home.join(default)),
        };
        Self {
            codex: provider("CODEX_HOME", ".codex"),
            grok: provider("GROK_HOME", ".grok"),
            statusline_state: std::env::var_os("HERDR_PLUGIN_STATE_DIR")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
        }
    }
}

fn grok_sample(outcome: crate::quota_grok::Outcome) -> Option<model::WeeklySample> {
    match outcome {
        crate::quota_grok::Outcome::Weekly(weekly) => Some(model::WeeklySample {
            used_percent: weekly.used_percent,
            resets_at: weekly.resets_at,
        }),
        _ => None,
    }
}

fn fresh_grok_weekly(
    sample: Option<model::WeeklySample>,
    observed: Option<Instant>,
    now: Instant,
    max_age_ms: u64,
    refresh_interval_ms: u64,
) -> Option<model::WeeklySample> {
    let settings = crate::quota_grok::Settings::default();
    let query_cycle =
        settings.interval() + settings.timeout() + Duration::from_millis(refresh_interval_ms);
    let max_age = Duration::from_millis(max_age_ms).max(query_cycle);
    sample.filter(|value| {
        value.resets_at.is_some()
            || observed.is_some_and(|observed| now.saturating_duration_since(observed) <= max_age)
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointRow {
    endpoint: String,
    kind: String,
    id: String,
    seq: u64,
}

struct HeadingCache {
    cwd: PathBuf,
    occupants: Vec<(String, Option<tokens::Occupant>)>,
    repository: ship_time::Repository,
}

pub struct MetadataPublisher {
    planner: tokens::Publisher,
    checkpoint_loaded: bool,
    directory: PathBuf,
    socket_key: String,
    endpoint: Option<String>,
    homes: Homes,
    started: Instant,
    request_seq: u64,
    refresh_ms: u64,
    shutdown: Option<Arc<AtomicBool>>,
    codex_weekly: Option<model::WeeklyWorker>,
    grok_weekly: Option<model::WeeklyWorker>,
    weekly_diagnostics_initialized: bool,
    grok_observed: Arc<std::sync::Mutex<Option<Instant>>>,
    codex_control: Arc<crate::quota_codex::QueryControl>,
    headings: BTreeMap<String, HeadingCache>,
    local_readings: BTreeMap<String, model::UsageReading>,
    /// Successful `sequences.json` writes. Tests assert one write per report.
    #[cfg(test)]
    checkpoint_persists: std::sync::atomic::AtomicU64,
}
impl MetadataPublisher {
    /// Construction performs no state I/O. The daemon acquires its session lock
    /// before refresh/clear reads the latest checkpoint or mutates any sequence.
    pub fn new(directory: PathBuf, socket_key: String, homes: Homes) -> io::Result<Self> {
        if !directory.is_absolute() || socket_key.is_empty() {
            return Err(io::Error::other(
                "absolute state directory and socket key required",
            ));
        }
        Ok(Self {
            planner: tokens::Publisher::default(),
            checkpoint_loaded: false,
            directory,
            socket_key,
            endpoint: None,
            homes,
            started: Instant::now(),
            request_seq: 0,
            refresh_ms: 5000,
            shutdown: None,
            codex_weekly: None,
            grok_weekly: None,
            weekly_diagnostics_initialized: false,
            grok_observed: Arc::new(std::sync::Mutex::new(None)),
            codex_control: Arc::new(crate::quota_codex::QueryControl::default()),
            headings: BTreeMap::new(),
            local_readings: BTreeMap::new(),
            #[cfg(test)]
            checkpoint_persists: std::sync::atomic::AtomicU64::new(0),
        })
    }

    fn heading_repository(
        &mut self,
        workspace: &str,
        cwd: Option<&Path>,
        occupants: &[(String, Option<tokens::Occupant>)],
        reading: ship_time::RepositoryRead,
    ) -> Option<ship_time::Repository> {
        let Some(cwd) = cwd else {
            self.headings.remove(workspace);
            return None;
        };
        if self
            .headings
            .get(workspace)
            .is_some_and(|previous| previous.cwd != cwd || previous.occupants != occupants)
        {
            self.headings.remove(workspace);
        }
        match reading {
            ship_time::RepositoryRead::Interrupted => self
                .headings
                .get(workspace)
                .map(|previous| previous.repository.clone()),
            ship_time::RepositoryRead::Complete(Some(repository)) => {
                self.headings.insert(
                    workspace.into(),
                    HeadingCache {
                        cwd: cwd.to_owned(),
                        occupants: occupants.to_vec(),
                        repository: repository.clone(),
                    },
                );
                Some(repository)
            }
            ship_time::RepositoryRead::Complete(None) => {
                self.headings.remove(workspace);
                None
            }
        }
    }

    fn start_weekly(&mut self, config: &Config) -> io::Result<()> {
        use crate::doctor::{WeeklyProvider, WeeklyResult, write_weekly_result};
        // Only the daemon holding the session lock receives a shutdown flag.
        let Some(shutdown) = self.shutdown.clone() else {
            return Ok(());
        };
        self.check_shutdown()?;
        if !self.weekly_diagnostics_initialized {
            for (provider, name) in [
                (WeeklyProvider::Codex, "codex"),
                (WeeklyProvider::Grok, "grok"),
            ] {
                let result = if config.allow_network && config.providers.iter().any(|id| id == name)
                {
                    WeeklyResult::Pending
                } else {
                    WeeklyResult::Disabled
                };
                write_weekly_result(&self.directory, provider, result)?;
            }
            self.weekly_diagnostics_initialized = true;
        }
        if !config.allow_network {
            return Ok(());
        }
        if self.codex_weekly.is_none() && config.providers.iter().any(|id| id == "codex") {
            if let Some(home) = self.homes.codex.clone() {
                let mut client = crate::quota_codex::WeeklyClient::new(
                    home,
                    crate::quota_codex::DEFAULT_REFRESH_INTERVAL,
                    crate::quota_codex::DEFAULT_TIMEOUT,
                );
                let interval = client.interval();
                let control = self.codex_control.clone();
                let directory = self.directory.clone();
                self.codex_weekly = Some(model::WeeklyWorker::start(
                    shutdown.clone(),
                    interval,
                    move || {
                        let sample =
                            client
                                .refresh_controlled(&control)
                                .map(|weekly| model::WeeklySample {
                                    used_percent: weekly.used_percent,
                                    resets_at: weekly.resets_at,
                                });
                        // Saving a diagnostic must not replace or block a quota result.
                        let _ = write_weekly_result(
                            &directory,
                            WeeklyProvider::Codex,
                            client.last_result(),
                        );
                        sample
                    },
                )?);
            } else {
                write_weekly_result(
                    &self.directory,
                    WeeklyProvider::Codex,
                    WeeklyResult::AuthMissing,
                )?;
            }
        }
        if self.grok_weekly.is_none() && config.providers.iter().any(|id| id == "grok") {
            let settings = crate::quota_grok::Settings::default();
            // Preserve resolution failures as fixed categories, not silent None.
            let mut client = crate::quota_grok::Client::from_env(settings);
            let directory = self.directory.clone();
            let observed = self.grok_observed.clone();
            self.grok_weekly = Some(model::WeeklyWorker::start(
                shutdown,
                settings.interval(),
                move || {
                    let (sample, result) = match client.as_mut() {
                        Ok(client) => {
                            let outcome = client.poll();
                            if let Ok(mut observed) = observed.lock() {
                                *observed = client.last_success();
                            }
                            (grok_sample(outcome), client.last_result())
                        }
                        Err(failure) => (None, failure.diagnostic()),
                    };
                    let _ = write_weekly_result(&directory, WeeklyProvider::Grok, result);
                    sample
                },
            )?);
        }
        Ok(())
    }

    fn check_shutdown(&self) -> io::Result<()> {
        if self
            .shutdown
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "publication interrupted by shutdown",
            ));
        }
        Ok(())
    }

    fn load_checkpoint(&mut self) -> io::Result<()> {
        if self.checkpoint_loaded {
            return Ok(());
        }
        let checkpoint = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.directory.join("sequences.json"))
        {
            Ok(file) => {
                let rows: Vec<CheckpointRow> = serde_json::from_reader(file)
                    .map_err(|_| io::Error::other("invalid sequence checkpoint"))?;
                let mut checkpoint = BTreeMap::new();
                let mut floors = BTreeMap::new();
                for row in rows {
                    if row.kind == "endpoint" {
                        if !row.id.is_empty() || floors.insert(row.endpoint, row.seq).is_some() {
                            return Err(io::Error::other("invalid checkpoint endpoint floor"));
                        }
                        continue;
                    }
                    let kind = match row.kind.as_str() {
                        "pane" => ResourceKind::Pane,
                        "workspace" => ResourceKind::Workspace,
                        _ => return Err(io::Error::other("invalid checkpoint resource kind")),
                    };
                    let resource = ResourceId {
                        endpoint_session: row.endpoint,
                        kind,
                        id: row.id,
                    };
                    if checkpoint.insert(resource, row.seq).is_some() {
                        return Err(io::Error::other("duplicate checkpoint resource"));
                    }
                }
                (checkpoint, floors)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                (BTreeMap::new(), BTreeMap::new())
            }
            Err(_) => return Err(io::Error::other("cannot read sequence checkpoint")),
        };
        self.planner = tokens::Publisher::restore_with_floors(checkpoint.0, checkpoint.1);
        self.checkpoint_loaded = true;
        Ok(())
    }

    fn persist(&self) -> io::Result<()> {
        let mut rows: Vec<_> = self
            .planner
            .checkpoint()
            .into_iter()
            .map(|(resource, seq)| CheckpointRow {
                endpoint: resource.endpoint_session,
                kind: match resource.kind {
                    ResourceKind::Pane => "pane",
                    ResourceKind::Workspace => "workspace",
                }
                .into(),
                id: resource.id,
                seq,
            })
            .collect();
        rows.extend(
            self.planner
                .checkpoint_floors()
                .into_iter()
                .map(|(endpoint, seq)| CheckpointRow {
                    endpoint,
                    kind: "endpoint".into(),
                    id: String::new(),
                    seq,
                }),
        );
        let mut file = tempfile::NamedTempFile::new_in(&self.directory)?;
        serde_json::to_writer(file.as_file_mut(), &rows)?;
        file.flush()?;
        file.as_file().sync_all()?;
        file.persist(self.directory.join("sequences.json"))
            .map_err(|_| io::Error::other("cannot persist sequence checkpoint"))?;
        File::open(&self.directory)?.sync_all()?;
        #[cfg(test)]
        {
            let _ = self.checkpoint_persists.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    fn request(&mut self, transport: &mut Transport, method: &str) -> io::Result<(String, Value)> {
        self.request_seq = self
            .request_seq
            .checked_add(1)
            .ok_or_else(|| io::Error::other("request sequence exhausted"))?;
        let id = format!("lineup-{}", self.request_seq);
        let response = exchange(transport, &json!({"id":id,"method":method,"params":{}}))?;
        Ok((id, response))
    }

    fn snapshot(&mut self, transport: &mut Transport) -> io::Result<protocol::Snapshot> {
        self.load_checkpoint()?;
        let identity = transport.identity();
        let endpoint = format!("{}:{}:{}", self.socket_key, identity.device, identity.inode);
        if self
            .endpoint
            .as_ref()
            .is_some_and(|previous| previous != &endpoint)
        {
            return Err(io::Error::other("publisher endpoint changed"));
        }
        self.endpoint = Some(endpoint);
        let (id, response) = self.request(transport, "session.snapshot")?;
        protocol::parse_snapshot(&id, &response)
            .map_err(|_| io::Error::other("invalid snapshot response"))
    }

    fn resource(&self, kind: ResourceKind, id: &str) -> ResourceId {
        ResourceId {
            endpoint_session: self.endpoint.clone().expect("snapshot binds endpoint"),
            kind,
            id: id.into(),
        }
    }

    fn report(
        &mut self,
        transport: &mut Transport,
        resource: &ResourceId,
        keys: &BTreeSet<String>,
        occupant: Option<&tokens::Occupant>,
        desired: &tokens::Tokens,
        deadline: Option<Instant>,
    ) -> io::Result<()> {
        if deadline.is_none() {
            self.check_shutdown()?;
        }
        let now_ms = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let snapshot = tokens::ResourceSnapshot {
            exists: true,
            stored_keys: keys.clone(),
        };
        let Some(report) = self
            .planner
            .prepare(
                resource,
                &snapshot,
                occupant,
                desired,
                now_ms,
                self.refresh_ms,
            )
            .map_err(|error| match error {
                tokens::PatchError::ResourceCapacity | tokens::PatchError::ReportCapacity => {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        CapacitySkip {
                            skipped_resources: 1,
                        },
                    )
                }
                _ => io::Error::other("metadata planning failed"),
            })?
        else {
            return Ok(());
        };
        // Reserve durably BEFORE the first byte can reach Herdr. Even an ambiguous
        // response burns this seq; retry/restart must use a strictly newer one.
        self.persist()?;
        self.request_seq = self
            .request_seq
            .checked_add(1)
            .ok_or_else(|| io::Error::other("request sequence exhausted"))?;
        let id = format!("lineup-{}", self.request_seq);
        let request = protocol::metadata_request(&id, &report)
            .map_err(|_| io::Error::other("invalid metadata request"))?;
        // Persistence may consume the budget checked by clear's caller. Recheck
        // at the last possible point before starting the bounded socket exchange.
        if let Some(deadline) = deadline {
            check_clear_deadline(deadline)?;
        } else {
            // Clear must still run after shutdown. Refresh finishes a complete
            // in-flight exchange but does not begin a new metadata frame.
            self.check_shutdown()?;
        }
        let response = exchange(transport, &request)?;
        let result = protocol::response_result(&id, &response)
            .map_err(|_| io::Error::other("metadata report rejected"))?;
        if result.get("type").and_then(Value::as_str) != Some("ok") {
            return Err(io::Error::other("invalid metadata acknowledgement"));
        }
        self.planner
            .acknowledge(&report, now_ms)
            .map_err(|_| io::Error::other("invalid metadata reservation"))?;
        // `acknowledge` records acceptance only. `checkpoint()` is the high-water
        // sequence already saved before the send, so another persist would
        // rewrite the same bytes.
        Ok(())
    }

    fn reconcile_missing(&mut self, snapshot: &protocol::Snapshot) -> io::Result<()> {
        self.headings.retain(|id, _| {
            snapshot
                .workspaces
                .iter()
                .any(|workspace| workspace.workspace_id == *id)
        });
        let live: BTreeSet<_> =
            snapshot
                .panes
                .iter()
                .map(|pane| self.resource(ResourceKind::Pane, &pane.pane_id))
                .chain(snapshot.workspaces.iter().map(|workspace| {
                    self.resource(ResourceKind::Workspace, &workspace.workspace_id)
                }))
                .collect();
        let previous = self.planner.clone();
        if self.planner.prune_for_snapshot(
            self.endpoint.as_deref().expect("snapshot binds endpoint"),
            &live,
        ) && let Err(error) = self.persist()
        {
            self.planner = previous;
            return Err(error);
        }
        Ok(())
    }

    fn refresh_with_readers(
        &mut self,
        transport: &mut Transport,
        config: &Config,
        codex: &dyn UsageReader,
        grok: &dyn UsageReader,
    ) -> io::Result<()> {
        self.check_shutdown()?;
        config.validate()?;
        self.refresh_ms = config.refresh_interval_ms;
        let snapshot = self.snapshot(transport)?;
        self.check_shutdown()?;
        self.reconcile_missing(&snapshot)?;
        // Keep cached association only while the authoritative pane identity matches.
        self.local_readings.retain(|id, reading| {
            snapshot.panes.iter().any(|pane| {
                pane.pane_id == *id
                    && pane.occupant.as_ref().is_some_and(|who| {
                        config
                            .providers
                            .iter()
                            .any(|provider| provider == agent_id(who.agent))
                            && matches!(
                                (reading.identity.provider, who.agent),
                                (model::Provider::Claude, Agent::Claude)
                                    | (model::Provider::Codex, Agent::Codex)
                                    | (model::Provider::Grok, Agent::Grok)
                                    | (model::Provider::Agy, Agent::Agy)
                            )
                            && reading.identity.native_session_id
                                == who.native_session.clone().unwrap_or_default()
                            && reading.identity.repository_identity
                                == pane.cwd.clone().unwrap_or_default()
                    })
            })
        });
        let now = SystemTime::now();
        let now_unix = unix(now);
        // One nonblocking cache snapshot per cycle keeps all Grok panes on the
        // same account observation even if a worker completes mid-refresh.
        let codex_weekly = config
            .allow_network
            .then(|| {
                self.codex_weekly
                    .as_ref()
                    .and_then(|worker| worker.sample(now))
            })
            .flatten();
        let grok_weekly = config
            .allow_network
            .then(|| {
                self.grok_weekly
                    .as_ref()
                    .and_then(|worker| worker.sample(now))
            })
            .flatten();
        let grok_weekly = fresh_grok_weekly(
            grok_weekly,
            self.grok_observed.try_lock().ok().and_then(|value| *value),
            Instant::now(),
            config.usage_max_age_ms,
            config.refresh_interval_ms,
        );
        let mode = match config.logo_mode.as_str() {
            "font" => tokens::LogoMode::Font,
            "text" => tokens::LogoMode::Text,
            _ => tokens::LogoMode::Auto,
        };
        let mut skipped_resources = 0u64;
        let local_now = chrono::Local::now();
        let mut headings = BTreeMap::new();
        for workspace in &snapshot.workspaces {
            self.check_shutdown()?;
            let panes: Vec<_> = snapshot
                .panes
                .iter()
                .filter(|pane| pane.workspace_id == workspace.workspace_id)
                .map(|pane| (pane.pane_id.as_str(), pane.cwd.as_deref().map(Path::new)))
                .collect();
            let cwd = ship_time::resolve_workspace_cwd(
                workspace.checkout_path.as_deref().map(Path::new),
                &panes,
            );
            let reading = cwd.map_or(ship_time::RepositoryRead::Complete(None), |cwd| {
                ship_time::read_repository_controlled(cwd, self.shutdown.as_deref())
            });
            let occupants = snapshot
                .panes
                .iter()
                .filter(|pane| pane.workspace_id == workspace.workspace_id)
                .map(|pane| (pane.pane_id.clone(), pane.occupant.clone()))
                .collect::<Vec<_>>();
            let repository =
                self.heading_repository(&workspace.workspace_id, cwd, &occupants, reading);
            let ship = repository.as_ref().and_then(|repo| repo.ship_unix);
            let resolved = repository.is_some();
            let desired = BTreeMap::from([
                (
                    "lineup_repo".into(),
                    Some(
                        repository
                            .map(|repo| repo.name)
                            .unwrap_or_else(|| "—".into()),
                    ),
                ),
                ("lineup_ship_unix".into(), ship.map(|unix| unix.to_string())),
                (
                    "lineup_ship_text".into(),
                    Some(ship_time::format_ship_time(ship, &local_now)),
                ),
            ]);
            headings.insert(workspace.workspace_id.as_str(), (resolved, desired));
        }
        // A single budget for the cycle prevents N panes multiplying the reader wait.
        let deadline = Instant::now() + Duration::from_millis(config.reader_timeout_ms);
        let first_panes = first_pane_ids(&snapshot);
        for pane in &snapshot.panes {
            self.check_shutdown()?;
            let first_pane = first_panes.contains(pane.pane_id.as_str());
            let occupant = pane.occupant.clone().filter(|who| {
                config
                    .providers
                    .iter()
                    .any(|provider| provider == agent_id(who.agent))
            });
            let usage = occupant.as_ref().and_then(|who| {
                let agy = AgyReader {
                    pane_id: pane.pane_id.clone(),
                };
                let native = match who.agent {
                    Agent::Agy | Agent::Grok => who.native_session.clone().unwrap_or_default(),
                    _ => who.native_session.clone()?,
                };
                let (provider, home, reader): (_, _, &dyn UsageReader) = match who.agent {
                    Agent::Claude => (
                        model::Provider::Claude,
                        self.homes.statusline_state.as_ref()?,
                        &ClaudeReader,
                    ),
                    Agent::Agy => (
                        model::Provider::Agy,
                        self.homes.statusline_state.as_ref()?,
                        &agy,
                    ),
                    Agent::Codex => (model::Provider::Codex, self.homes.codex.as_ref()?, codex),
                    Agent::Grok => (
                        model::Provider::Grok,
                        self.homes.grok.as_ref().unwrap_or(&self.directory),
                        grok,
                    ),
                    _ => return None,
                };
                // Snapshot has no pane-specific home/profile. Only the injected daemon
                // home is searched, with an exact native id; never infer by cwd or use
                // another session when a different pane profile is inaccessible.
                let identity = model::SessionIdentity {
                    provider,
                    native_session_id: native.clone(),
                    repository_identity: pane.cwd.clone().unwrap_or_default(),
                };
                // Repository discovery and earlier panes may take seconds. A
                // newly written observation must be compared to this read's clock.
                let now = SystemTime::now();
                let request = model::ReadRequest {
                    identity: &identity,
                    home,
                    now,
                    max_age: Duration::from_millis(config.usage_max_age_ms),
                    timeout: deadline.saturating_duration_since(Instant::now()),
                    shutdown: self.shutdown.as_deref(),
                };
                let mut reading = if who.agent == Agent::Grok && self.homes.grok.is_none() {
                    // Billing account attribution does not require a local session
                    // home. Do not search plugin state as a substitute for context.
                    model::UsageReading::missing(
                        &request,
                        model::SourceKind::GrokSignals,
                        model::MissingReason::NotFound,
                    )
                } else {
                    reader.read(&request)
                };
                if reading.identity != identity {
                    self.local_readings.remove(&pane.pane_id);
                    return None;
                }
                // A shared cycle budget can expire before this pane gets a turn.
                // Do not turn a transient read timeout into loss of proven identity.
                // EntryLimit is not transient: the directory stays over the cap, so
                // the previous context must be dropped instead of kept without expiry.
                // Cache local observations only; account workers retain their own
                // reset/auth policy and are overlaid afresh below.
                if reading.context
                    == model::UsageOutcome::Unavailable(model::MissingReason::Timeout)
                    && matches!(
                        reading.weekly_remaining,
                        model::UsageOutcome::Unavailable(_)
                    )
                    && let Some(previous) = self.local_readings.get(&pane.pane_id)
                    && previous.identity == identity
                    && previous.source_kind == reading.source_kind
                {
                    reading = previous.clone();
                    let weekly_valid = reading.weekly_resets_at.map_or_else(
                        || {
                            reading.observed_at.is_some_and(|observed| {
                                now.duration_since(observed)
                                    .is_ok_and(|age| age <= request.max_age)
                            })
                        },
                        |reset| reset > now,
                    );
                    if !weekly_valid {
                        reading.weekly_remaining =
                            model::UsageOutcome::Unavailable(model::MissingReason::Stale);
                        reading.weekly_window = None;
                        reading.weekly_resets_at = None;
                    }
                }
                if reading.binding.is_some() {
                    self.local_readings
                        .insert(pane.pane_id.clone(), reading.clone());
                } else {
                    self.local_readings.remove(&pane.pane_id);
                }
                if config.allow_network {
                    let weekly = match who.agent {
                        // Herdr snapshots do not expose pane-specific CODEX_HOME or
                        // profiles. Attribute the daemon home's quota only after its
                        // rollout reader proves this exact native session belongs
                        // there. Different homes remain unavailable; profiles sharing
                        // a home cannot be distinguished, and auth changes require
                        // restarting the daemon to discard the account cache.
                        Agent::Codex
                            if reading.binding
                                == Some(model::UsageBinding::NativeSession(native.clone())) =>
                        {
                            codex_weekly
                        }
                        // Grok has one configured account, shared across all its
                        // panes even when no local context/native id is available.
                        Agent::Grok => grok_weekly,
                        _ => None,
                    };
                    if let Some(weekly) = weekly {
                        reading.weekly_remaining =
                            model::UsageOutcome::Available(100.0 - weekly.used_percent);
                        reading.weekly_window = Some(model::WeeklyWindow::RollingSevenDays);
                        reading.weekly_resets_at = weekly.resets_at;
                        if who.agent == Agent::Grok {
                            reading.binding = Some(model::UsageBinding::Pane(pane.pane_id.clone()));
                        }
                    }
                }
                Some(tokens::Usage {
                    binding: reading.binding,
                    occupant: who.clone(),
                    context_pct: match reading.context {
                        model::UsageOutcome::Available(value) => Some(value),
                        _ => None,
                    },
                    weekly_remaining_pct: match reading.weekly_remaining {
                        model::UsageOutcome::Available(value)
                            if reading.weekly_window.is_some() =>
                        {
                            Some(value)
                        }
                        _ => None,
                    },
                    weekly_reset_unix: reading
                        .weekly_resets_at
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .and_then(|age| i64::try_from(age.as_secs()).ok()),
                })
            });
            self.check_shutdown()?;
            let mut desired = tokens::pane_tokens(
                &tokens::PaneInput {
                    pane_id: pane.pane_id.clone(),
                    occupant: occupant.clone(),
                    state: Some(pane.state.clone()),
                    usage,
                },
                mode,
                false,
                now_unix,
            );
            let (resolved, heading) = &headings[pane.workspace_id.as_str()];
            if first_pane && *resolved {
                desired.insert("lineup_group".into(), heading["lineup_repo"].clone());
                desired.insert(
                    "lineup_group_ship_text".into(),
                    heading["lineup_ship_text"].clone(),
                );
            }
            if !first_pane
                && config.non_first_pane_logo_indent != 0
                && let Some(Some(logo)) = desired.get_mut("lineup_logo")
            {
                logo.insert_str(
                    0,
                    &"\u{2800}".repeat(usize::from(config.non_first_pane_logo_indent)),
                );
            }
            let resource = self.resource(ResourceKind::Pane, &pane.pane_id);
            if let Err(error) = self.report(
                transport,
                &resource,
                &pane.stored_keys,
                occupant.as_ref(),
                &desired,
                None,
            ) {
                if capacity_skip_count(&error).is_some() {
                    skipped_resources = skipped_resources.saturating_add(1);
                    continue;
                }
                // Do not continue I/O after an ambiguous response: reconnect/reconcile.
                return Err(error);
            }
        }
        for workspace in &snapshot.workspaces {
            self.check_shutdown()?;
            let desired = &headings[workspace.workspace_id.as_str()].1;
            let resource = self.resource(ResourceKind::Workspace, &workspace.workspace_id);
            if let Err(error) = self.report(
                transport,
                &resource,
                &workspace.stored_keys,
                None,
                desired,
                None,
            ) {
                if capacity_skip_count(&error).is_none() {
                    return Err(error);
                }
                skipped_resources = skipped_resources.saturating_add(1);
            }
        }
        if skipped_resources > 0 {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                CapacitySkip { skipped_resources },
            ))
        } else {
            Ok(())
        }
    }
}

impl daemon::Publisher for MetadataPublisher {
    fn set_shutdown(&mut self, shutdown: Arc<AtomicBool>) {
        self.shutdown = Some(shutdown);
    }
    fn refresh(&mut self, transport: &mut Transport, config: &Config) -> io::Result<()> {
        config.validate()?;
        self.start_weekly(config)?;
        self.refresh_with_readers(transport, config, &CodexReader, &GrokReader)
    }
    fn shutdown(&mut self) {
        self.codex_control.stop();
        self.codex_weekly.take();
        self.grok_weekly.take();
    }
    fn clear(&mut self, transport: &mut Transport) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let snapshot = self.snapshot(transport)?;
        for pane in snapshot.panes {
            check_clear_deadline(deadline)?;
            let resource = self.resource(ResourceKind::Pane, &pane.pane_id);
            let desired = tokens::PANE_KEYS
                .iter()
                .map(|key| ((*key).into(), None))
                .collect();
            self.report(
                transport,
                &resource,
                &pane.stored_keys,
                None,
                &desired,
                Some(deadline),
            )?;
        }
        for workspace in snapshot.workspaces {
            check_clear_deadline(deadline)?;
            let resource = self.resource(ResourceKind::Workspace, &workspace.workspace_id);
            let desired = tokens::WORKSPACE_KEYS
                .iter()
                .map(|key| ((*key).into(), None))
                .collect();
            self.report(
                transport,
                &resource,
                &workspace.stored_keys,
                None,
                &desired,
                Some(deadline),
            )?;
        }
        Ok(())
    }
    fn reconnected(&mut self) {
        self.planner = tokens::Publisher::restore_with_floors(
            self.planner.checkpoint(),
            self.planner.checkpoint_floors(),
        );
    }
}

impl Drop for MetadataPublisher {
    fn drop(&mut self) {
        // Also covers errors before/around daemon lifecycle initialization.
        self.codex_control.stop();
    }
}

/// Preserve snapshot pane order, including panes without supported occupants.
/// Recomputed each refresh so headings and logo indentation move together.
fn first_pane_ids(snapshot: &protocol::Snapshot) -> BTreeSet<&str> {
    let mut seen_workspaces = BTreeSet::new();
    snapshot
        .panes
        .iter()
        .filter(|pane| seen_workspaces.insert(pane.workspace_id.as_str()))
        .map(|pane| pane.pane_id.as_str())
        .collect()
}

fn check_clear_deadline(deadline: Instant) -> io::Result<()> {
    // Transport bounds write_frame and read_frame to 500 ms each. Do not
    // start a report unless its entire worst-case socket round trip fits.
    if deadline.saturating_duration_since(Instant::now()) < Duration::from_secs(1) {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "metadata clear deadline reached; remaining keys will expire",
        ))
    } else {
        Ok(())
    }
}

fn exchange(transport: &mut Transport, request: &Value) -> io::Result<Value> {
    transport.send_line(&serde_json::to_vec(request)?)?;
    serde_json::from_slice(&transport.receive_line()?)
        .map_err(|_| io::Error::other("invalid Herdr JSON response"))
}
fn unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}
fn agent_id(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
        Agent::Grok => "grok",
        Agent::Agy => "agy",
        Agent::Kimi => "kimi",
        Agent::Hermes => "hermes",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{daemon::Publisher as _, model::*, socket::Transport};
    use serde_json::{Value, json};
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        time::Duration,
    };

    fn remember_tokens(stored: &mut BTreeMap<String, Value>, report: &Value) {
        let params = &report["params"];
        let id = params["pane_id"]
            .as_str()
            .or_else(|| params["workspace_id"].as_str())
            .unwrap();
        let tokens = stored
            .entry(id.into())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap();
        for (key, value) in params["tokens"].as_object().unwrap() {
            if value.is_null() {
                tokens.remove(key);
            } else {
                tokens.insert(key.clone(), value.clone());
            }
        }
    }

    fn with_stored_tokens(mut response: Value, stored: &BTreeMap<String, Value>) -> Value {
        let snapshot = &mut response["result"]["snapshot"];
        for (kind, id_key) in [("panes", "pane_id"), ("workspaces", "workspace_id")] {
            for resource in snapshot[kind].as_array_mut().unwrap() {
                if let Some(tokens) = stored.get(resource[id_key].as_str().unwrap()) {
                    resource["tokens"] = tokens.clone();
                }
            }
        }
        response
    }

    // Implements Herdr's per-key TTL patch semantics, closing after each reply.
    // Only synthetic session records and metadata are exchanged.
    fn token_server(
        listener: UnixListener,
        clock: Arc<std::sync::atomic::AtomicU64>,
    ) -> std::thread::JoinHandle<()> {
        token_server_with_template(
            listener,
            clock,
            Arc::new(std::sync::Mutex::new(snapshot(&json!("template"), true))),
        )
    }

    fn token_server_with_template(
        listener: UnixListener,
        clock: Arc<std::sync::atomic::AtomicU64>,
        template: Arc<std::sync::Mutex<Value>>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let mut resources = BTreeMap::<String, BTreeMap<String, (Value, u64)>>::new();
            loop {
                let request = receive(&mut stream);
                let now = clock.load(Ordering::Acquire);
                for tokens in resources.values_mut() {
                    tokens.retain(|_, (_, expires)| *expires > now);
                }
                let response = match request["method"].as_str().unwrap() {
                    "session.snapshot" => {
                        let mut response = template.lock().unwrap().clone();
                        response["id"] = request["id"].clone();
                        for (kind, id_key) in [("panes", "pane_id"), ("workspaces", "workspace_id")]
                        {
                            for resource in
                                response["result"]["snapshot"][kind].as_array_mut().unwrap()
                            {
                                let tokens = resources
                                    .entry(resource[id_key].as_str().unwrap().into())
                                    .or_default();
                                resource["tokens"] = Value::Object(
                                    tokens
                                        .iter()
                                        .map(|(k, (v, _))| (k.clone(), v.clone()))
                                        .collect(),
                                );
                            }
                        }
                        response
                    }
                    "pane.report_metadata" | "workspace.report_metadata" => {
                        let params = &request["params"];
                        let id = params["pane_id"]
                            .as_str()
                            .or_else(|| params["workspace_id"].as_str())
                            .unwrap();
                        let tokens = resources.entry(id.into()).or_default();
                        let ttl = params["ttl_ms"].as_u64().unwrap();
                        for (key, value) in params["tokens"].as_object().unwrap() {
                            if value.is_null() {
                                tokens.remove(key);
                            } else {
                                tokens.insert(key.clone(), (value.clone(), now + ttl));
                            }
                        }
                        json!({"id":request["id"],"result":{"type":"ok"}})
                    }
                    "test.finish" => {
                        writeln!(
                            stream.get_mut(),
                            "{}",
                            json!({"id":request["id"],"result":{"type":"ok"}})
                        )
                        .unwrap();
                        break;
                    }
                    other => panic!("unexpected fake method: {other}"),
                };
                writeln!(stream.get_mut(), "{response}").unwrap();
            }
        })
    }

    #[test]
    fn fake_socket_renews_before_slow_cycle_crosses_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let server = token_server(UnixListener::bind(&socket).unwrap(), clock.clone());
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        let desired = BTreeMap::from([
            ("lineup_logo".into(), Some("Codex".into())),
            ("lineup_weekly_text".into(), Some("—".into())),
        ]);
        // R = 5s, cycle work w = 3s: the daemon schedules from completion,
        // so P = R + w = 8s. The old 2R threshold skips P, then renews at
        // 2P = 16s > TTL = 3R = 15s. Check just before that next cycle.
        for now in [0, 8000] {
            clock.store(now, Ordering::Release);
            publisher.started = Instant::now() - Duration::from_millis(now);
            let snapshot = publisher.snapshot(&mut transport).unwrap();
            let resource = publisher.resource(ResourceKind::Pane, "w1:p1");
            publisher
                .report(
                    &mut transport,
                    &resource,
                    &snapshot.panes[0].stored_keys,
                    None,
                    &desired,
                    None,
                )
                .unwrap();
        }
        clock.store(15999, Ordering::Release);
        let (_, response) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        publisher.request(&mut transport, "test.finish").unwrap();
        server.join().unwrap();
        let tokens = &response["result"]["snapshot"]["panes"][0]["tokens"];
        assert_eq!(
            tokens["lineup_logo"], "Codex",
            "logo expired between healthy cycles"
        );
        assert_eq!(
            tokens["lineup_weekly_text"], "—",
            "missing usage must keep its key"
        );
    }

    #[test]
    fn fake_socket_repairs_missing_live_keys_before_cached_renewal() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let server = token_server(UnixListener::bind(&socket).unwrap(), clock.clone());
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        let desired = BTreeMap::from([("lineup_weekly_text".into(), Some("—".into()))]);
        let first = publisher.snapshot(&mut transport).unwrap();
        let resource = publisher.resource(ResourceKind::Pane, "w1:p1");
        publisher
            .report(
                &mut transport,
                &resource,
                &first.panes[0].stored_keys,
                None,
                &desired,
                None,
            )
            .unwrap();
        // Simulate a server-side expiry/removal while the planner still thinks
        // the value is fresh. The authoritative snapshot must force repair.
        clock.store(16000, Ordering::Release);
        let missing = publisher.snapshot(&mut transport).unwrap();
        assert!(!missing.panes[0].stored_keys.contains("lineup_weekly_text"));
        publisher
            .report(
                &mut transport,
                &resource,
                &missing.panes[0].stored_keys,
                None,
                &desired,
                None,
            )
            .unwrap();
        let (_, response) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        publisher.request(&mut transport, "test.finish").unwrap();
        server.join().unwrap();
        assert_eq!(
            response["result"]["snapshot"]["panes"][0]["tokens"]["lineup_weekly_text"],
            "—"
        );
    }

    #[test]
    fn fake_socket_timeout_keeps_exact_codex_binding_for_account_quota() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let server = token_server(UnixListener::bind(&socket).unwrap(), clock);
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().into(),
            "fake".into(),
            Homes {
                codex: Some(dir.path().join("empty-home")),
                ..Homes::default()
            },
        )
        .unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut calls = 0;
        publisher.codex_weekly = Some(
            WeeklyWorker::start(shutdown.clone(), Duration::ZERO, move || {
                calls += 1;
                if calls == 2 {
                    ready_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                if calls >= 2 {
                    return None;
                }
                Some(WeeklySample {
                    used_percent: 27.0,
                    resets_at: Some(SystemTime::now() + Duration::from_secs(3600)),
                })
            })
            .unwrap(),
        );
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let config = Config {
            allow_network: true,
            ..Config::default()
        };
        publisher
            .refresh_with_readers(&mut transport, &config, &SampleReader, &TimeoutReader)
            .unwrap();
        publisher
            .refresh_with_readers(&mut transport, &config, &TimeoutReader, &TimeoutReader)
            .unwrap();
        let (_, response) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while publisher
            .codex_weekly
            .as_ref()
            .unwrap()
            .sample(SystemTime::now())
            .is_some()
        {
            assert!(
                Instant::now() < deadline,
                "fake account revocation did not complete"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        publisher
            .refresh_with_readers(&mut transport, &config, &TimeoutReader, &TimeoutReader)
            .unwrap();
        let (_, revoked) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        publisher.request(&mut transport, "test.finish").unwrap();
        shutdown.store(true, Ordering::Release);
        server.join().unwrap();
        assert_eq!(
            revoked["result"]["snapshot"]["panes"][0]["tokens"]["lineup_weekly_text"],
            "—"
        );
        let tokens = &response["result"]["snapshot"]["panes"][0]["tokens"];
        assert_eq!(tokens["lineup_context_text"], "42%");
        assert!(
            tokens["lineup_weekly_text"]
                .as_str()
                .unwrap()
                .starts_with("7d 73% "),
            "timeout discarded a verified home binding"
        );
    }

    #[test]
    fn fake_socket_observation_written_during_refresh_is_not_future() {
        let _guard = crate::statusline::READER_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let template = Arc::new(std::sync::Mutex::new(snapshot(&json!("template"), true)));
        template.lock().unwrap()["result"]["snapshot"]["panes"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "pane_id":"w1:p2", "workspace_id":"w1", "agent":"claude", "agent_status":"working",
                "agent_session":{"agent":"claude", "kind":"id", "value":"synthetic-claude"}
            }));
        let server = token_server_with_template(
            UnixListener::bind(&socket).unwrap(),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
            template,
        );
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().into(),
            "fake".into(),
            Homes {
                codex: Some(dir.path().join("empty-home")),
                statusline_state: Some(dir.path().into()),
                ..Homes::default()
            },
        )
        .unwrap();
        struct WriteDuringEarlierPane(std::path::PathBuf);
        impl UsageReader for WriteDuringEarlierPane {
            fn read(&self, r: &ReadRequest<'_>) -> UsageReading {
                std::thread::sleep(Duration::from_millis(20));
                // The real hook stamps receipt time and atomically replaces its record.
                let record = crate::statusline::Record {
                    version: 1,
                    agent: crate::statusline::Agent::Claude,
                    key: "synthetic-claude".into(),
                    observed_ms: SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    context: Some(23.0),
                    weekly_remaining: Some(73.0),
                    weekly_reset_ms: None,
                };
                std::fs::create_dir_all(self.0.join("statusline")).unwrap();
                let temporary = tempfile::NamedTempFile::new_in(self.0.join("statusline")).unwrap();
                serde_json::to_writer(temporary.as_file(), &record).unwrap();
                temporary
                    .persist(crate::statusline::record_path(
                        &self.0,
                        record.agent,
                        &record.key,
                    ))
                    .unwrap();
                SampleReader.read(r)
            }
        }
        publisher
            .refresh_with_readers(
                &mut transport,
                &Config::default(),
                &WriteDuringEarlierPane(dir.path().into()),
                &TimeoutReader,
            )
            .unwrap();
        let (_, response) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        publisher.request(&mut transport, "test.finish").unwrap();
        server.join().unwrap();
        let tokens = &response["result"]["snapshot"]["panes"][1]["tokens"];
        assert_eq!(tokens["lineup_context_text"], "23%");
        assert_eq!(tokens["lineup_weekly_text"], "7d 73%");
    }

    #[test]
    fn fake_socket_shared_budget_preserves_claude_weekly_but_not_expired_or_rebound_usage() {
        let _guard = crate::statusline::READER_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let template = Arc::new(std::sync::Mutex::new(snapshot(&json!("template"), true)));
        template.lock().unwrap()["result"]["snapshot"]["panes"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "pane_id":"w1:p2", "workspace_id":"w1", "agent":"claude", "agent_status":"idle",
                "agent_session":{"agent":"claude", "kind":"id", "value":"synthetic-claude"}
            }));
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let server = token_server_with_template(
            UnixListener::bind(&socket).unwrap(),
            clock,
            template.clone(),
        );
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().into(),
            "fake".into(),
            Homes {
                codex: Some(dir.path().join("empty-home")),
                statusline_state: Some(dir.path().into()),
                ..Homes::default()
            },
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("statusline")).unwrap();
        let record = crate::statusline::Record {
            version: 1,
            agent: crate::statusline::Agent::Claude,
            key: "synthetic-claude".into(),
            observed_ms: (unix(SystemTime::now()) as u64) * 1000,
            context: Some(23.0),
            weekly_remaining: Some(73.0),
            weekly_reset_ms: Some(((unix(SystemTime::now()) + 3600) as u64) * 1000),
        };
        std::fs::write(
            crate::statusline::record_path(dir.path(), record.agent, &record.key),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        struct ExhaustBudget;
        impl UsageReader for ExhaustBudget {
            fn read(&self, r: &ReadRequest<'_>) -> UsageReading {
                std::thread::sleep(r.timeout + Duration::from_millis(1));
                TimeoutReader.read(r)
            }
        }
        let config = Config {
            reader_timeout_ms: 100,
            ..Config::default()
        };
        publisher
            .refresh_with_readers(&mut transport, &config, &SampleReader, &TimeoutReader)
            .unwrap();
        publisher
            .refresh_with_readers(&mut transport, &config, &ExhaustBudget, &TimeoutReader)
            .unwrap();
        let (_, retained) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        let cached = publisher.local_readings.get_mut("w1:p2").unwrap();
        // Inject an elapsed reset into the prior observation without real waits.
        cached.weekly_resets_at = Some(SystemTime::now() - Duration::from_secs(1));
        publisher
            .refresh_with_readers(&mut transport, &config, &ExhaustBudget, &TimeoutReader)
            .unwrap();
        let (_, expired) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        // No reset: an old observation must not gain a fresh age from a timeout.
        let cached = publisher.local_readings.get_mut("w1:p2").unwrap();
        cached.weekly_remaining = UsageOutcome::Available(73.0);
        cached.weekly_window = Some(WeeklyWindow::RollingSevenDays);
        cached.weekly_resets_at = None;
        cached.observed_at = Some(SystemTime::now() - Duration::from_secs(61));
        publisher
            .refresh_with_readers(&mut transport, &config, &ExhaustBudget, &TimeoutReader)
            .unwrap();
        let (_, stale) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        template.lock().unwrap()["result"]["snapshot"]["panes"][1]["agent_session"]["value"] =
            json!("other-session");
        publisher
            .refresh_with_readers(&mut transport, &config, &ExhaustBudget, &TimeoutReader)
            .unwrap();
        let (_, rebound) = publisher
            .request(&mut transport, "session.snapshot")
            .unwrap();
        publisher.request(&mut transport, "test.finish").unwrap();
        server.join().unwrap();
        let pane = |response: &Value| response["result"]["snapshot"]["panes"][1]["tokens"].clone();
        assert_eq!(pane(&retained)["lineup_context_text"], "23%");
        assert!(
            pane(&retained)["lineup_weekly_text"]
                .as_str()
                .unwrap()
                .starts_with("7d 73% ")
        );
        assert_eq!(pane(&expired)["lineup_context_text"], "23%");
        assert_eq!(pane(&expired)["lineup_weekly_text"], "—");
        assert_eq!(pane(&stale)["lineup_weekly_text"], "—");
        assert_eq!(pane(&rebound)["lineup_context_text"], "—\u{2800}\u{2800}");
        assert_eq!(pane(&rebound)["lineup_weekly_text"], "—");
    }

    #[test]
    fn fake_socket_timeout_cache_does_not_cross_identity_or_authoritative_failure() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let template = Arc::new(std::sync::Mutex::new(snapshot(&json!("template"), true)));
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let server = token_server_with_template(
            UnixListener::bind(&socket).unwrap(),
            clock,
            template.clone(),
        );
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().into(),
            "fake".into(),
            Homes {
                codex: Some(dir.path().join("empty-home")),
                ..Homes::default()
            },
        )
        .unwrap();
        let config = Config::default();
        let mut observations = Vec::new();
        for change in [
            "session",
            "cwd",
            "provider",
            "departure",
            "invalid",
            "missing",
            "mismatch",
        ] {
            *template.lock().unwrap() = snapshot(&json!("template"), true);
            publisher
                .refresh_with_readers(&mut transport, &config, &SampleReader, &TimeoutReader)
                .unwrap();
            match change {
                "session" => {
                    template.lock().unwrap()["result"]["snapshot"]["panes"][0]["agent_session"]["value"] =
                        json!("other-session")
                }
                "cwd" => {
                    template.lock().unwrap()["result"]["snapshot"]["panes"][0]["cwd"] =
                        json!("/synthetic-other-repo")
                }
                "provider" => {
                    template.lock().unwrap()["result"]["snapshot"]["panes"][0]["agent"] =
                        json!("grok")
                }
                "departure" => {
                    template.lock().unwrap()["result"]["snapshot"]["panes"] = json!([]);
                    publisher
                        .refresh_with_readers(
                            &mut transport,
                            &config,
                            &TimeoutReader,
                            &TimeoutReader,
                        )
                        .unwrap();
                    *template.lock().unwrap() = snapshot(&json!("template"), true);
                }
                "invalid" | "missing" | "mismatch" => {
                    struct FailedReader(&'static str);
                    impl UsageReader for FailedReader {
                        fn read(&self, r: &ReadRequest<'_>) -> UsageReading {
                            let reason = if self.0 == "missing" {
                                MissingReason::NotFound
                            } else {
                                MissingReason::Invalid
                            };
                            let mut reading =
                                UsageReading::missing(r, SourceKind::CodexRollout, reason);
                            if self.0 == "mismatch" {
                                reading.identity.native_session_id = "unrelated".into();
                            }
                            reading
                        }
                    }
                    publisher
                        .refresh_with_readers(
                            &mut transport,
                            &config,
                            &FailedReader(change),
                            &TimeoutReader,
                        )
                        .unwrap();
                }
                _ => unreachable!(),
            }
            publisher
                .refresh_with_readers(&mut transport, &config, &TimeoutReader, &TimeoutReader)
                .unwrap();
            let (_, response) = publisher
                .request(&mut transport, "session.snapshot")
                .unwrap();
            observations.push((
                change,
                response["result"]["snapshot"]["panes"][0]["tokens"].clone(),
            ));
        }
        publisher.request(&mut transport, "test.finish").unwrap();
        server.join().unwrap();
        for (change, tokens) in observations {
            assert_eq!(
                tokens["lineup_context_text"], "—\u{2800}\u{2800}",
                "cached context survived {change}"
            );
            assert_eq!(
                tokens["lineup_weekly_text"], "—",
                "cached quota survived {change}"
            );
        }
    }

    #[test]
    fn heading_timeout_retains_only_the_same_workspace_and_occupant() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().to_owned(), "fake".into(), Homes::default()).unwrap();
        let cwd = Path::new("/synthetic/repo");
        let repo = ship_time::Repository {
            name: "repo".into(),
            ship_unix: Some(123),
        };
        let occupant = vec![(
            "p1".into(),
            Some(tokens::Occupant {
                agent: Agent::Codex,
                native_session: Some("native-one".into()),
            }),
        )];
        assert_eq!(
            publisher.heading_repository(
                "w1",
                Some(cwd),
                &occupant,
                ship_time::RepositoryRead::Complete(Some(repo.clone()))
            ),
            Some(repo.clone())
        );
        assert_eq!(
            publisher.heading_repository(
                "w1",
                Some(cwd),
                &occupant,
                ship_time::RepositoryRead::Interrupted
            ),
            Some(repo)
        );
        assert_eq!(
            publisher.heading_repository(
                "w2",
                Some(cwd),
                &occupant,
                ship_time::RepositoryRead::Interrupted
            ),
            None
        );
        let changed = vec![(
            "p1".into(),
            Some(tokens::Occupant {
                agent: Agent::Codex,
                native_session: Some("native-two".into()),
            }),
        )];
        assert_eq!(
            publisher.heading_repository(
                "w1",
                Some(cwd),
                &changed,
                ship_time::RepositoryRead::Interrupted
            ),
            None
        );
    }

    #[test]
    fn confirmed_repository_removal_and_cwd_change_discard_cached_heading() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().to_owned(), "fake".into(), Homes::default()).unwrap();
        let cwd = Path::new("/synthetic/repo");
        let repo = ship_time::Repository {
            name: "repo".into(),
            ship_unix: Some(123),
        };
        for replacement in [Some(Path::new("/synthetic/other")), None] {
            publisher.heading_repository(
                "w1",
                Some(cwd),
                &[],
                ship_time::RepositoryRead::Complete(Some(repo.clone())),
            );
            assert_eq!(
                publisher.heading_repository(
                    "w1",
                    replacement,
                    &[],
                    ship_time::RepositoryRead::Interrupted
                ),
                None
            );
        }
        publisher.heading_repository(
            "w1",
            Some(cwd),
            &[],
            ship_time::RepositoryRead::Complete(Some(repo)),
        );
        assert_eq!(
            publisher.heading_repository(
                "w1",
                Some(cwd),
                &[],
                ship_time::RepositoryRead::Complete(None)
            ),
            None
        );
        assert_eq!(
            publisher.heading_repository(
                "w1",
                Some(cwd),
                &[],
                ship_time::RepositoryRead::Interrupted
            ),
            None
        );
    }

    #[test]
    fn missing_reset_weekly_covers_next_query_and_refresh() {
        let now = Instant::now();
        let sample = Some(model::WeeklySample {
            used_percent: 35.0,
            resets_at: None,
        });
        let settings = crate::quota_grok::Settings::default();
        let config = Config::default();
        let refresh = Duration::from_millis(config.refresh_interval_ms);
        let next_query_done = now + settings.interval() + settings.timeout();
        assert!(
            fresh_grok_weekly(
                sample,
                Some(now),
                next_query_done + refresh,
                config.usage_max_age_ms,
                config.refresh_interval_ms
            )
            .is_some()
        );
        assert!(
            fresh_grok_weekly(
                sample,
                Some(now),
                next_query_done + refresh + Duration::from_millis(1),
                config.usage_max_age_ms,
                config.refresh_interval_ms
            )
            .is_none()
        );
        let longer_age = settings.interval() + settings.timeout() + refresh + settings.interval();
        assert!(
            fresh_grok_weekly(
                sample,
                Some(now),
                now + longer_age,
                longer_age.as_millis() as u64,
                config.refresh_interval_ms
            )
            .is_some()
        );
        assert!(
            fresh_grok_weekly(
                sample,
                None,
                now,
                config.usage_max_age_ms,
                config.refresh_interval_ms
            )
            .is_none()
        );
    }

    #[test]
    fn worker_auth_failure_is_visible_to_doctor_with_injected_home() {
        if let Some(root) = std::env::var_os("LINEUP_FAKE_DIAGNOSTIC_CHILD") {
            let root = PathBuf::from(root);
            let shutdown = Arc::new(AtomicBool::new(false));
            let mut publisher =
                MetadataPublisher::new(root.clone(), "fake".into(), Homes::from_env()).unwrap();
            publisher.set_shutdown(shutdown.clone());
            publisher
                .start_weekly(&Config {
                    allow_network: true,
                    providers: vec!["grok".into()],
                    ..Config::default()
                })
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let diagnostic = crate::doctor::weekly_diagnostics(&root);
                if diagnostic.contains("grok=auth_shape") {
                    assert!(diagnostic.contains("codex=disabled"));
                    break;
                }
                assert!(Instant::now() < deadline, "fixed auth result not saved");
                std::thread::sleep(Duration::from_millis(10));
            }
            shutdown.store(true, Ordering::Release);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let grok = dir.path().join(".grok");
        std::fs::create_dir(&grok).unwrap();
        std::fs::write(grok.join("auth.json"), "{}").unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "publish::tests::worker_auth_failure_is_visible_to_doctor_with_injected_home",
            ])
            .env("LINEUP_FAKE_DIAGNOSTIC_CHILD", dir.path())
            .env("HOME", dir.path())
            .env_remove("GROK_HOME")
            .env_remove("GROK_AUTH_FILE")
            .output()
            .unwrap();
        assert!(child.status.success(), "fake diagnostic worker failed");
    }

    #[test]
    fn disabled_workers_save_fixed_diagnostics_without_queries() {
        let dir = tempfile::tempdir().unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        publisher.set_shutdown(Arc::new(AtomicBool::new(false)));
        publisher.start_weekly(&Config::default()).unwrap();
        assert_eq!(
            crate::doctor::weekly_diagnostics(dir.path()),
            "Last saved weekly query results: codex=disabled; grok=disabled"
        );
    }

    #[test]
    fn fake_socket_weekly_cache_publishes_without_waiting_for_queries() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let request = receive(&mut stream);
            let mut response = snapshot(&request["id"], true);
            let panes = &mut response["result"]["snapshot"]["panes"];
            *panes = json!([
                {"pane_id":"w1:p1","workspace_id":"w1","agent":"codex","agent_status":"working","agent_session":{"agent":"codex","kind":"id","value":"synthetic-session"}},
                {"pane_id":"w1:p2","workspace_id":"w1","agent":"grok","agent_status":"idle"},
                {"pane_id":"w1:p3","workspace_id":"w1","agent":"grok","agent_status":"idle"},
                {"pane_id":"w1:p4","workspace_id":"w1","agent":"codex","agent_status":"working","agent_session":{"agent":"codex","kind":"id","value":"foreign-session"}}
            ]);
            writeln!(stream.get_mut(), "{response}").unwrap();
            let mut reports = Vec::new();
            for _ in 0..5 {
                let report = receive(&mut stream);
                writeln!(
                    stream.get_mut(),
                    "{}",
                    json!({"id":report["id"],"result":{"type":"ok"}})
                )
                .unwrap();
                reports.push(report);
            }
            reports
        });
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut releases = Vec::new();
        let mut workers = Vec::new();
        for used_percent in [27.0, 42.0] {
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let mut calls = 0;
            workers.push(
                model::WeeklyWorker::start(shutdown.clone(), Duration::ZERO, move || {
                    calls += 1;
                    if calls == 2 {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    }
                    Some(model::WeeklySample {
                        used_percent,
                        resets_at: Some(SystemTime::now() + Duration::from_secs(86400)),
                    })
                })
                .unwrap(),
            );
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            releases.push(release_tx);
        }
        let mut publisher = MetadataPublisher::new(
            dir.path().into(),
            "fake".into(),
            Homes {
                codex: Some(dir.path().join("codex")),
                grok: None,
                ..Homes::default()
            },
        )
        .unwrap();
        publisher.codex_weekly = Some(workers.remove(0));
        publisher.grok_weekly = Some(workers.remove(0));
        publisher.set_shutdown(shutdown.clone());
        struct AssociatedReader;
        impl UsageReader for AssociatedReader {
            fn read(&self, r: &ReadRequest<'_>) -> UsageReading {
                if r.identity.native_session_id == "synthetic-session" {
                    UsageReading::sample(r, SourceKind::CodexRollout, 7.0, r.now)
                } else {
                    UsageReading::missing(r, SourceKind::CodexRollout, MissingReason::Unassociated)
                }
            }
        }
        let mut transport = Transport::connect(&socket).unwrap();
        let started = Instant::now();
        let result = publisher.refresh_with_readers(
            &mut transport,
            &Config {
                allow_network: true,
                ..Config::default()
            },
            &AssociatedReader,
            &TimeoutReader,
        );
        let elapsed = started.elapsed();
        shutdown.store(true, Ordering::Release);
        for release in releases {
            release.send(()).unwrap();
        }
        result.unwrap();
        let reports = server.join().unwrap();
        assert!(elapsed < Duration::from_secs(1));
        for (report, remaining) in reports[..3].iter().zip(["73", "58", "58"]) {
            let tokens = &report["params"]["tokens"];
            assert_eq!(tokens["lineup_weekly_pct"], remaining);
            assert!(
                tokens["lineup_weekly_text"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("7d {remaining}% "))
            );
        }
        assert_eq!(reports[0]["params"]["tokens"]["lineup_context_pct"], "7");
        assert_eq!(
            reports[1]["params"]["tokens"]["lineup_context_text"],
            "—\u{2800}\u{2800}"
        );
        assert_eq!(reports[3]["params"]["tokens"]["lineup_weekly_text"], "—");
    }

    #[test]
    fn group_heading_moves_with_snapshot_order_and_clears_without_repository() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("heading-repo");
        std::fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(args)
                    .env("HOME", dir.path())
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_AUTHOR_DATE", "2024-01-01T00:00:00Z")
                    .env("GIT_COMMITTER_DATE", "2024-01-01T00:00:00Z")
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        git(&["init", "--initial-branch=main"]);
        git(&[
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
        ]);
        git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let mut stored = BTreeMap::new();
            let mut cycles = Vec::new();
            for (cycle, count) in [(0, 5), (1, 2), (2, 2)] {
                let request = receive(&mut stream);
                let mut panes = vec![
                    json!({"pane_id":"w1:p10","workspace_id":"w1","agent_status":"idle"}),
                    json!({"pane_id":"w2:p1","workspace_id":"w2","agent":"kimi","agent_status":"idle"}),
                    json!({"pane_id":"w1:p2","workspace_id":"w1","agent":"kimi","agent_status":"idle"}),
                ];
                if cycle > 0 {
                    panes.swap(0, 2);
                }
                let checkout = if cycle < 2 {
                    repo.to_str().unwrap()
                } else {
                    "/nonexistent-heading-fixture"
                };
                writeln!(
                    stream.get_mut(),
                    "{}",
                    with_stored_tokens(
                        json!({"id":request["id"],"result":{
                            "type":"session_snapshot","snapshot":{"version":"0.9.3","protocol":22,
                            "panes":panes,"workspaces":[
                                {"workspace_id":"w1","worktree":{"checkout_path":checkout}},
                                {"workspace_id":"w2"}]}
                        }}),
                        &stored
                    )
                )
                .unwrap();
                let mut reports = BTreeMap::new();
                for _ in 0..count {
                    let report = receive(&mut stream);
                    writeln!(
                        stream.get_mut(),
                        "{}",
                        json!({"id":report["id"],"result":{"type":"ok"}})
                    )
                    .unwrap();
                    remember_tokens(&mut stored, &report);
                    let params = &report["params"];
                    if cycle == 0 && params["pane_id"] == "w1:p10" {
                        assert_eq!(params["tokens"]["lineup_group"], "heading-repo");
                    }
                    let id = params["pane_id"]
                        .as_str()
                        .or_else(|| params["workspace_id"].as_str())
                        .unwrap();
                    assert!(params["tokens"].as_object().unwrap().len() <= 16);
                    reports.insert(id.to_owned(), params["tokens"].clone());
                }
                cycles.push(reports);
            }
            cycles
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        for _ in 0..3 {
            publisher
                .refresh(&mut transport, &Config::default())
                .unwrap();
        }
        let cycles = server.join().unwrap();
        assert_eq!(cycles[0]["w1:p10"]["lineup_group"], "heading-repo");
        assert_eq!(
            cycles[0]["w1:p10"]["lineup_group_ship_text"],
            cycles[0]["w1"]["lineup_ship_text"]
        );
        for pane in ["w1:p2", "w2:p1"] {
            assert!(
                cycles[0][pane]
                    .as_object()
                    .unwrap()
                    .contains_key("lineup_group")
            );
            assert!(cycles[0][pane]["lineup_group"].is_null());
            assert!(cycles[0][pane]["lineup_group_ship_text"].is_null());
        }
        assert_eq!(cycles[1]["w1:p2"]["lineup_group"], "heading-repo");
        assert_eq!(
            cycles[1]["w1:p2"]["lineup_group_ship_text"],
            cycles[0]["w1"]["lineup_ship_text"]
        );
        assert_eq!(
            cycles[1]["w1:p10"],
            json!({"lineup_group":null,"lineup_group_ship_text":null})
        );
        assert_eq!(
            cycles[2]["w1:p2"],
            json!({"lineup_group":null,"lineup_group_ship_text":null})
        );
        assert_eq!(cycles[2]["w1"]["lineup_repo"], "—");
    }

    #[test]
    fn logo_indent_tracks_snapshot_order_and_changes_only_logos() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let mut stored = BTreeMap::new();
            let original = [
                ("w1:p10", "w1"),
                ("w2:p9", "w2"),
                ("w1:p2", "w1"),
                ("w2:p3", "w2"),
            ];
            let reordered = [original[2], original[1], original[0], original[3]];
            let mut cycles = Vec::new();
            for (order, count) in [(original, 6), (original, 2), (reordered, 2), (reordered, 2)] {
                let request = receive(&mut stream);
                let panes: Vec<_> = order.iter().map(|(pane, workspace)| json!({
                    "pane_id":pane, "workspace_id":workspace, "agent":"kimi", "agent_status":"idle"
                })).collect();
                writeln!(stream.get_mut(), "{}", with_stored_tokens(json!({"id":request["id"],"result":{
                    "type":"session_snapshot","snapshot":{"version":"0.9.3","protocol":22,
                    "panes":panes,"workspaces":[{"workspace_id":"w1"},{"workspace_id":"w2"}]}
                }}), &stored)).unwrap();
                let mut reports = BTreeMap::new();
                for _ in 0..count {
                    let report = receive(&mut stream);
                    writeln!(
                        stream.get_mut(),
                        "{}",
                        json!({"id":report["id"],"result":{"type":"ok"}})
                    )
                    .unwrap();
                    remember_tokens(&mut stored, &report);
                    if let Some(pane) = report["params"]["pane_id"].as_str() {
                        reports.insert(pane.to_owned(), report["params"]["tokens"].clone());
                    }
                }
                cycles.push(reports);
            }
            cycles
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        let mut config = Config::default();
        publisher.refresh(&mut transport, &config).unwrap();
        config.non_first_pane_logo_indent = 2;
        publisher.refresh(&mut transport, &config).unwrap();
        publisher.refresh(&mut transport, &config).unwrap();
        config.non_first_pane_logo_indent = 0;
        publisher.refresh(&mut transport, &config).unwrap();
        let cycles = server.join().unwrap();
        let expected = tokens::pane_tokens(
            &tokens::PaneInput {
                occupant: Some(tokens::Occupant {
                    agent: Agent::Kimi,
                    native_session: None,
                }),
                state: Some("idle".into()),
                ..Default::default()
            },
            tokens::LogoMode::Auto,
            false,
            0,
        );
        let baseline = protocol::metadata_request(
            "baseline",
            &tokens::Publisher::default()
                .prepare(
                    &ResourceId {
                        endpoint_session: "fake".into(),
                        kind: ResourceKind::Pane,
                        id: "baseline".into(),
                    },
                    &tokens::ResourceSnapshot {
                        exists: true,
                        ..Default::default()
                    },
                    None,
                    &expected,
                    0,
                    5000,
                )
                .unwrap()
                .unwrap(),
        )
        .unwrap()["params"]["tokens"]
            .clone();
        for report in cycles[0].values() {
            assert_eq!(
                serde_json::to_vec(report).unwrap(),
                serde_json::to_vec(&baseline).unwrap()
            );
        }
        for (cycle, expected_logos) in [
            (
                1,
                [
                    ("w1:p2", "\u{2800}\u{2800}Kimi"),
                    ("w2:p3", "\u{2800}\u{2800}Kimi"),
                ],
            ),
            (2, [("w1:p2", "Kimi"), ("w1:p10", "\u{2800}\u{2800}Kimi")]),
            (3, [("w1:p10", "Kimi"), ("w2:p3", "Kimi")]),
        ] {
            assert_eq!(cycles[cycle].len(), 2);
            for (pane, logo) in expected_logos {
                assert_eq!(cycles[cycle][pane], json!({"lineup_logo":logo}));
            }
        }
    }

    #[test]
    fn real_herdr_one_response_per_connection_publishes_heading() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("herdr.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            // Captured in an isolated Herdr 0.9.3 session (protocol 22).
            // Optional fields absent in that response stay absent here.
            let (stream, _) = listener.accept().unwrap();
            let mut stream = BufReader::new(stream);
            let request = receive(&mut stream);
            let response = json!({"id":request["id"],"result":{"type":"session_snapshot","snapshot":{
                "version":"0.9.3","protocol":22,
                "workspaces":[{"workspace_id":"w1","number":1,"label":"isolated","focused":true,
                    "pane_count":1,"tab_count":1,"active_tab_id":"w1:t1","agent_status":"unknown"}],
                "panes":[{"pane_id":"w1:p1","terminal_id":"isolated-terminal","workspace_id":"w1",
                    "tab_id":"w1:t1","focused":true,"cwd":"/nonexistent-isolated-fixture",
                    "foreground_cwd":"/nonexistent-isolated-fixture","agent_status":"unknown","revision":0}]
            }}});
            writeln!(stream.get_mut(), "{response}").unwrap();
            drop(stream); // The real JSON API closes after every response.
            listener.set_nonblocking(true).unwrap();
            let mut report = None;
            for _ in 0..2 {
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            break BufReader::new(stream);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                return None;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                let request = receive(&mut stream);
                writeln!(
                    stream.get_mut(),
                    "{}",
                    json!({"id":request["id"],"result":{"type":"ok"}})
                )
                .unwrap();
                report = Some(request);
            }
            report
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "isolated".into(), Homes::default()).unwrap();
        let result = publisher.refresh(&mut transport, &Config::default());
        let report = server.join().unwrap();
        assert!(result.is_ok(), "real one-shot response failed: {result:?}");
        let report = report.unwrap();
        assert_eq!(report["method"], "workspace.report_metadata");
        assert_eq!(report["params"]["tokens"]["lineup_repo"], "—");
    }

    #[test]
    fn logo_indent_counts_unidentified_panes_and_preserves_null_logos() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let request = receive(&mut stream);
            let panes: Vec<_> = [
                ("w1:p9", None),
                ("w1:p2", Some("kimi")),
                ("w1:p1", Some("unknown")),
            ]
            .iter()
            .map(|(pane, agent)| {
                json!({"pane_id":pane,"workspace_id":"w1",
                    "agent":agent,"agent_status":"idle"})
            })
            .collect();
            writeln!(
                stream.get_mut(),
                "{}",
                json!({"id":request["id"],"result":{
                    "type":"session_snapshot","snapshot":{"version":"0.9.3","protocol":22,
                    "panes":panes,"workspaces":[{"workspace_id":"w1"}]}
                }})
            )
            .unwrap();
            let mut reports = BTreeMap::new();
            for _ in 0..4 {
                let report = receive(&mut stream);
                writeln!(
                    stream.get_mut(),
                    "{}",
                    json!({"id":report["id"],"result":{"type":"ok"}})
                )
                .unwrap();
                if let Some(pane) = report["params"]["pane_id"].as_str() {
                    reports.insert(pane.to_owned(), report["params"]["tokens"].clone());
                }
            }
            reports
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        publisher
            .refresh(
                &mut transport,
                &Config {
                    logo_mode: "font".into(),
                    non_first_pane_logo_indent: 16,
                    ..Default::default()
                },
            )
            .unwrap();
        let reports = server.join().unwrap();
        assert_eq!(
            reports["w1:p2"]["lineup_logo"],
            format!("{}\u{e1a6}", "\u{2800}".repeat(16))
        );
        for pane in ["w1:p9", "w1:p1"] {
            assert!(
                reports[pane]
                    .as_object()
                    .unwrap()
                    .values()
                    .all(Value::is_null)
            );
        }
    }

    // Each ordinary Herdr JSON request uses a separate accepted connection.
    // Keep scenario logic sequential while exercising the real socket lifetime.
    struct OneShotServer {
        listener: UnixListener,
        stream: BufReader<std::os::unix::net::UnixStream>,
        read: bool,
        timeout: Duration,
    }
    impl OneShotServer {
        fn new(listener: UnixListener, timeout: Duration) -> Self {
            let (stream, _) = listener.accept().unwrap();
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(timeout)).unwrap();
            listener.set_nonblocking(true).unwrap();
            Self {
                listener,
                stream: BufReader::new(stream),
                read: false,
                timeout,
            }
        }
        fn read_line(&mut self, line: &mut String) -> io::Result<usize> {
            if self.read {
                let deadline = Instant::now() + self.timeout;
                loop {
                    match self.listener.accept() {
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false)?;
                            stream.set_read_timeout(Some(self.timeout))?;
                            self.stream = BufReader::new(stream);
                            break;
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            if Instant::now() >= deadline {
                                return Err(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "no next request",
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            self.read = true;
            self.stream.read_line(line)
        }
        fn get_mut(&mut self) -> ClosingResponse<'_> {
            ClosingResponse(self.stream.get_mut())
        }
    }

    struct ClosingResponse<'a>(&'a mut std::os::unix::net::UnixStream);
    impl Write for ClosingResponse<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let count = self.0.write(bytes)?;
            if bytes[..count].contains(&b'\n') {
                self.0.shutdown(std::net::Shutdown::Both)?;
            }
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    #[test]
    fn shutdown_during_reader_sends_no_refresh_metadata() {
        struct StoppingReader(Arc<AtomicBool>);
        impl UsageReader for StoppingReader {
            fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
                self.0.store(true, Ordering::Release);
                TimeoutReader.read(request)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("state")).unwrap();
        let socket = dir.path().join("cancel.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_millis(300));
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            writeln!(stream.get_mut(), "{}", json!({"id":request["id"],"result":{
                "type":"session_snapshot","snapshot":{"version":"0.9.3","protocol":22,
                "workspaces":[{"workspace_id":"w1"}],"panes":[{
                "pane_id":"w1:p1","workspace_id":"w1","agent":"codex",
                "agent_session":{"agent":"codex","kind":"id","value":"synthetic-native"},"agent_status":"idle"}]}
            }})).unwrap();
            let mut reports = 0;
            loop {
                line.clear();
                if stream.read_line(&mut line).is_err() {
                    break;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                reports += 1;
                writeln!(
                    stream.get_mut(),
                    "{}",
                    json!({"id":request["id"],"result":{"type":"ok"}})
                )
                .unwrap();
            }
            reports
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().join("state"),
            "fake".into(),
            Homes {
                codex: Some(dir.path().join("synthetic-home")),
                ..Homes::default()
            },
        )
        .unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        publisher.set_shutdown(flag.clone());
        let result = publisher.refresh_with_readers(
            &mut transport,
            &Config::default(),
            &StoppingReader(flag.clone()),
            &TimeoutReader,
        );
        let reports = server.join().unwrap();
        assert!(flag.load(Ordering::Acquire), "fixture did not reach reader");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(reports, 0, "shutdown must not publish a partial refresh");
    }

    #[test]
    fn shutdown_during_stalled_reader_returns_before_reader_timeout() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        let dir = tempfile::tempdir().unwrap();
        let identity = SessionIdentity {
            provider: Provider::Codex,
            native_session_id: "session-1".into(),
            repository_identity: "repo-1".into(),
        };
        let timeout = Duration::from_millis(1200);
        let shutdown = Arc::new(AtomicBool::new(false));
        let request = ReadRequest {
            identity: &identity,
            home: dir.path(),
            now: UNIX_EPOCH,
            max_age: Duration::from_secs(60),
            timeout,
            shutdown: Some(&shutdown),
        };
        let entered = Arc::new(AtomicBool::new(false));
        let entered_job = entered.clone();
        let (release, blocked) = std::sync::mpsc::channel();
        let flag = shutdown.clone();
        let watcher = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !entered.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline, "stalled reader did not start");
                std::thread::sleep(Duration::from_millis(5));
            }
            flag.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let result = bounded_read(&request, SourceKind::CodexRollout, &BUSY, move |request| {
            entered_job.store(true, Ordering::Release);
            let _ = blocked.recv();
            UsageReading::sample(request, SourceKind::CodexRollout, 7.0, request.now)
        });
        let elapsed = started.elapsed();
        let _ = release.send(());
        watcher.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while BUSY.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            elapsed < Duration::from_millis(400),
            "shutdown during a stalled reader waited {elapsed:?} of the {timeout:?} reader timeout"
        );
        assert_eq!(
            result.context,
            UsageOutcome::Unavailable(MissingReason::Timeout)
        );
    }

    #[test]
    fn shutdown_during_reader_then_clear_fits_in_stop_deadline() {
        struct StalledReader {
            entered: Arc<AtomicBool>,
            release: Arc<AtomicBool>,
        }
        impl UsageReader for StalledReader {
            fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
                static BUSY: AtomicBool = AtomicBool::new(false);
                let entered = self.entered.clone();
                let release = self.release.clone();
                bounded_read(request, SourceKind::CodexRollout, &BUSY, move |request| {
                    entered.store(true, Ordering::Release);
                    let give_up = Instant::now() + Duration::from_secs(30);
                    while !release.load(Ordering::Acquire) && Instant::now() < give_up {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    UsageReading::missing(request, SourceKind::CodexRollout, MissingReason::Timeout)
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("stop-budget.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(2));
            let mut line = String::new();
            while stream.read_line(&mut line).is_ok() && !line.is_empty() {
                let request: Value = serde_json::from_str(&line).unwrap();
                let response = if request["method"] == "session.snapshot" {
                    json!({"id":request["id"],"result":{
                        "type":"session_snapshot","snapshot":{"version":"0.9.3","protocol":22,
                        "workspaces":[{"workspace_id":"w1"}],"panes":[{
                        "pane_id":"w1:p1","workspace_id":"w1","agent":"codex",
                        "agent_session":{"agent":"codex","kind":"id","value":"synthetic-native"},
                        "agent_status":"idle"}]}
                    }})
                } else {
                    json!({"id":request["id"],"result":{"type":"ok"}})
                };
                if writeln!(stream.get_mut(), "{response}").is_err() {
                    break;
                }
                line.clear();
            }
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().join("state"),
            "fake".into(),
            Homes {
                codex: Some(dir.path().join("synthetic-home")),
                ..Homes::default()
            },
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("state")).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        publisher.set_shutdown(shutdown.clone());
        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let reader = StalledReader {
            entered: entered.clone(),
            release: release.clone(),
        };
        let (signaled_tx, signaled_rx) = std::sync::mpsc::channel();
        let flag = shutdown.clone();
        let watcher = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !entered.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline, "reader did not start");
                std::thread::sleep(Duration::from_millis(5));
            }
            signaled_tx.send(Instant::now()).unwrap();
            flag.store(true, Ordering::Release);
        });
        let config = Config {
            refresh_interval_ms: 30000,
            reader_timeout_ms: 30000,
            usage_max_age_ms: 30000,
            providers: vec!["codex".into()],
            ..Config::default()
        };
        let refresh =
            publisher.refresh_with_readers(&mut transport, &config, &reader, &TimeoutReader);
        let signaled = signaled_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let reader_wait = signaled.elapsed();
        release.store(true, Ordering::Release);
        watcher.join().unwrap();
        assert_eq!(refresh.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(
            reader_wait < Duration::from_millis(1500),
            "shutdown during a 30000 ms reader waited {reader_wait:?}"
        );
        // Clear's own budget is three seconds. Stop allows five. The reader
        // wait must leave that clear budget inside the stop deadline.
        let clear_budget = Duration::from_secs(3);
        assert!(
            reader_wait + clear_budget < Duration::from_secs(5),
            "reader shutdown wait {reader_wait:?} plus clear budget does not fit in five seconds"
        );
        let clear_started = Instant::now();
        publisher.clear(&mut transport).unwrap();
        assert!(clear_started.elapsed() < clear_budget);
        assert!(
            signaled.elapsed() < Duration::from_secs(5),
            "shutdown through clear took {:?}",
            signaled.elapsed()
        );
        drop(transport);
        server.join().unwrap();
    }

    struct TimeoutReader;
    impl UsageReader for TimeoutReader {
        fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
            UsageReading::missing(request, SourceKind::CodexRollout, MissingReason::Timeout)
        }
    }

    struct EntryCapReader {
        root: PathBuf,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl UsageReader for EntryCapReader {
        fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
            let call = self.calls.fetch_add(1, Ordering::Relaxed);
            if call == 0 {
                return UsageReading::sample(request, SourceKind::CodexRollout, 42.0, request.now);
            }
            let mut budget = Budget::new(request);
            match budget.files(&self.root, 0, |_| true) {
                Ok(paths) if paths.len() == MAX_DIRECTORY_ENTRIES => {
                    UsageReading::sample(request, SourceKind::CodexRollout, 7.0, request.now)
                }
                Ok(_) => {
                    UsageReading::missing(request, SourceKind::CodexRollout, MissingReason::Invalid)
                }
                Err(reason) => UsageReading::missing(request, SourceKind::CodexRollout, reason),
            }
        }
    }

    #[test]
    fn entry_cap_boundary_does_not_keep_previous_context() {
        let dir = tempfile::tempdir().unwrap();
        let entries = dir.path().join("entries");
        std::fs::create_dir(&entries).unwrap();
        for index in 0..MAX_DIRECTORY_ENTRIES {
            std::fs::write(entries.join(format!("e{index:04}")), b"").unwrap();
        }
        let socket = dir.path().join("fake.sock");
        let server = token_server(
            UnixListener::bind(&socket).unwrap(),
            Arc::new(std::sync::atomic::AtomicU64::new(0)),
        );
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().into(),
            "fake".into(),
            Homes {
                codex: Some(dir.path().join("empty-home")),
                ..Homes::default()
            },
        )
        .unwrap();
        let reader = EntryCapReader {
            root: entries.clone(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let config = Config {
            refresh_interval_ms: 5000,
            reader_timeout_ms: 5000,
            ..Config::default()
        };
        let context = |publisher: &mut MetadataPublisher, transport: &mut Transport| {
            let (_, response) = publisher.request(transport, "session.snapshot").unwrap();
            response["result"]["snapshot"]["panes"][0]["tokens"]["lineup_context_text"].clone()
        };
        publisher
            .refresh_with_readers(&mut transport, &config, &reader, &TimeoutReader)
            .unwrap();
        assert_eq!(context(&mut publisher, &mut transport), "42%");
        publisher
            .refresh_with_readers(&mut transport, &config, &reader, &TimeoutReader)
            .unwrap();
        assert_eq!(context(&mut publisher, &mut transport), "07%");
        std::fs::write(entries.join("extra"), b"").unwrap();
        publisher
            .refresh_with_readers(&mut transport, &config, &reader, &TimeoutReader)
            .unwrap();
        assert_eq!(context(&mut publisher, &mut transport), "—\u{2800}\u{2800}");
        publisher.request(&mut transport, "test.finish").unwrap();
        server.join().unwrap();
    }

    struct SampleReader;
    impl UsageReader for SampleReader {
        fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
            UsageReading::sample(request, SourceKind::CodexRollout, 42.0, request.now)
        }
    }

    fn snapshot(id: &Value, agent: bool) -> Value {
        json!({"id": id, "result": {"type": "session_snapshot", "snapshot": {
            "version": "0.9.3", "protocol": 17,
            "workspaces": [{"workspace_id": "w1", "tokens": {}}],
            "panes": [{"pane_id": "w1:p1", "workspace_id": "w1",
                "agent": if agent { Some("codex") } else { None },
                "agent_session": {"agent":"codex", "kind":"id", "value":"synthetic-session"},
                "agent_status":"working", "tokens":{"lineup_context_pct":"old"}}]
        }}})
    }

    #[test]
    fn checkpoint_is_loaded_after_construction_before_first_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let request = receive(&mut stream);
            writeln!(stream.get_mut(), "{}", snapshot(&request["id"], true)).unwrap();
            let mut reports = Vec::new();
            for _ in 0..2 {
                let request = receive(&mut stream);
                writeln!(
                    stream.get_mut(),
                    "{}",
                    json!({"id":request["id"],"result":{"type":"ok"}})
                )
                .unwrap();
                reports.push(request);
            }
            reports
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        // Model an older daemon completing its final reservation before our
        // caller obtains the session lock. Construction must not cache old seq.
        let identity = transport.identity();
        std::fs::write(
            dir.path().join("sequences.json"),
            serde_json::to_vec(&vec![CheckpointRow {
                endpoint: format!("fake:{}:{}", identity.device, identity.inode),
                kind: "pane".into(),
                id: "w1:p1".into(),
                seq: 40,
            }])
            .unwrap(),
        )
        .unwrap();
        publisher
            .refresh(&mut transport, &crate::config::Config::default())
            .unwrap();
        let reports = server.join().unwrap();
        assert_eq!(reports[0]["params"]["seq"], 41);
    }

    #[test]
    fn checkpoint_pruning_persists_floor_and_drops_replaced_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = "fake:1:2";
        let rows = vec![
            CheckpointRow {
                endpoint: endpoint.into(),
                kind: "pane".into(),
                id: "w1:p2".into(),
                seq: 17,
            },
            CheckpointRow {
                endpoint: "fake:old:socket".into(),
                kind: "pane".into(),
                id: "w1:p3".into(),
                seq: 99,
            },
        ];
        std::fs::write(
            dir.path().join("sequences.json"),
            serde_json::to_vec(&rows).unwrap(),
        )
        .unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        publisher.load_checkpoint().unwrap();
        publisher.endpoint = Some(endpoint.into());
        let current =
            protocol::parse_snapshot("snapshot", &snapshot(&json!("snapshot"), true)).unwrap();
        publisher.reconcile_missing(&current).unwrap();

        let rows: Vec<CheckpointRow> =
            serde_json::from_slice(&std::fs::read(dir.path().join("sequences.json")).unwrap())
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "endpoint");
        assert_eq!(rows[0].endpoint, endpoint);
        assert_eq!(rows[0].seq, 17);

        let mut restarted =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        restarted.load_checkpoint().unwrap();
        let resource = ResourceId {
            endpoint_session: endpoint.into(),
            kind: ResourceKind::Pane,
            id: "w1:p2".into(),
        };
        let report = restarted
            .planner
            .prepare(
                &resource,
                &tokens::ResourceSnapshot {
                    exists: true,
                    stored_keys: BTreeSet::new(),
                },
                None,
                &BTreeMap::from([("lineup_agent".into(), Some("codex".into()))]),
                0,
                5000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(report.seq, 18);
    }

    #[test]
    fn one_report_persists_the_reserved_sequence_once() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(2));
            for _ in 0..2 {
                let request = receive(&mut stream);
                writeln!(
                    stream.get_mut(),
                    "{}",
                    json!({"id": request["id"], "result": {"type": "ok"}})
                )
                .unwrap();
            }
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        let resource = ResourceId {
            endpoint_session: "endpoint".into(),
            kind: ResourceKind::Pane,
            id: "w1:p1".into(),
        };
        let desired = BTreeMap::from([("lineup_agent".into(), Some("codex".into()))]);
        publisher
            .report(
                &mut transport,
                &resource,
                &BTreeSet::new(),
                None,
                &desired,
                None,
            )
            .unwrap();
        assert_eq!(publisher.checkpoint_persists.load(Ordering::Relaxed), 1);
        let reserved = std::fs::read(dir.path().join("sequences.json")).unwrap();
        let rows: Vec<CheckpointRow> = serde_json::from_slice(&reserved).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].seq, 1);
        // Acceptance does not move the high-water mark, so rewriting the
        // checkpoint after acknowledge stores the same bytes.
        publisher.persist().unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("sequences.json")).unwrap(),
            reserved
        );

        let mut restarted =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        restarted.load_checkpoint().unwrap();
        assert_eq!(restarted.planner.checkpoint().get(&resource), Some(&1));
        restarted
            .report(
                &mut transport,
                &resource,
                &BTreeSet::new(),
                None,
                &desired,
                None,
            )
            .unwrap();
        assert_eq!(restarted.checkpoint_persists.load(Ordering::Relaxed), 1);
        let rows: Vec<CheckpointRow> =
            serde_json::from_slice(&std::fs::read(dir.path().join("sequences.json")).unwrap())
                .unwrap();
        assert_eq!(rows[0].seq, 2);
        server.join().unwrap();
    }

    #[test]
    fn clear_rechecks_budget_after_reservation_before_sending() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let mut transport = Transport::connect(&socket).unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut stream = BufReader::new(stream);
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        let resource = ResourceId {
            endpoint_session: "fake".into(),
            kind: ResourceKind::Pane,
            id: "w1:p1".into(),
        };
        let desired = BTreeMap::from([("lineup_agent".into(), None)]);
        // Reproduce the budget remaining after slow checkpoint persistence.
        let result = publisher.report(
            &mut transport,
            &resource,
            &BTreeSet::new(),
            None,
            &desired,
            Some(Instant::now() + Duration::from_millis(900)),
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        let rows: Vec<CheckpointRow> =
            serde_json::from_slice(&std::fs::read(dir.path().join("sequences.json")).unwrap())
                .unwrap();
        assert_eq!(rows[0].seq, 1);
        let mut line = String::new();
        assert!(stream.read_line(&mut line).is_err());
        assert!(line.is_empty());
    }

    #[test]
    fn stalled_clear_response_times_out_without_replaying_the_report() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(2));
            let request = receive(&mut stream);
            let mut response = snapshot(&request["id"], false);
            response["result"]["snapshot"]["panes"] = json!((0..9).map(|i| json!({
                "pane_id":format!("w1:p{i}"),"workspace_id":"w1","agent_status":"idle","tokens":{}
            })).collect::<Vec<_>>());
            writeln!(stream.get_mut(), "{response}").unwrap();
            let report = receive(&mut stream);
            assert_eq!(report["method"], "pane.report_metadata");
            // Hold the acknowledgement until clear actually finishes. No
            // sleep races a 500 ms frame deadline or writes after peer close.
            let error_kind = completed_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            let mut trailing = String::new();
            assert_eq!(stream.stream.read_line(&mut trailing).unwrap(), 0);
            assert!(trailing.is_empty());
            assert_eq!(
                stream.listener.accept().unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            error_kind
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        let started = Instant::now();
        let error = publisher.clear(&mut transport).unwrap_err();
        drop(transport);
        completed_tx.send(error.kind()).unwrap();
        assert_eq!(server.join().unwrap(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
        let rows: Vec<CheckpointRow> =
            serde_json::from_slice(&std::fs::read(dir.path().join("sequences.json")).unwrap())
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].seq, 1);
    }

    #[test]
    fn clear_reserves_time_for_the_last_socket_round_trip() {
        // Model the remaining budget directly. A fixed slow-response sleep
        // leaves too little scheduling margin on loaded CI machines.
        let now = Instant::now();
        assert_eq!(
            check_clear_deadline(now + Duration::from_millis(900))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(check_clear_deadline(now).is_err());
    }

    fn assert_daemon_child_stays_closed(
        listener: &UnixListener,
        command: &mut std::process::Command,
    ) {
        listener.set_nonblocking(true).unwrap();
        let output = command.output().unwrap();
        assert!(
            !output.status.success(),
            "metadata daemon child must fail before opening a socket"
        );
        match listener.accept() {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Ok(_) => panic!("metadata daemon child opened the inherited socket"),
            Err(error) => panic!("fake socket accept failed: {error}"),
        }
    }

    #[test]
    fn metadata_daemon_child_refuses_inherited_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let state = dir.path().join("state");
        let config = dir.path().join("config");
        std::fs::create_dir(&state).unwrap();
        std::fs::create_dir(&config).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "publish::tests::metadata_daemon_child",
                "--ignored",
                "--nocapture",
            ])
            .env("HERDR_SOCKET_PATH", &socket)
            .env("HERDR_PLUGIN_STATE_DIR", &state)
            .env("HERDR_PLUGIN_CONFIG_DIR", &config)
            .env("HERDR_SESSION", "main")
            .env_remove(crate::test_support::ISOLATION_ROOT);
        assert_daemon_child_stays_closed(&listener, &mut command);
        assert!(std::fs::read_dir(&state).unwrap().next().is_none());
    }

    #[test]
    fn metadata_daemon_child_refuses_socket_outside_isolation_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let socket = outside.path().join("fake.sock");
        let state = root.path().join("state");
        let config = root.path().join("config");
        std::fs::create_dir(&state).unwrap();
        std::fs::create_dir(&config).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let link = root.path().join("escaped.sock");
        std::os::unix::fs::symlink(&socket, &link).unwrap();
        let mut command =
            crate::test_support::isolated_daemon_command(root.path(), &link, &state, &config);
        command.args([
            "--exact",
            "publish::tests::metadata_daemon_child",
            "--ignored",
            "--nocapture",
        ]);
        assert_daemon_child_stays_closed(&listener, &mut command);
    }

    #[test]
    #[ignore = "isolated metadata daemon subprocess entry point"]
    fn metadata_daemon_child() {
        let options = crate::daemon::Options::from_env().unwrap();
        // Fail closed before any session connection when the inherited
        // environment is not the explicit isolated root.
        crate::test_support::require_isolated_runtime(&options).unwrap();
        let paths = crate::lock::RuntimePaths::new(&options.state, &options.socket).unwrap();
        // Missing homes exercise unavailable usage without reading real data.
        let mut publisher =
            MetadataPublisher::new(paths.root, paths.key, Homes::default()).unwrap();
        crate::daemon::run_foreground(&options, false, &mut publisher).unwrap();
    }

    #[test]
    fn daemon_hook_publishes_missing_usage_heading_and_clears_on_stop() {
        let dir = tempfile::tempdir().unwrap();
        let options = crate::daemon::Options {
            socket: dir.path().join("fake.sock"),
            state: dir.path().join("state"),
            config: dir.path().join("config"),
        };
        let repo = dir.path().join("synthetic-repository");
        std::fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap())
                .env("HOME", dir.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
                .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
                .output()
                .unwrap();
            assert!(output.status.success());
        };
        git(&["init", "-q"]);
        git(&[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-qm",
            "Synthetic fixture",
        ]);
        git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let listener = UnixListener::bind(&options.socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let mut reports = Vec::new();
            loop {
                let mut line = String::new();
                if stream.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                let response = match request["method"].as_str().unwrap() {
                    "ping" => {
                        json!({"id":request["id"],"result":{"type":"pong","version":"0.9.3","protocol":17}})
                    }
                    "session.snapshot" => {
                        let mut response = snapshot(&request["id"], true);
                        response["result"]["snapshot"]["workspaces"][0]["worktree"] =
                            json!({"checkout_path":repo,"repo_root":repo});
                        response["result"]["snapshot"]["panes"][0]["cwd"] =
                            json!("/nonexistent-synthetic-pane");
                        response
                    }
                    "pane.report_metadata" | "workspace.report_metadata" => {
                        reports.push(request.clone());
                        json!({"id":request["id"],"result":{"type":"ok"}})
                    }
                    _ => panic!("unexpected synthetic request"),
                };
                writeln!(stream.get_mut(), "{response}").unwrap();
            }
            reports
        });
        let paths = crate::lock::RuntimePaths::new(&options.state, &options.socket).unwrap();
        let mut command = crate::test_support::isolated_daemon_command(
            dir.path(),
            &options.socket,
            &options.state,
            &options.config,
        );
        let mut daemon = command
            .args([
                "--exact",
                "publish::tests::metadata_daemon_child",
                "--ignored",
                "--nocapture",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if crate::daemon::probe(&paths, &options.socket)
                .ok()
                .flatten()
                .is_some_and(|health| health.last_refresh_ms.is_some())
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        crate::daemon::stop(&options).unwrap();
        assert!(daemon.wait().unwrap().success());
        let reports = server.join().unwrap();
        assert_eq!(reports.len(), 4);
        assert_eq!(
            reports[0]["params"]["tokens"]["lineup_context_text"],
            "—\u{2800}\u{2800}"
        );
        assert_eq!(
            reports[1]["params"]["tokens"]["lineup_repo"],
            "synthetic-repository"
        );
        assert_eq!(
            reports[1]["params"]["tokens"]["lineup_ship_unix"],
            "1767225600"
        );
        assert!(reports[2..].iter().all(|request| {
            request["params"]["tokens"]
                .as_object()
                .unwrap()
                .values()
                .all(Value::is_null)
        }));
        assert!(paths.root.join("sequences.json").exists());
    }

    #[test]
    fn rejected_report_burns_sequence_and_clear_preserves_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let mut reports = Vec::new();
            for cycle in 0..3 {
                let request = receive(&mut stream);
                let response = snapshot(&request["id"], true);
                writeln!(stream.get_mut(), "{response}").unwrap();
                for _ in 0..if cycle == 0 { 1 } else { 2 } {
                    let request = receive(&mut stream);
                    let response = if cycle == 0 {
                        json!({"id":request["id"],"error":{"code":"rejected","message":"synthetic rejection"}})
                    } else {
                        json!({"id":request["id"],"result":{"type":"ok"}})
                    };
                    writeln!(stream.get_mut(), "{response}").unwrap();
                    reports.push(request);
                }
            }
            reports
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        let config = crate::config::Config::default();
        assert!(publisher.refresh(&mut transport, &config).is_err());
        publisher.refresh(&mut transport, &config).unwrap();
        publisher.clear(&mut transport).unwrap();
        let reports = server.join().unwrap();
        assert_eq!(reports[0]["params"]["seq"], 1);
        assert_eq!(reports[1]["params"]["seq"], 2);
        assert_eq!(reports[3]["params"]["seq"], 3);
        for request in &reports[3..] {
            let tokens = request["params"]["tokens"].as_object().unwrap();
            assert!(tokens.values().all(Value::is_null));
            assert!(tokens.keys().all(|key| key.starts_with("lineup_")));
        }
    }

    #[test]
    fn failed_reservation_persistence_sends_no_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_millis(100));
            let request = receive(&mut stream);
            writeln!(stream.get_mut(), "{}", snapshot(&request["id"], true)).unwrap();
            let mut line = String::new();
            assert!(stream.read_line(&mut line).is_err());
            assert!(line.is_empty());
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().join("missing-state"),
            "fake".into(),
            Homes::default(),
        )
        .unwrap();
        let result = publisher.refresh(&mut transport, &crate::config::Config::default());
        assert_eq!(capacity_skip_count(result.as_ref().unwrap_err()), None);
        server.join().unwrap();
    }

    #[test]
    fn crowded_snapshot_publishes_visible_fields_and_clears_old_optional_fields() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(2));
            let request = receive(&mut stream);
            let mut response = snapshot(&request["id"], true);
            let pane = &mut response["result"]["snapshot"]["panes"][0];
            pane["cwd"] = json!("/nonexistent-synthetic-repo");
            // Names and source prefixes mirror snapshot metadata; no runtime
            // values or session contents are copied into this fixture.
            let mut stored: serde_json::Map<String, Value> = (0..28)
                .map(|i| (format!("quota_fixture_{i}"), json!("synthetic")))
                .collect();
            for key in ["lineup_context_pct", "lineup_agent", "lineup_state"] {
                stored.insert(key.into(), json!("old-synthetic"));
            }
            pane["tokens"] = Value::Object(stored);
            writeln!(stream.get_mut(), "{response}").unwrap();
            let report = receive(&mut stream);
            assert_eq!(report["method"], "pane.report_metadata");
            let tokens = report["params"]["tokens"].as_object().unwrap();
            assert_eq!(tokens["lineup_context_text"], "42%");
            for key in ["lineup_logo", "lineup_state_working", "lineup_weekly_text"] {
                assert!(tokens[key].is_string());
            }
            for key in ["lineup_context_pct", "lineup_agent", "lineup_state"] {
                assert!(tokens[key].is_null());
            }
            assert!(tokens.keys().all(|key| key.starts_with("lineup_")));
            assert_eq!(tokens.values().filter(|value| value.is_string()).count(), 4);
            writeln!(
                stream.get_mut(),
                "{}",
                json!({"id":report["id"],"result":{"type":"ok"}})
            )
            .unwrap();
            let workspace = receive(&mut stream);
            assert_eq!(workspace["method"], "workspace.report_metadata");
            writeln!(
                stream.get_mut(),
                "{}",
                json!({"id":workspace["id"],"result":{"type":"ok"}})
            )
            .unwrap();
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().into(),
            "fake".into(),
            Homes {
                codex: Some(dir.path().into()),
                ..Default::default()
            },
        )
        .unwrap();
        publisher
            .refresh_with_readers(
                &mut transport,
                &Config::default(),
                &SampleReader,
                &TimeoutReader,
            )
            .unwrap();
        server.join().unwrap();
    }
    #[test]
    fn capacity_failure_skips_pane_and_still_publishes_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(2));
            let request = receive(&mut stream);
            let mut response = snapshot(&request["id"], true);
            response["result"]["snapshot"]["panes"][0]["tokens"] = Value::Object(
                (0..32)
                    .map(|i| (format!("other_{i}"), json!("preserve")))
                    .collect(),
            );
            writeln!(stream.get_mut(), "{response}").unwrap();
            let request = receive(&mut stream);
            assert_eq!(request["method"], "workspace.report_metadata");
            writeln!(
                stream.get_mut(),
                "{}",
                json!({"id":request["id"],"result":{"type":"ok"}})
            )
            .unwrap();
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher =
            MetadataPublisher::new(dir.path().into(), "fake".into(), Homes::default()).unwrap();
        let result = publisher.refresh(&mut transport, &crate::config::Config::default());
        assert_eq!(capacity_skip_count(result.as_ref().unwrap_err()), Some(1));
        let mut health = crate::daemon::Health {
            plugin: crate::daemon::PLUGIN_ID.into(),
            socket_path: socket,
            socket_key: "fake".into(),
            endpoint: transport.identity(),
            nonce: "test".into(),
            pid: std::process::id(),
            last_refresh_ms: None,
            refresh_failed: false,
            capacity_skipped_resources: 0,
        };
        crate::daemon::record_refresh(&mut health, &result, 7);
        assert_eq!(crate::doctor::refresh_diagnostic(&health).unwrap(), 7);
        assert_eq!(
            crate::doctor::capacity_diagnostic(&health).unwrap(),
            "Metadata capacity exhausted: 1 resource skipped"
        );
        server.join().unwrap();
    }

    trait RequestReader {
        fn request_line(&mut self, line: &mut String) -> io::Result<usize>;
    }
    impl RequestReader for OneShotServer {
        fn request_line(&mut self, line: &mut String) -> io::Result<usize> {
            self.read_line(line)
        }
    }
    impl RequestReader for BufReader<std::os::unix::net::UnixStream> {
        fn request_line(&mut self, line: &mut String) -> io::Result<usize> {
            self.read_line(line)
        }
    }
    fn receive(stream: &mut impl RequestReader) -> Value {
        let mut line = String::new();
        stream.request_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[test]
    fn fake_socket_publishes_statusline_context_weekly_and_reset() {
        let _guard = crate::statusline::READER_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let observations = dir.path().join("observations");
        std::fs::create_dir_all(observations.join("statusline")).unwrap();
        let reset = unix(SystemTime::now()) + 86400;
        for (agent, key) in [
            (crate::statusline::Agent::Claude, "synthetic-session"),
            (crate::statusline::Agent::Agy, "w1:p1"),
        ] {
            let record = crate::statusline::Record {
                version: 1,
                agent,
                key: key.into(),
                observed_ms: (unix(SystemTime::now()) as u64) * 1000,
                context: Some(23.0),
                weekly_remaining: Some(73.0),
                weekly_reset_ms: Some((reset as u64) * 1000),
            };
            std::fs::write(
                crate::statusline::record_path(&observations, agent, key),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let mut stored = BTreeMap::new();
            let mut reports = Vec::new();
            for (agent, native) in [
                ("claude", Some("synthetic-session")),
                ("agy", None),
                ("claude", Some("different")),
            ] {
                let req = receive(&mut stream);
                let mut response = snapshot(&req["id"], true);
                let pane = &mut response["result"]["snapshot"]["panes"][0];
                pane["agent"] = json!(agent);
                pane["agent_session"] = native
                    .map(|id| json!({"agent":agent,"kind":"id","value":id}))
                    .unwrap_or(Value::Null);
                let response = with_stored_tokens(response, &stored);
                writeln!(stream.get_mut(), "{response}").unwrap();
                let report = receive(&mut stream);
                assert_eq!(report["method"], "pane.report_metadata");
                writeln!(
                    stream.get_mut(),
                    "{}",
                    json!({"id":report["id"],"result":{"type":"ok"}})
                )
                .unwrap();
                remember_tokens(&mut stored, &report);
                reports.push(report);
                if reports.len() == 1 {
                    let report = receive(&mut stream);
                    remember_tokens(&mut stored, &report);
                    writeln!(
                        stream.get_mut(),
                        "{}",
                        json!({"id":report["id"],"result":{"type":"ok"}})
                    )
                    .unwrap();
                }
            }
            reports
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let mut publisher = MetadataPublisher::new(
            dir.path().into(),
            "fake".into(),
            Homes {
                statusline_state: Some(observations),
                ..Homes::default()
            },
        )
        .unwrap();
        for _ in 0..3 {
            publisher
                .refresh(&mut transport, &Config::default())
                .unwrap();
        }
        let reports = server.join().unwrap();
        for report in &reports[..2] {
            let t = &report["params"]["tokens"];
            assert_eq!(t["lineup_context_pct"], "23");
            assert_eq!(t["lineup_weekly_pct"], "73");
            assert!(
                t["lineup_weekly_text"]
                    .as_str()
                    .unwrap()
                    .starts_with("7d 73% ")
            );
        }
        let t = &reports[2]["params"]["tokens"];
        assert_eq!(t["lineup_context_text"], "—\u{2800}\u{2800}");
        assert_eq!(t["lineup_weekly_text"], "—");
        assert!(t["lineup_weekly_pct"].is_null());
    }

    #[test]
    fn fake_socket_publishes_clears_and_restores_durable_sequences() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let state = dir.path().join("state");
        std::fs::create_dir(&state).unwrap();
        let home = dir.path().join("empty-home");
        std::fs::create_dir(&home).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let checkpoint = state.join("sequences.json");
        let server_checkpoint = checkpoint.clone();
        let server = std::thread::spawn(move || {
            let mut stream = OneShotServer::new(listener, Duration::from_secs(5));
            let mut stored = BTreeMap::new();
            let mut reports = Vec::new();
            for cycle in 0..3 {
                let mut line = String::new();
                stream.read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], "session.snapshot");
                let response = with_stored_tokens(snapshot(&request["id"], cycle != 1), &stored);
                writeln!(stream.get_mut(), "{response}").unwrap();
                let count = if cycle == 1 { 1 } else { 2 };
                for _ in 0..count {
                    line.clear();
                    stream.read_line(&mut line).unwrap();
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let rows: Vec<CheckpointRow> =
                        serde_json::from_slice(&std::fs::read(&server_checkpoint).unwrap())
                            .unwrap();
                    assert!(rows.iter().any(|row| row.id
                        == request["params"][if request["method"] == "pane.report_metadata" {
                            "pane_id"
                        } else {
                            "workspace_id"
                        }]
                        && row.seq == request["params"]["seq"]));
                    let response = json!({"id":request["id"], "result":{"type":"ok"}});
                    writeln!(stream.get_mut(), "{response}").unwrap();
                    remember_tokens(&mut stored, &request);
                    reports.push(request);
                }
            }
            reports
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let homes = Homes {
            codex: Some(home.clone()),
            grok: Some(home),
            statusline_state: None,
        };
        let mut publisher =
            MetadataPublisher::new(state.clone(), "fake-socket".into(), homes.clone()).unwrap();
        let config = crate::config::Config::default();
        publisher
            .refresh_with_readers(&mut transport, &config, &SampleReader, &TimeoutReader)
            .unwrap();
        publisher
            .refresh_with_readers(&mut transport, &config, &TimeoutReader, &TimeoutReader)
            .unwrap();
        let mut restarted = MetadataPublisher::new(state, "fake-socket".into(), homes).unwrap();
        restarted
            .refresh_with_readers(&mut transport, &config, &TimeoutReader, &TimeoutReader)
            .unwrap();
        let reports = server.join().unwrap();
        assert_eq!(reports[0]["params"]["tokens"]["lineup_agent"], "codex");
        assert_eq!(reports[0]["params"]["tokens"]["lineup_context_text"], "42%");
        assert_eq!(reports[0]["params"]["tokens"]["lineup_context_pct"], "42");
        assert!(reports[3]["params"]["tokens"]["lineup_context_pct"].is_null());
        assert_eq!(
            reports[3]["params"]["tokens"]["lineup_context_text"],
            "—\u{2800}\u{2800}"
        );
        assert_eq!(reports[1]["method"], "workspace.report_metadata");
        assert_eq!(
            reports[2]["params"]["tokens"].as_object().unwrap().len(),
            14
        );
        assert!(
            reports[2]["params"]["tokens"]
                .as_object()
                .unwrap()
                .values()
                .all(Value::is_null)
        );
        assert_eq!(reports[3]["params"]["seq"], 3);
        assert_eq!(reports[4]["params"]["seq"], 2);
    }
}
