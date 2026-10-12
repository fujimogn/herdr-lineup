//! Durable, session-scoped reconciliation from snapshots to owned metadata.

use crate::{
    config::Config,
    daemon,
    model::{self, UsageReader},
    protocol,
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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Default)]
pub struct Homes {
    pub codex: Option<PathBuf>,
    pub grok: Option<PathBuf>,
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
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointRow {
    endpoint: String,
    kind: String,
    id: String,
    seq: u64,
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
        })
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
                for row in rows {
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
                checkpoint
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
            Err(_) => return Err(io::Error::other("cannot read sequence checkpoint")),
        };
        self.planner = tokens::Publisher::restore(checkpoint);
        self.checkpoint_loaded = true;
        Ok(())
    }

    fn persist(&self) -> io::Result<()> {
        let rows: Vec<_> = self
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
        let mut file = tempfile::NamedTempFile::new_in(&self.directory)?;
        serde_json::to_writer(file.as_file_mut(), &rows)?;
        file.flush()?;
        file.as_file().sync_all()?;
        file.persist(self.directory.join("sequences.json"))
            .map_err(|_| io::Error::other("cannot persist sequence checkpoint"))?;
        File::open(&self.directory)?.sync_all()
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
                    io::Error::new(io::ErrorKind::InvalidInput, "metadata capacity exhausted")
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
        self.persist()
    }

    fn reconcile_missing(&mut self, snapshot: &protocol::Snapshot) {
        let live: BTreeSet<_> =
            snapshot
                .panes
                .iter()
                .map(|pane| self.resource(ResourceKind::Pane, &pane.pane_id))
                .chain(snapshot.workspaces.iter().map(|workspace| {
                    self.resource(ResourceKind::Workspace, &workspace.workspace_id)
                }))
                .collect();
        for resource in self.planner.checkpoint().keys() {
            if resource.endpoint_session == self.endpoint.as_deref().unwrap_or("")
                && !live.contains(resource)
            {
                let _ = self.planner.prepare(
                    resource,
                    &tokens::ResourceSnapshot::default(),
                    None,
                    &tokens::Tokens::new(),
                    0,
                    self.refresh_ms,
                );
            }
        }
    }

    fn refresh_with_readers(
        &mut self,
        transport: &mut Transport,
        config: &Config,
        codex: &dyn UsageReader,
        grok: &dyn UsageReader,
    ) -> io::Result<()> {
        config.validate()?;
        self.refresh_ms = config.refresh_interval_ms;
        let snapshot = self.snapshot(transport)?;
        self.reconcile_missing(&snapshot);
        let now = SystemTime::now();
        let now_unix = unix(now);
        // A single budget for the cycle prevents N panes multiplying the reader wait.
        let deadline = Instant::now() + Duration::from_millis(config.reader_timeout_ms);
        let mode = match config.logo_mode.as_str() {
            "font" => tokens::LogoMode::Font,
            "text" => tokens::LogoMode::Text,
            _ => tokens::LogoMode::Auto,
        };
        let mut failed = false;
        for pane in &snapshot.panes {
            let occupant = pane.occupant.clone().filter(|who| {
                config
                    .providers
                    .iter()
                    .any(|provider| provider == agent_id(who.agent))
            });
            let usage = occupant.as_ref().and_then(|who| {
                let native = who.native_session.as_ref()?;
                let (provider, home, reader) = match who.agent {
                    Agent::Codex => (model::Provider::Codex, self.homes.codex.as_ref()?, codex),
                    Agent::Grok => (model::Provider::Grok, self.homes.grok.as_ref()?, grok),
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
                let request = model::ReadRequest {
                    identity: &identity,
                    home,
                    now,
                    max_age: Duration::from_millis(config.usage_max_age_ms),
                    timeout: deadline.saturating_duration_since(Instant::now()),
                };
                let reading = reader.read(&request);
                if reading.identity != identity {
                    return None;
                }
                Some(tokens::Usage {
                    occupant: who.clone(),
                    context_pct: match reading.context {
                        model::UsageOutcome::Available(value) => Some(value),
                        _ => None,
                    },
                    // All weekly adapters remain unsupported in v1.
                    weekly_remaining_pct: None,
                    weekly_reset_unix: None,
                })
            });
            let desired = tokens::pane_tokens(
                &tokens::PaneInput {
                    occupant: occupant.clone(),
                    state: Some(pane.state.clone()),
                    usage,
                },
                mode,
                false,
                now_unix,
            );
            let resource = self.resource(ResourceKind::Pane, &pane.pane_id);
            if let Err(error) = self.report(
                transport,
                &resource,
                &pane.stored_keys,
                occupant.as_ref(),
                &desired,
                None,
            ) {
                if error.kind() == io::ErrorKind::InvalidInput {
                    failed = true;
                    continue;
                }
                // Do not continue I/O after an ambiguous response: reconnect/reconcile.
                return Err(error);
            }
        }
        let local_now = chrono::Local::now();
        for workspace in &snapshot.workspaces {
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
            let repository = cwd.and_then(ship_time::read_repository);
            let ship = repository.as_ref().and_then(|repo| repo.ship_unix);
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
            let resource = self.resource(ResourceKind::Workspace, &workspace.workspace_id);
            if let Err(error) = self.report(
                transport,
                &resource,
                &workspace.stored_keys,
                None,
                &desired,
                None,
            ) {
                if error.kind() != io::ErrorKind::InvalidInput {
                    return Err(error);
                }
                failed = true;
            }
        }
        if failed {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata capacity exhausted; affected resources skipped",
            ))
        } else {
            Ok(())
        }
    }
}

