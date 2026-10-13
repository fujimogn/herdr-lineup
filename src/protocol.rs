//! Pure JSON protocol for the pinned Herdr v0.9.3 socket API. No transport I/O.
//!
//! Wire contract: <https://github.com/herdrdev/herdr/blob/v0.9.3/docs/next/website/src/content/docs/socket-api.mdx>.
//! Snapshot record shapes: `src/api/schema/{response,session,panes}.rs` at
//! that same tag. Fixtures below are synthetic, never captured from a session.

use crate::tokens::{
    Agent, Occupant, PANE_KEYS, Report, ResourceKind, SOURCE, WORKSPACE_KEYS, normalize_value,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerError {
    pub code: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    InvalidReport,
    ResponseIdMismatch,
    MalformedResponse,
    Server(ServerError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pane {
    pub pane_id: String,
    pub workspace_id: String,
    pub occupant: Option<Occupant>,
    pub state: String,
    pub cwd: Option<String>,
    pub foreground_cwd: Option<String>,
    pub stored_keys: BTreeSet<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    pub workspace_id: String,
    pub checkout_path: Option<String>,
    pub stored_keys: BTreeSet<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub version: String,
    pub protocol: u32,
    pub panes: Vec<Pane>,
    pub workspaces: Vec<Workspace>,
}

/// Convert a reserved `tokens::Publisher` report directly into a JSON request.
/// Numeric tokens remain unit-less strings; absent/empty values become null.
/// The caller owns sequence persistence, transport framing and acknowledgement.
pub fn metadata_request(id: &str, report: &Report) -> Result<Value, ProtocolError> {
    let (method, target, allowed): (&str, &str, &[&str]) = match report.resource.kind {
        ResourceKind::Pane => ("pane.report_metadata", "pane_id", &PANE_KEYS),
        ResourceKind::Workspace => ("workspace.report_metadata", "workspace_id", &WORKSPACE_KEYS),
    };
    if id.trim().is_empty()
        || report.resource.id.trim().is_empty()
        || report.source != SOURCE
        || !(1..=86_400_000).contains(&report.ttl_ms)
        || report.tokens.len() > 16
        || report
            .tokens
            .keys()
            .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(ProtocolError::InvalidReport);
    }
    let tokens: Map<String, Value> = report
        .tokens
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                normalize_value(value.as_deref())
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            )
        })
        .collect();
    let mut params =
        json!({"source":SOURCE,"ttl_ms":report.ttl_ms,"seq":report.seq,"tokens":tokens});
    params[target] = Value::String(report.resource.id.clone());
    Ok(json!({"id":id,"method":method,"params":params}))
}

/// Validate the envelope and correlation id before callers accept a response.
/// Server messages are deliberately not retained: they may contain user data.
/// Unknown error codes remain errors, never successful acknowledgements.
pub fn response_result<'a>(id: &str, response: &'a Value) -> Result<&'a Value, ProtocolError> {
    let received = required_string(response, "id")?;
    if received != id {
        return Err(ProtocolError::ResponseIdMismatch);
    }
    match (response.get("result"), response.get("error")) {
        (Some(result), None) if result.is_object() => Ok(result),
        (None, Some(error)) => {
            let code = required_string(error, "code")?;
            required_string(error, "message")?;
            Err(ProtocolError::Server(ServerError { code: code.into() }))
        }
        _ => Err(ProtocolError::MalformedResponse),
    }
}

