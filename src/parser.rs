use crate::domain::*;
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot read state database {path}: {source}")]
    Sqlite {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
}

#[derive(Default)]
struct Meta {
    id: Option<String>,
    created: Option<(String, u64)>,
    cwd: Option<(String, u64)>,
    parent: Option<(String, u64)>,
    agent_path: Option<(String, u64)>,
    nickname: Option<(String, u64)>,
    role: Option<(String, u64)>,
    depth: Option<(u32, u64)>,
    model: Option<(String, u64)>,
    effort: Option<(String, u64)>,
    version: Option<(String, u64)>,
    subagent: bool,
    events: u64,
    errors: u64,
}

fn source(path: &Path, line: u64) -> SourceRef {
    SourceRef::Rollout {
        path: path.to_path_buf(),
        line: Some(line),
    }
}
fn fact<T>(item: Option<(T, u64)>, path: &Path) -> ProfileFact<T> {
    item.map(|(v, l)| ProfileFact::observed(v, source(path, l)))
        .unwrap_or_else(ProfileFact::unknown)
}
fn text(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        x => Some(x.to_string()),
    })
}

/// Parse one rollout, tolerating malformed JSONL records and reporting their count.
pub fn parse_rollout(path: impl AsRef<Path>) -> Result<ParsedRollout, ParseError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| ParseError::Io {
        path: path.into(),
        source,
    })?;
    let mut meta = Meta::default();
    let mut calls = Vec::<ParsedSpawnCall>::new();
    let mut pending = HashMap::new();
    for (offset, raw) in BufReader::new(file).lines().enumerate() {
        let line = (offset + 1) as u64;
        let raw = raw.map_err(|source| ParseError::Io {
            path: path.into(),
            source,
        })?;
        if raw.trim().is_empty() {
            continue;
        }
        let event: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => {
                meta.errors += 1;
                continue;
            }
        };
        let Some(event_obj) = event.as_object() else {
            continue;
        };
        meta.events += 1;
        let payload = event_obj
            .get("payload")
            .and_then(Value::as_object)
            .unwrap_or(event_obj);
        let top_kind = event_obj.get("type").and_then(Value::as_str).unwrap_or("");
        let kind = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(top_kind);
        if top_kind == "session_meta" {
            update_session(&mut meta, payload, line);
        }
        if matches!(top_kind, "turn_context" | "task_started") {
            update_runtime(&mut meta, payload, line);
        }
        if matches!(
            kind,
            "function_call" | "custom_tool_call" | "tool_call" | "function"
        ) {
            let name = text(payload.get("name"));
            if name
                .as_deref()
                .is_some_and(|n| n == "spawn_agent" || n.ends_with("spawn_agent"))
            {
                let args = parse_object(
                    payload
                        .get("arguments")
                        .or_else(|| payload.get("input"))
                        .or_else(|| payload.get("args")),
                );
                let call_id = text(payload.get("call_id").or_else(|| payload.get("id")));
                let idx = calls.len();
                if let Some(id) = &call_id {
                    pending.insert(id.clone(), idx);
                }
                calls.push(ParsedSpawnCall {
                    parent_id: meta.id.clone(),
                    timestamp: text(
                        event_obj
                            .get("timestamp")
                            .or_else(|| payload.get("timestamp")),
                    ),
                    line,
                    call_id,
                    arguments: Value::Object(args),
                    child_ids: vec![],
                    output_line: None,
                    output_error: None,
                    path: path.into(),
                });
            }
        } else if matches!(
            kind,
            "function_call_output" | "custom_tool_call_output" | "tool_result" | "function_output"
        ) {
            let call_id = text(payload.get("call_id"));
            let idx = call_id
                .as_ref()
                .and_then(|id| pending.get(id))
                .copied()
                .or_else(|| calls.len().checked_sub(1));
            if let Some(call) = idx.and_then(|i| calls.get_mut(i)) {
                let output = parse_json(
                    payload
                        .get("output")
                        .or_else(|| payload.get("result"))
                        .or_else(|| payload.get("content")),
                );
                call.output_line = Some(line);
                call.child_ids = extract_ids(&output);
                call.output_error = find_text(&output, &["error", "message"])
                    .filter(|_| output.get("error").is_some());
            }
        }
    }
    let Some(id) = meta.id.clone() else {
        return Ok(ParsedRollout {
            root: None,
            agent: None,
            calls,
        });
    };
    if meta.subagent {
        let agent = AgentSession {
            id,
            parent_thread_id: fact(meta.parent, path),
            path: path.into(),
            cwd: fact(meta.cwd, path),
            agent_path: fact(meta.agent_path, path),
            agent_nickname: fact(meta.nickname, path),
            agent_role: fact(meta.role, path),
            depth: fact(meta.depth, path),
            model: fact(meta.model, path),
            effort: fact(meta.effort, path),
            multi_agent_version: fact(meta.version, path),
            event_count: meta.events,
            parse_errors: meta.errors,
        };
        Ok(ParsedRollout {
            root: None,
            agent: Some(agent),
            calls,
        })
    } else {
        let root = RootConversation {
            id,
            path: path.into(),
            created_at: fact(meta.created, path),
            cwd: fact(meta.cwd, path),
            model: fact(meta.model, path),
            effort: fact(meta.effort, path),
            event_count: meta.events,
            parse_errors: meta.errors,
        };
        Ok(ParsedRollout {
            root: Some(root),
            agent: None,
            calls,
        })
    }
}