impl daemon::Publisher for MetadataPublisher {
    fn refresh(&mut self, transport: &mut Transport, config: &Config) -> io::Result<()> {
        self.refresh_with_readers(transport, config, &CodexReader, &GrokReader)
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
        self.planner = tokens::Publisher::restore(self.planner.checkpoint());
    }
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

    struct TimeoutReader;
    impl UsageReader for TimeoutReader {
        fn read(&self, request: &ReadRequest<'_>) -> UsageReading {
            UsageReading::missing(request, SourceKind::CodexRollout, MissingReason::Timeout)
        }
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
        let publisher = MetadataPublisher::new(
            paths.root.clone(),
            paths.key.clone(),
            // Missing homes exercise unavailable usage without competing with
            // the reader modules' separately serialized process-wide workers.
            Homes::default(),
        )
        .unwrap();
        let daemon_options = options.clone();
        let daemon = std::thread::spawn(move || {
            let mut publisher = publisher;
            crate::daemon::run_foreground(&daemon_options, false, &mut publisher).unwrap();
        });
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
        daemon.join().unwrap();
        let reports = server.join().unwrap();
        assert_eq!(reports.len(), 4);
        assert_eq!(reports[0]["params"]["tokens"]["lineup_context_text"], "—");
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
        assert!(
            publisher
                .refresh(&mut transport, &crate::config::Config::default())
                .is_err()
        );
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
        assert!(
            publisher
                .refresh(&mut transport, &crate::config::Config::default())
                .is_err()
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
            let mut reports = Vec::new();
            for cycle in 0..3 {
                let mut line = String::new();
                stream.read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], "session.snapshot");
                let response = snapshot(&request["id"], cycle != 1);
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
                    reports.push(request);
                }
            }
            reports
        });
        let mut transport = Transport::connect(&socket).unwrap();
        let homes = Homes {
            codex: Some(home.clone()),
            grok: Some(home),
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
        assert_eq!(reports[3]["params"]["tokens"]["lineup_context_text"], "—");
        assert_eq!(reports[1]["method"], "workspace.report_metadata");
        assert_eq!(
            reports[2]["params"]["tokens"].as_object().unwrap().len(),
            12
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