/// Parse the authoritative pane records, never display labels or event payloads.
/// Missing native ids keep agent/state visible but cannot associate usage.
/// Path session references are not native ids. Snapshot identity is NOT endpoint
/// identity: the transport must separately pin the server connection/generation.
/// Unknown fields are ignored; malformed required fields reject the whole cache.
pub fn parse_snapshot(id: &str, response: &Value) -> Result<Snapshot, ProtocolError> {
    let result = response_result(id, response)?;
    if required_string(result, "type")? != "session_snapshot" {
        return Err(ProtocolError::MalformedResponse);
    }
    let snapshot = result
        .get("snapshot")
        .ok_or(ProtocolError::MalformedResponse)?;
    let version = nonempty_string(snapshot, "version")?.into();
    let protocol = snapshot
        .get("protocol")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(ProtocolError::MalformedResponse)?;
    let mut workspace_ids = BTreeSet::new();
    let workspaces = required_array(snapshot, "workspaces")?
        .iter()
        .map(|workspace| {
            let workspace_id = nonempty_string(workspace, "workspace_id")?.to_owned();
            if !workspace_ids.insert(workspace_id.clone()) {
                return Err(ProtocolError::MalformedResponse);
            }
            let checkout_path = match workspace.get("worktree") {
                None | Some(Value::Null) => None,
                Some(worktree) if worktree.is_object() => {
                    Some(nonempty_string(worktree, "checkout_path")?.to_owned())
                }
                _ => return Err(ProtocolError::MalformedResponse),
            };
            Ok(Workspace {
                workspace_id,
                checkout_path,
                stored_keys: stored_keys(workspace)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut pane_ids = BTreeSet::new();
    let panes = required_array(snapshot, "panes")?
        .iter()
        .map(|pane| {
            let pane_id = nonempty_string(pane, "pane_id")?.to_owned();
            let workspace_id = nonempty_string(pane, "workspace_id")?.to_owned();
            if !pane_ids.insert(pane_id.clone()) || !workspace_ids.contains(&workspace_id) {
                return Err(ProtocolError::MalformedResponse);
            }
            let label = optional_string(pane, "agent")?;
            let agent = label.and_then(Agent::from_id);
            let occupant = agent.map(|agent| Occupant {
                agent,
                native_session: pane.get("agent_session").and_then(|session| {
                    if session.get("agent").and_then(Value::as_str) != label
                        || session.get("kind").and_then(Value::as_str) != Some("id")
                    {
                        return None;
                    }
                    session
                        .get("value")
                        .and_then(Value::as_str)
                        .filter(|id| !id.trim().is_empty())
                        .map(str::to_owned)
                }),
            });
            let state = match required_string(pane, "agent_status")? {
                state @ ("idle" | "working" | "blocked" | "done" | "unknown") => state,
                _ => "unknown",
            }
            .into();
            Ok(Pane {
                pane_id,
                workspace_id,
                occupant,
                state,
                cwd: optional_string(pane, "cwd")?.map(str::to_owned),
                foreground_cwd: optional_string(pane, "foreground_cwd")?.map(str::to_owned),
                stored_keys: stored_keys(pane)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Snapshot {
        version,
        protocol,
        panes,
        workspaces,
    })
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, ProtocolError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ProtocolError::MalformedResponse)
}
fn nonempty_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, ProtocolError> {
    required_string(value, key).and_then(|s| {
        if s.trim().is_empty() {
            Err(ProtocolError::MalformedResponse)
        } else {
            Ok(s)
        }
    })
}
fn optional_string<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, ProtocolError> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        _ => Err(ProtocolError::MalformedResponse),
    }
}
fn required_array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, ProtocolError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or(ProtocolError::MalformedResponse)
}
fn stored_keys(value: &Value) -> Result<BTreeSet<String>, ProtocolError> {
    match value.get("tokens") {
        None => Ok(BTreeSet::new()),
        Some(Value::Object(tokens)) if tokens.values().all(Value::is_string) => {
            Ok(tokens.keys().cloned().collect())
        }
        _ => Err(ProtocolError::MalformedResponse),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{Publisher, ResourceId, ResourceKind, ResourceSnapshot, Tokens};
    use serde_json::json;
    use std::collections::BTreeMap;

    // Synthetic fixture: socket-api.mdx examples plus the v0.9.3 public
    // schema/session.rs and schema/panes.rs record structure. No live data.
    #[test]
    fn workspace_worktree_checkout_is_parsed_and_invalid_paths_rejected() {
        let mut response = fixture();
        response["result"]["snapshot"]["workspaces"][0]["worktree"] = json!({
            "repo_root": "/example/repo", "checkout_path": "/example/checkout"
        });
        let parsed = parse_snapshot("snapshot-1", &response).unwrap();
        assert_eq!(
            parsed.workspaces[0].checkout_path.as_deref(),
            Some("/example/checkout")
        );
        response["result"]["snapshot"]["workspaces"][0]["worktree"]["checkout_path"] = json!(42);
        assert_eq!(
            parse_snapshot("snapshot-1", &response),
            Err(ProtocolError::MalformedResponse)
        );
    }

    fn fixture() -> Value {
        json!({"id":"snapshot-1","result":{"type":"session_snapshot","snapshot":{
            "version":"0.9.3","protocol":1,"focused_workspace_id":"w1",
            "workspaces":[{"workspace_id":"w1","number":1,"label":"demo",
                "focused":true,"pane_count":2,"tab_count":1,"active_tab_id":"w1:t1",
                "agent_status":"working","tokens":{"other_plugin":"preserve"}}],
            "tabs":[],"layouts":[],"agents":[],
            "panes":[{"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1",
                "terminal_id":"terminal-1","focused":true,"revision":1,
                "agent":"codex","agent_status":"working","cwd":"/example/repo",
                "foreground_cwd":"/example/repo/subdir","display_agent":"Misleading label",
                "agent_session":{"source":"herdr:codex","agent":"codex","kind":"id","value":"example-session"},
                "tokens":{"lineup_context_pct":"7","other_plugin":"preserve"}},
                {"pane_id":"w1:p2","workspace_id":"w1","tab_id":"w1:t1",
                "terminal_id":"terminal-2","focused":false,"revision":1,"agent_status":"idle"}]
        }}})
    }

    fn planned(kind: ResourceKind, tokens: Tokens) -> crate::tokens::Report {
        Publisher::default()
            .prepare(
                &ResourceId {
                    endpoint_session: "synthetic-endpoint".into(),
                    kind,
                    id: if kind == ResourceKind::Pane {
                        "w1:p1"
                    } else {
                        "w1"
                    }
                    .into(),
                },
                &ResourceSnapshot {
                    exists: true,
                    ..ResourceSnapshot::default()
                },
                None,
                &tokens,
                0,
                5000,
            )
            .unwrap()
            .unwrap()
    }

    #[test]
    fn pane_report_preserves_patch_and_null_numeric_clear() {
        let report = planned(
            ResourceKind::Pane,
            BTreeMap::from([
                ("lineup_context_pct".into(), None),
                ("lineup_context_text".into(), Some("—".into())),
                ("lineup_state_idle".into(), Some("".into())),
            ]),
        );
        assert_eq!(
            metadata_request("report-1", &report).unwrap(),
            json!({
                "id":"report-1","method":"pane.report_metadata","params":{
                    "pane_id":"w1:p1","source":"herdr-lineup","ttl_ms":15000,"seq":1,
                    "tokens":{"lineup_context_pct":null,"lineup_context_text":"—","lineup_state_idle":null}
                }
            })
        );
    }

    #[test]
    fn workspace_report_and_empty_values_clear_without_touching_omitted_keys() {
        let mut report = planned(
            ResourceKind::Workspace,
            BTreeMap::from([
                ("lineup_repo".into(), Some("demo".into())),
                ("lineup_ship_unix".into(), Some("1791763200".into())),
            ]),
        );
        report
            .tokens
            .insert("lineup_repo".into(), Some(" \n\t".into()));
        let request = metadata_request("report-2", &report).unwrap();
        assert_eq!(request["method"], "workspace.report_metadata");
        assert_eq!(request["params"]["workspace_id"], "w1");
        assert_eq!(
            request["params"]["tokens"],
            json!({"lineup_repo":null,"lineup_ship_unix":"1791763200"})
        );
        assert!(request["params"].get("pane_id").is_none());
    }

    #[test]
    fn request_rejects_out_of_protocol_ttl_and_foreign_source() {
        let mut report = planned(
            ResourceKind::Workspace,
            BTreeMap::from([("lineup_repo".into(), Some("demo".into()))]),
        );
        for ttl in [0, 86_400_001] {
            report.ttl_ms = ttl;
            assert_eq!(
                metadata_request("r", &report),
                Err(ProtocolError::InvalidReport)
            );
        }
        report.ttl_ms = 86_400_000;
        assert!(metadata_request("r", &report).is_ok());
        report.source = "other-plugin";
        assert_eq!(
            metadata_request("r", &report),
            Err(ProtocolError::InvalidReport)
        );
    }

    #[test]
    fn snapshot_extracts_authoritative_occupant_and_resource_keys() {
        let snapshot = parse_snapshot("snapshot-1", &fixture()).unwrap();
        assert_eq!(snapshot.version, "0.9.3");
        assert_eq!(snapshot.protocol, 1);
        let pane = &snapshot.panes[0];
        assert_eq!(pane.pane_id, "w1:p1");
        assert_eq!(pane.workspace_id, "w1");
        assert_eq!(
            pane.occupant,
            Some(crate::tokens::Occupant {
                agent: crate::tokens::Agent::Codex,
                native_session: Some("example-session".into())
            })
        );
        assert_eq!(pane.state, "working");
        assert_eq!(pane.cwd.as_deref(), Some("/example/repo"));
        assert_eq!(pane.foreground_cwd.as_deref(), Some("/example/repo/subdir"));
        assert!(pane.stored_keys.contains("other_plugin"));
        assert!(snapshot.workspaces[0].stored_keys.contains("other_plugin"));
        assert!(snapshot.panes[1].occupant.is_none());
    }

    #[test]
    fn native_identity_requires_matching_agent_and_id_kind() {
        for session in [
            json!(null),
            json!({"source":"herdr:codex","agent":"grok","kind":"id","value":"other-session"}),
            json!({"source":"herdr:codex","agent":"codex","kind":"path","value":"/example/session"}),
            json!({"source":"herdr:codex","agent":"codex","kind":"id","value":" "}),
        ] {
            let mut value = fixture();
            value["result"]["snapshot"]["panes"][0]["agent_session"] = session;
            assert!(
                parse_snapshot("snapshot-1", &value).unwrap().panes[0]
                    .occupant
                    .as_ref()
                    .unwrap()
                    .native_session
                    .is_none()
            );
        }
        let mut value = fixture();
        value["result"]["snapshot"]["panes"][0]["agent"] = json!("custom-bot");
        assert!(
            parse_snapshot("snapshot-1", &value).unwrap().panes[0]
                .occupant
                .is_none()
        );
    }

    #[test]
    fn six_agents_and_unknown_future_state_are_handled() {
        for (label, agent) in [
            ("claude", crate::tokens::Agent::Claude),
            ("codex", crate::tokens::Agent::Codex),
            ("grok", crate::tokens::Agent::Grok),
            ("agy", crate::tokens::Agent::Agy),
            ("kimi", crate::tokens::Agent::Kimi),
            ("hermes", crate::tokens::Agent::Hermes),
        ] {
            let mut value = fixture();
            value["result"]["snapshot"]["panes"][0]["agent"] = json!(label);
            value["result"]["snapshot"]["panes"][0]["agent_status"] = json!("future-state");
            let parsed = parse_snapshot("snapshot-1", &value).unwrap();
            assert_eq!(parsed.panes[0].occupant.as_ref().unwrap().agent, agent);
            assert_eq!(parsed.panes[0].state, "unknown");
        }
    }

    #[test]
    fn malformed_or_unrelated_snapshot_never_becomes_an_empty_authoritative_cache() {
        assert_eq!(
            parse_snapshot("another-request", &fixture()),
            Err(ProtocolError::ResponseIdMismatch)
        );
        for pointer in [
            "/result/snapshot/panes",
            "/result/snapshot/workspaces",
            "/result/snapshot/version",
            "/result/snapshot/protocol",
            "/result/snapshot/panes/0/pane_id",
            "/result/snapshot/panes/0/workspace_id",
            "/result/snapshot/panes/0/agent_status",
        ] {
            let mut value = fixture();
            *value.pointer_mut(pointer).unwrap() = json!(null);
            assert_eq!(
                parse_snapshot("snapshot-1", &value),
                Err(ProtocolError::MalformedResponse)
            );
        }
        let mut value = fixture();
        value["result"]["snapshot"]["panes"][1] = value["result"]["snapshot"]["panes"][0].clone();
        assert_eq!(
            parse_snapshot("snapshot-1", &value),
            Err(ProtocolError::MalformedResponse)
        );
    }

    #[test]
    fn response_errors_preserve_codes_without_server_messages() {
        for code in [
            "not_found",
            "unknown_method",
            "invalid_params",
            "invalid_request",
            "events_lost",
            "future_code",
        ] {
            let value = json!({"id":"r","error":{"code":code,"message":"synthetic detail"}});
            assert_eq!(
                response_result("r", &value),
                Err(ProtocolError::Server(ServerError { code: code.into() }))
            );
        }
        assert_eq!(
            response_result("r", &json!({"id":"r","result":{"type":"pong"}})).unwrap(),
            &json!({"type":"pong"})
        );
        for value in [
            json!({"id":"r"}),
            json!({"id":"r","error":{"message":"bad"}}),
            json!({"id":"r","result":{},"error":{"code":"not_found","message":"bad"}}),
        ] {
            assert_eq!(
                response_result("r", &value),
                Err(ProtocolError::MalformedResponse)
            );
        }
    }
}