/// Combine rollout and optional state-DB evidence into the public profile model.
pub fn scan_sources(
    rollouts: &[PathBuf],
    state_databases: &[PathBuf],
) -> Result<ScanResult, ParseError> {
    let mut result = ScanResult {
        rollout_files: rollouts.to_vec(),
        state_databases: state_databases.to_vec(),
        ..Default::default()
    };
    let mut parsed = Vec::new();
    for path in rollouts {
        let p = parse_rollout(path)?;
        if let Some(r) = p.root.clone() {
            result.root_conversations.push(r)
        }
        if let Some(a) = p.agent.clone() {
            result.agent_sessions.push(a)
        }
        parsed.push(p);
    }
    let agents: HashMap<_, _> = result
        .agent_sessions
        .iter()
        .map(|a| (a.id.clone(), a))
        .collect();
    for rollout in &parsed {
        for call in &rollout.calls {
            result.spawn_attempts.push(attempt_from_call(
                call,
                agents
                    .get(call.child_ids.first().unwrap_or(&String::new()))
                    .copied(),
            ));
        }
    }
    for agent in &result.agent_sessions {
        if !result
            .spawn_attempts
            .iter()
            .any(|a| a.child_thread_id.as_deref() == Some(&agent.id))
        {
            result.spawn_attempts.push(attempt_from_agent(agent));
        }
    }
    for db in state_databases {
        merge_state(db, &mut result)?;
    }
    Ok(result)
}

