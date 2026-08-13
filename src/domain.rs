use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// The confidence attached to every profiling value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FactConfidence {
    Observed,
    Derived,
    Unknown,
    Conflicting,
}

/// A stable reference to evidence without copying the raw payload into an index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum SourceRef {
    Rollout { path: PathBuf, line: Option<u64> },
    StateDatabase { path: PathBuf, rowid: Option<i64> },
    Derived { rule: String },
}

/// A value plus its evidence and confidence. Unknown is represented by `None`, never zero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileFact<T> {
    pub value: Option<T>,
    pub confidence: FactConfidence,
    pub provenance: Vec<SourceRef>,
    pub conflicting_values: Vec<T>,
}

impl<T> ProfileFact<T> {
    pub fn unknown() -> Self {
        Self {
            value: None,
            confidence: FactConfidence::Unknown,
            provenance: vec![],
            conflicting_values: vec![],
        }
    }

    pub fn observed(value: T, source: SourceRef) -> Self {
        Self {
            value: Some(value),
            confidence: FactConfidence::Observed,
            provenance: vec![source],
            conflicting_values: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpawnStatus {
    Requested,
    Spawned,
    Failed,
    StateOnly,
    Orphan,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RootConversation {
    pub id: String,
    pub path: PathBuf,
    pub created_at: ProfileFact<String>,
    pub cwd: ProfileFact<String>,
    pub model: ProfileFact<String>,
    pub effort: ProfileFact<String>,
    pub event_count: u64,
    pub parse_errors: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub parent_thread_id: ProfileFact<String>,
    pub path: PathBuf,
    pub cwd: ProfileFact<String>,
    pub agent_path: ProfileFact<String>,
    pub agent_nickname: ProfileFact<String>,
    pub agent_role: ProfileFact<String>,
    pub depth: ProfileFact<u32>,
    pub model: ProfileFact<String>,
    pub effort: ProfileFact<String>,
    pub multi_agent_version: ProfileFact<String>,
    pub event_count: u64,
    pub parse_errors: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpawnAttempt {
    pub id: String,
    pub status: SpawnStatus,
    pub created_at: ProfileFact<String>,
    pub parent_thread_id: Option<String>,
    pub child_thread_id: Option<String>,
    pub task_name: ProfileFact<String>,
    pub message: ProfileFact<String>,
    pub agent_type: ProfileFact<String>,
    pub requested_model: ProfileFact<String>,
    pub requested_effort: ProfileFact<String>,
    pub fork_turns: ProfileFact<String>,
    pub effective_model: ProfileFact<String>,
    pub effective_effort: ProfileFact<String>,
    pub agent_role: ProfileFact<String>,
    pub agent_nickname: ProfileFact<String>,
    pub agent_path: ProfileFact<String>,
    pub depth: ProfileFact<u32>,
    pub output_error: ProfileFact<String>,
    pub state_status: ProfileFact<String>,
    pub evidence: Vec<SourceRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScanResult {
    pub root_conversations: Vec<RootConversation>,
    pub agent_sessions: Vec<AgentSession>,
    pub spawn_attempts: Vec<SpawnAttempt>,
    pub rollout_files: Vec<PathBuf>,
    pub state_databases: Vec<PathBuf>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParsedRollout {
    pub root: Option<RootConversation>,
    pub agent: Option<AgentSession>,
    pub(crate) calls: Vec<ParsedSpawnCall>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ParsedSpawnCall {
    pub parent_id: Option<String>,
    pub timestamp: Option<String>,
    pub line: u64,
    pub call_id: Option<String>,
    pub arguments: Value,
    pub child_ids: Vec<String>,
    pub output_line: Option<u64>,
    pub output_error: Option<String>,
    pub path: PathBuf,
}