fn update_session(m: &mut Meta, p: &Map<String, Value>, line: u64) {
    m.id = text(p.get("id").or_else(|| p.get("session_id"))).or(m.id.take());
    if let Some(v) = text(p.get("timestamp").or_else(|| p.get("created_at"))) {
        m.created = Some((v, line))
    }
    if let Some(v) = text(p.get("cwd").or_else(|| p.get("workdir"))) {
        m.cwd = Some((v, line))
    }
    let spawn = p
        .get("source")
        .and_then(|v| {
            v.pointer("/subagent/thread_spawn")
                .or_else(|| v.get("thread_spawn"))
        })
        .and_then(Value::as_object);
    if let Some(s) = spawn {
        m.subagent = true;
        assign_text(
            &mut m.parent,
            s.get("parent_thread_id").or_else(|| s.get("parent_id")),
            line,
        );
        assign_text(&mut m.agent_path, s.get("agent_path"), line);
        assign_text(
            &mut m.nickname,
            s.get("agent_nickname").or_else(|| s.get("nickname")),
            line,
        );
        assign_text(
            &mut m.role,
            s.get("agent_role")
                .or_else(|| s.get("role"))
                .or_else(|| s.get("agent_type")),
            line,
        );
        if let Some(d) = s.get("depth").and_then(Value::as_u64) {
            m.depth = Some((d as u32, line));
        }
    }
    assign_text(
        &mut m.parent,
        p.get("parent_thread_id").or_else(|| p.get("parent_id")),
        line,
    );
    if m.parent.is_some() {
        m.subagent = true
    }
}
fn update_runtime(m: &mut Meta, p: &Map<String, Value>, line: u64) {
    assign_text(&mut m.model, p.get("model"), line);
    assign_text(
        &mut m.effort,
        p.get("effort")
            .or_else(|| p.get("reasoning_effort"))
            .or_else(|| p.get("model_reasoning_effort")),
        line,
    );
    assign_text(&mut m.version, p.get("multi_agent_version"), line);
}
fn assign_text(dst: &mut Option<(String, u64)>, v: Option<&Value>, line: u64) {
    if let Some(v) = text(v) {
        *dst = Some((v, line))
    }
}
fn parse_object(v: Option<&Value>) -> Map<String, Value> {
    match parse_json(v) {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}
fn parse_json(v: Option<&Value>) -> Value {
    match v {
        Some(Value::String(s)) => {
            serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
        }
        Some(v) => v.clone(),
        None => Value::Null,
    }
}
fn find_text(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| text(v.get(k)))
}
fn extract_ids(v: &Value) -> Vec<String> {
    let mut out = vec![];
    fn walk(v: &Value, o: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    if matches!(k.as_str(), "agent_id" | "child_thread_id" | "thread_id") {
                        if let Some(s) = v.as_str() {
                            o.push(s.into())
                        }
                    } else {
                        walk(v, o)
                    }
                }
            }
            Value::Array(a) => {
                for v in a {
                    walk(v, o)
                }
            }
            _ => {}
        }
    }
    walk(v, &mut out);
    out.sort();
    out.dedup();
    out
}
fn arg(call: &ParsedSpawnCall, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| text(call.arguments.get(k)))
}
fn call_fact(call: &ParsedSpawnCall, value: Option<String>) -> ProfileFact<String> {
    value
        .map(|v| ProfileFact::observed(v, source(&call.path, call.line)))
        .unwrap_or_else(ProfileFact::unknown)
}
fn attempt_from_call(c: &ParsedSpawnCall, child: Option<&AgentSession>) -> SpawnAttempt {
    let child_id = c.child_ids.first().cloned();
    let failed = c.output_error.is_some() && child.is_none();
    SpawnAttempt {
        id: blake3::hash(
            format!(
                "{:?}:{:?}:{}:{:?}",
                c.parent_id, c.call_id, c.line, child_id
            )
            .as_bytes(),
        )
        .to_hex()
        .to_string(),
        status: if failed {
            SpawnStatus::Failed
        } else if child.is_some() || child_id.is_some() {
            SpawnStatus::Spawned
        } else {
            SpawnStatus::Requested
        },
        created_at: call_fact(c, c.timestamp.clone()),
        parent_thread_id: c.parent_id.clone(),
        child_thread_id: child_id,
        task_name: call_fact(c, arg(c, &["task_name", "task", "name"])),
        message: call_fact(c, arg(c, &["message", "prompt", "instructions"])),
        agent_type: call_fact(c, arg(c, &["agent_type", "agent_role", "role"])),
        requested_model: call_fact(c, arg(c, &["model", "effective_model"])),
        requested_effort: call_fact(
            c,
            arg(c, &["reasoning_effort", "effort", "model_reasoning_effort"]),
        ),
        fork_turns: call_fact(c, arg(c, &["fork_turns"])),
        effective_model: child
            .map(|x| x.model.clone())
            .unwrap_or_else(ProfileFact::unknown),
        effective_effort: child
            .map(|x| x.effort.clone())
            .unwrap_or_else(ProfileFact::unknown),
        agent_role: child
            .map(|x| x.agent_role.clone())
            .unwrap_or_else(ProfileFact::unknown),
        agent_nickname: child
            .map(|x| x.agent_nickname.clone())
            .unwrap_or_else(ProfileFact::unknown),
        agent_path: child
            .map(|x| x.agent_path.clone())
            .unwrap_or_else(ProfileFact::unknown),
        depth: child
            .map(|x| x.depth.clone())
            .unwrap_or_else(ProfileFact::unknown),
        output_error: call_fact(c, c.output_error.clone()),
        state_status: ProfileFact::unknown(),
        evidence: vec![source(&c.path, c.line)]
            .into_iter()
            .chain(c.output_line.map(|l| source(&c.path, l)))
            .collect(),
    }
}
fn attempt_from_agent(a: &AgentSession) -> SpawnAttempt {
    SpawnAttempt {
        id: blake3::hash(format!("child:{}", a.id).as_bytes())
            .to_hex()
            .to_string(),
        status: SpawnStatus::Orphan,
        created_at: ProfileFact::unknown(),
        parent_thread_id: a.parent_thread_id.value.clone(),
        child_thread_id: Some(a.id.clone()),
        task_name: a.agent_path.clone(),
        message: ProfileFact::unknown(),
        agent_type: ProfileFact::unknown(),
        requested_model: ProfileFact::unknown(),
        requested_effort: ProfileFact::unknown(),
        fork_turns: ProfileFact::unknown(),
        effective_model: a.model.clone(),
        effective_effort: a.effort.clone(),
        agent_role: a.agent_role.clone(),
        agent_nickname: a.agent_nickname.clone(),
        agent_path: a.agent_path.clone(),
        depth: a.depth.clone(),
        output_error: ProfileFact::unknown(),
        state_status: ProfileFact::unknown(),
        evidence: a.parent_thread_id.provenance.clone(),
    }
}
fn merge_state(path: &Path, result: &mut ScanResult) -> Result<(), ParseError> {
    let uri = format!("file:{}?mode=ro", path.to_string_lossy());
    let conn = rusqlite::Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|source| ParseError::Sqlite {
        path: path.into(),
        source,
    })?;
    let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='thread_spawn_edges')",[],|r|r.get(0)).map_err(|source|ParseError::Sqlite{path:path.into(),source})?;
    if !exists {
        return Ok(());
    }
    let mut stmt = conn
        .prepare("SELECT rowid, * FROM thread_spawn_edges")
        .map_err(|source| ParseError::Sqlite {
            path: path.into(),
            source,
        })?;
    let names = stmt
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let rows = stmt
        .query_map([], |row| {
            let mut m = HashMap::new();
            for (i, n) in names.iter().enumerate() {
                let v: rusqlite::types::Value = row.get(i)?;
                m.insert(n.clone(), v);
            }
            Ok(m)
        })
        .map_err(|source| ParseError::Sqlite {
            path: path.into(),
            source,
        })?;
    for row in rows {
        let row = row.map_err(|source| ParseError::Sqlite {
            path: path.into(),
            source,
        })?;
        let get = |ks: &[&str]| ks.iter().find_map(|k| row.get(*k)).and_then(sql_text);
        let parent = get(&["parent_thread_id", "parent_id", "source_thread_id"]);
        let child = get(&[
            "child_thread_id",
            "child_id",
            "agent_id",
            "receiver_thread_id",
        ]);
        if parent.is_none() && child.is_none() {
            continue;
        }
        let status = get(&["status", "state", "terminal_state"]);
        let src = SourceRef::StateDatabase {
            path: path.into(),
            rowid: row.get("rowid").and_then(|v| {
                if let rusqlite::types::Value::Integer(i) = v {
                    Some(*i)
                } else {
                    None
                }
            }),
        };
        if let Some(a) = result.spawn_attempts.iter_mut().find(|a| {
            (child.is_some() && a.child_thread_id == child)
                || (child.is_none() && a.parent_thread_id == parent)
        }) {
            a.state_status = status
                .map(|s| ProfileFact::observed(s, src.clone()))
                .unwrap_or_else(ProfileFact::unknown);
            a.evidence.push(src)
        } else {
            result.spawn_attempts.push(SpawnAttempt {
                id: blake3::hash(format!("state:{parent:?}:{child:?}").as_bytes())
                    .to_hex()
                    .to_string(),
                status: SpawnStatus::StateOnly,
                created_at: ProfileFact::unknown(),
                parent_thread_id: parent,
                child_thread_id: child,
                task_name: ProfileFact::unknown(),
                message: ProfileFact::unknown(),
                agent_type: ProfileFact::unknown(),
                requested_model: ProfileFact::unknown(),
                requested_effort: ProfileFact::unknown(),
                fork_turns: ProfileFact::unknown(),
                effective_model: ProfileFact::unknown(),
                effective_effort: ProfileFact::unknown(),
                agent_role: ProfileFact::unknown(),
                agent_nickname: ProfileFact::unknown(),
                agent_path: ProfileFact::unknown(),
                depth: ProfileFact::unknown(),
                output_error: ProfileFact::unknown(),
                state_status: status
                    .map(|s| ProfileFact::observed(s, src.clone()))
                    .unwrap_or_else(ProfileFact::unknown),
                evidence: vec![src],
            })
        }
    }
    Ok(())
}
fn sql_text(v: &rusqlite::types::Value) -> Option<String> {
    match v {
        rusqlite::types::Value::Text(s) => Some(s.clone()),
        rusqlite::types::Value::Integer(i) => Some(i.to_string()),
        rusqlite::types::Value::Real(x) => Some(x.to_string()),
        _ => None,
    }
}
