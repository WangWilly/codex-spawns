//! Rebuildable SQLite-backed catalog for conversation profiles.
//!
//! The repository stores display metadata and excerpts only. Source rollouts remain
//! authoritative and are never opened for writing.

use crate::{ProfileFact, ProjectAssignment, TokenUsage, TokenUsageSummary};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

const SCHEMA_VERSION: i64 = 3;
const SNAPSHOT_RETENTION_GENERATIONS: i64 = 64;
pub use crate::projection::PROJECTION_VERSION as REQUIRED_PROJECTION_VERSION;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid cursor")]
    InvalidCursor,
    #[error("refresh rejected: {0}")]
    RefreshRejected(String),
    #[error("duplicate conversation id in refresh batch: {0}")]
    DuplicateConversationId(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexOptions {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationRecord {
    pub id: String,
    pub title: String,
    pub title_source: String,
    pub cwd: String,
    pub created_at: String,
    pub last_activity_at: String,
    pub archived: bool,
    pub model: Option<String>,
    pub status: Option<String>,
    pub agent_count: u64,
    pub max_depth: u32,
    pub profile_complete: bool,
    pub project: ProfileFact<ProjectAssignment>,
    pub tokens: TokenUsageSummary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRecord {
    pub id: String,
    pub root_id: String,
    pub parent_id: Option<String>,
    pub agent_path: Option<String>,
    pub task_name: Option<String>,
    pub task_excerpt: Option<String>,
    pub title: String,
    pub role: Option<String>,
    pub nickname: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub status: String,
    pub depth: u32,
    pub evidence_complete: bool,
    pub tokens: ProfileFact<TokenUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationProfile {
    pub conversation: ConversationRecord,
    pub agents: Vec<AgentRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConversationState {
    Active,
    Archived,
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProfileQuality {
    Complete,
    Partial,
    Conflicting,
    Updating,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationSemantics {
    pub id: String,
    pub state: ConversationState,
    pub profile: ProfileQuality,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConversationFilter {
    pub archived: Option<bool>,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub query: Option<String>,
    pub project: Option<ProjectFilter>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectFilter {
    Assigned(String),
    Projectless,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortField {
    #[default]
    Updated,
    Title,
    Agents,
    Depth,
    State,
    Profile,
    Project,
    Tokens,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseOrder {
    pub field: SortField,
    pub direction: SortDirection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionStatus {
    pub current: u32,
    pub required: u32,
}

impl ProjectionStatus {
    pub fn needs_reprojection(self) -> bool {
        self.current < self.required
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowseCursor {
    snapshot_at: String,
    order: BrowseOrder,
    id: String,
    filter: String,
    key: String,
    null_rank: i64,
}

impl BrowseCursor {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("cursor is serializable")
    }
    pub fn decode(value: &str) -> Result<Self, IndexError> {
        serde_json::from_str(value).map_err(|_| IndexError::InvalidCursor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowsePage {
    pub conversations: Vec<ConversationRecord>,
    pub next_cursor: Option<BrowseCursor>,
    pub semantics: BTreeMap<String, (ConversationState, ProfileQuality)>,
    /// Total rows matching the current filter in this immutable snapshot.
    pub approximate_total: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexStats {
    pub conversations: u64,
    pub agents: u64,
    pub sources: u64,
    pub missing_sources: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRecord {
    pub logical_id: String,
    pub canonical_path: PathBuf,
    pub size: u64,
    pub modified_ns: i64,
    pub fingerprint: String,
    pub safe_offset: u64,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceChange {
    New,
    Unchanged,
    Appended { from_offset: u64 },
    Replaced,
    Moved { from: PathBuf },
    Reactivated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshEvent {
    Started,
    BatchCommitted { records: usize },
    SourceMissing { logical_id: String },
    Completed,
}

#[derive(Clone, Debug, Default)]
pub struct RefreshBatch {
    pub conversations: Vec<ConversationRecord>,
    pub agents: Vec<AgentRecord>,
    pub sources: Vec<SourceRecord>,
    pub discovered_all_sources: bool,
    /// True only when both Codex App stores were read as one valid snapshot.
    pub app_metadata_refreshed: bool,
    pub app_metadata_diagnostic: Option<String>,
    /// Testable failure injection at the public transaction boundary.
    pub reject_reason: Option<String>,
}

pub struct ProfileIndex {
    conn: Connection,
}

impl ProfileIndex {
    pub fn open(options: IndexOptions) -> Result<Self, IndexError> {
        if let Some(parent) = options.path.parent() {
            fs::create_dir_all(parent)?;
            set_dir_permissions(parent)?;
        }
        let conn = Connection::open(&options.path)?;
        set_file_permissions(&options.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        // These additions deliberately remain compatible with indexes created by v1.
        ensure_column(
            &conn,
            "conversations",
            "conversation_state",
            "TEXT NOT NULL DEFAULT 'active'",
        )?;
        ensure_column(
            &conn,
            "conversations",
            "profile_quality",
            "TEXT NOT NULL DEFAULT 'partial'",
        )?;
        for table in ["conversations", "conversation_versions"] {
            ensure_column(&conn, table, "project_json", "TEXT")?;
            ensure_column(&conn, table, "project_kind", "INTEGER NOT NULL DEFAULT 2")?;
            ensure_column(&conn, table, "project_id", "TEXT")?;
            ensure_column(&conn, table, "project_name", "TEXT")?;
            ensure_column(&conn, table, "tokens_json", "TEXT")?;
            ensure_column(&conn, table, "token_total", "INTEGER")?;
        }
        for table in ["agents", "agent_versions"] {
            ensure_column(&conn, table, "title", "TEXT NOT NULL DEFAULT ''")?;
            ensure_column(&conn, table, "tokens_json", "TEXT")?;
            ensure_column(&conn, table, "token_total", "INTEGER")?;
        }
        conn.execute(
            "INSERT OR IGNORE INTO conversation_versions(id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,conversation_state,profile_quality,indexed_generation,project_json,project_kind,project_id,project_name,tokens_json,token_total)
             SELECT id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,conversation_state,profile_quality,indexed_generation,project_json,project_kind,project_id,project_name,tokens_json,token_total FROM conversations",
            [],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO agent_versions(id,root_id,parent_id,agent_path,task_name,task_excerpt,role,nickname,model,effort,status,depth,evidence_complete,indexed_generation,title,tokens_json,token_total)
             SELECT a.id,a.root_id,a.parent_id,a.agent_path,a.task_name,a.task_excerpt,a.role,a.nickname,a.model,a.effort,a.status,a.depth,a.evidence_complete,c.indexed_generation,a.title,a.tokens_json,a.token_total
             FROM agents a JOIN conversations c ON c.id=a.root_id",
            [],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO index_metadata(key,value) VALUES('projection_version','0')",
            [],
        )?;
        conn.execute("INSERT OR IGNORE INTO index_metadata(key,value) VALUES('app_metadata_status','not yet refreshed')", [])?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    pub fn refresh<F>(&mut self, batch: RefreshBatch, mut emit: F) -> Result<(), IndexError>
    where
        F: FnMut(RefreshEvent),
    {
        let mut conversation_ids = HashSet::with_capacity(batch.conversations.len());
        for conversation in &batch.conversations {
            if !conversation_ids.insert(&conversation.id) {
                return Err(IndexError::DuplicateConversationId(conversation.id.clone()));
            }
        }
        emit(RefreshEvent::Started);
        let tx = self.conn.transaction()?;
        apply_batch(&tx, &batch, &mut emit)?;
        if let Some(reason) = batch.reject_reason {
            return Err(IndexError::RefreshRejected(reason));
        }
        tx.commit()?;
        emit(RefreshEvent::BatchCommitted {
            records: batch.conversations.len(),
        });
        emit(RefreshEvent::Completed);
        Ok(())
    }

    pub fn browse(
        &self,
        filter: &ConversationFilter,
        cursor: Option<&BrowseCursor>,
        page_size: usize,
    ) -> Result<BrowsePage, IndexError> {
        self.browse_ordered(filter, cursor, page_size, BrowseOrder::default())
    }

    pub fn browse_ordered(
        &self,
        filter: &ConversationFilter,
        cursor: Option<&BrowseCursor>,
        page_size: usize,
        order: BrowseOrder,
    ) -> Result<BrowsePage, IndexError> {
        let limit = if page_size == 0 {
            25
        } else {
            page_size.min(250)
        };
        let filter_signature = filter_signature(filter);
        let snapshot_at = cursor
            .map(|c| c.snapshot_at.clone())
            .unwrap_or_else(|| max_generation(&self.conn).unwrap_or_default().to_string());
        let generation = snapshot_at
            .parse::<i64>()
            .map_err(|_| IndexError::InvalidCursor)?;
        if cursor.is_some() && !generation_is_retained(&self.conn, generation)? {
            return Err(IndexError::InvalidCursor);
        }
        if cursor.is_some_and(|cursor| cursor.order != order || cursor.filter != filter_signature) {
            return Err(IndexError::InvalidCursor);
        }
        let (key_expr, null_expr, key_is_numeric) = sort_expressions(order.field);
        let primary_op = if order.direction == SortDirection::Asc {
            ">"
        } else {
            "<"
        };
        // Updated timestamps always place unknown values last and use ascending IDs
        // as their deterministic tie-breaker. Other fields apply direction to IDs.
        let id_op = if matches!(
            order.field,
            SortField::Updated | SortField::Project | SortField::Tokens
        ) || order.direction == SortDirection::Asc
        {
            ">"
        } else {
            "<"
        };
        let id_direction = if id_op == ">" { "ASC" } else { "DESC" };
        let primary_direction = if order.direction == SortDirection::Asc {
            "ASC"
        } else {
            "DESC"
        };
        let cast = if key_is_numeric {
            "CAST(?11 AS INTEGER)"
        } else {
            "?11"
        };
        let cursor_predicate = if cursor.is_some() {
            format!("AND (({null_expr}) > ?10 OR (({null_expr}) = ?10 AND (({key_expr}) {primary_op} {cast} OR (({key_expr}) = {cast} AND id {id_op} ?12))))")
        } else {
            String::new()
        };
        let sql = format!("WITH snapshot AS (
             SELECT cv.* FROM conversation_versions cv JOIN (
               SELECT id,MAX(indexed_generation) AS generation FROM conversation_versions
               WHERE indexed_generation <= ?1 GROUP BY id
             ) latest ON latest.id=cv.id AND latest.generation=cv.indexed_generation
           )
           SELECT id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,conversation_state,profile_quality,project_json,tokens_json,
                  CAST(({key_expr}) AS TEXT),({null_expr})
           FROM snapshot WHERE (?2 IS NULL OR archived=?2) AND (?3 IS NULL OR cwd=?3) AND (?4 IS NULL OR model=?4) AND (?5 IS NULL OR status=?5)
             AND (?6 IS NULL OR title LIKE '%'||?6||'%' OR id LIKE '%'||?6||'%' OR cwd LIKE '%'||?6||'%' OR project_name LIKE '%'||?6||'%'
               OR EXISTS (SELECT 1 FROM agent_versions a JOIN (
                    SELECT id,MAX(indexed_generation) AS generation FROM agent_versions
                    WHERE indexed_generation <= ?1 GROUP BY id
                  ) latest_agent ON latest_agent.id=a.id AND latest_agent.generation=a.indexed_generation
                  WHERE a.root_id=snapshot.id AND
                 (a.id LIKE '%'||?6||'%' OR a.task_name LIKE '%'||?6||'%' OR a.role LIKE '%'||?6||'%'
                  OR a.nickname LIKE '%'||?6||'%' OR a.model LIKE '%'||?6||'%' OR a.status LIKE '%'||?6||'%')))
             AND (?7 IS NULL OR project_kind=?7) AND (?8 IS NULL OR project_id=?8)
             {cursor_predicate}
           ORDER BY ({null_expr}) ASC, ({key_expr}) {primary_direction}, id {id_direction} LIMIT ?9");
        let archived = filter.archived.map(i64::from);
        let total = snapshot_total(&self.conn, generation, filter)?;
        let mut stmt = self.conn.prepare(&sql)?;
        let query_limit = (limit + 1) as i64;
        let cursor_null = cursor.map_or(0, |value| value.null_rank);
        let cursor_key = cursor.map_or("", |value| value.key.as_str());
        let cursor_id = cursor.map_or("", |value| value.id.as_str());
        let map_row = |row: &rusqlite::Row<'_>| {
            Ok((
                row_to_conversation(row)?,
                row.get::<_, String>(12)?,
                row.get::<_, String>(13)?,
                row.get::<_, String>(16)?,
                row.get::<_, i64>(17)?,
            ))
        };
        let mut indexed = if cursor.is_some() {
            stmt.query_map(
                params![
                    generation,
                    archived,
                    filter.cwd,
                    filter.model,
                    filter.status,
                    filter.query,
                    project_kind(filter),
                    project_id(filter),
                    query_limit,
                    cursor_null,
                    cursor_key,
                    cursor_id
                ],
                map_row,
            )?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(
                params![
                    generation,
                    archived,
                    filter.cwd,
                    filter.model,
                    filter.status,
                    filter.query,
                    project_kind(filter),
                    project_id(filter),
                    query_limit
                ],
                map_row,
            )?
            .collect::<Result<Vec<_>, _>>()?
        };
        let has_more = indexed.len() > limit;
        indexed.truncate(limit);
        let semantics = indexed
            .iter()
            .map(|(record, state, profile, _, _)| {
                (
                    record.id.clone(),
                    (parse_state(state), parse_profile(profile)),
                )
            })
            .collect();
        let next_key = indexed
            .last()
            .map(|(_, _, _, key, rank)| (key.clone(), *rank));
        let conversations: Vec<_> = indexed
            .into_iter()
            .map(|(record, _, _, _, _)| record)
            .collect();
        let next_cursor = if has_more {
            conversations.last().map(|c| BrowseCursor {
                snapshot_at,
                order,
                id: c.id.clone(),
                filter: filter_signature,
                key: next_key.as_ref().unwrap().0.clone(),
                null_rank: next_key.as_ref().unwrap().1,
            })
        } else {
            None
        };
        Ok(BrowsePage {
            conversations,
            next_cursor,
            semantics,
            approximate_total: total,
        })
    }

    pub fn projection_status(&self) -> Result<ProjectionStatus, IndexError> {
        let current = self.conn.query_row(
            "SELECT value FROM index_metadata WHERE key='projection_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        Ok(ProjectionStatus {
            current: current.parse().unwrap_or_default(),
            required: REQUIRED_PROJECTION_VERSION,
        })
    }

    pub fn current_projection_version(&self) -> Result<u32, IndexError> {
        Ok(self.projection_status()?.current)
    }

    pub const fn required_projection_version() -> u32 {
        REQUIRED_PROJECTION_VERSION
    }

    pub fn needs_reprojection(&self) -> Result<bool, IndexError> {
        Ok(self.projection_status()?.needs_reprojection())
    }

    /// Atomically installs a reprojected batch and records its projection version.
    pub fn complete_reprojection<F>(
        &mut self,
        version: u32,
        batch: RefreshBatch,
        semantics: &[ConversationSemantics],
        mut emit: F,
    ) -> Result<(), IndexError>
    where
        F: FnMut(RefreshEvent),
    {
        let tx = self.conn.transaction()?;
        apply_batch(&tx, &batch, &mut emit)?;
        apply_semantics(&tx, semantics)?;
        if let Some(reason) = batch.reject_reason {
            return Err(IndexError::RefreshRejected(reason));
        }
        tx.execute(
            "UPDATE index_metadata SET value=?1 WHERE key='projection_version'",
            [version.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_semantics(
        &mut self,
        semantics: &[ConversationSemantics],
    ) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        let generation: i64 = tx.query_row(
            "SELECT COALESCE(MAX(indexed_generation),0)+1 FROM conversation_versions",
            [],
            |row| row.get(0),
        )?;
        for record in semantics {
            tx.execute("UPDATE conversations SET conversation_state=?2,profile_quality=?3,indexed_generation=?4 WHERE id=?1",
                params![record.id, state_label(record.state), profile_label(record.profile), generation])?;
            tx.execute("INSERT INTO conversation_versions(id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,conversation_state,profile_quality,indexed_generation,project_json,project_kind,project_id,project_name,tokens_json,token_total)
                SELECT id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,conversation_state,profile_quality,indexed_generation,project_json,project_kind,project_id,project_name,tokens_json,token_total FROM conversations WHERE id=?1", [&record.id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn source_change(&self, candidate: &SourceRecord) -> Result<SourceChange, IndexError> {
        let found = self.conn.query_row("SELECT canonical_path,size,modified_ns,fingerprint,safe_offset,missing FROM sources WHERE logical_id=?1",
            [&candidate.logical_id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,u64>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,u64>(4)?,r.get::<_,bool>(5)?))).optional()?;
        Ok(match found {
            None => SourceChange::New,
            Some((_, _, _, _, _, true)) => SourceChange::Reactivated,
            Some((path, _, _, _, _, false))
                if path != candidate.canonical_path.to_string_lossy() =>
            {
                SourceChange::Moved { from: path.into() }
            }
            Some((_, size, mtime, fp, _, false))
                if size == candidate.size
                    && mtime == candidate.modified_ns
                    && fp == candidate.fingerprint =>
            {
                SourceChange::Unchanged
            }
            Some((_, size, _, fp, offset, false))
                if candidate.size >= size && fp == candidate.fingerprint =>
            {
                SourceChange::Appended {
                    from_offset: offset,
                }
            }
            Some(_) => SourceChange::Replaced,
        })
    }

    pub fn profile(&self, root_id: &str) -> Result<Option<ConversationProfile>, IndexError> {
        let conversation = self
            .conn
            .query_row(
            "SELECT id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,project_json,tokens_json FROM conversations WHERE id=?1",
                [root_id],
                row_to_conversation,
            )
            .optional()?;
        let Some(conversation) = conversation else {
            return Ok(None);
        };
        let mut statement = self.conn.prepare(
            "SELECT id,root_id,parent_id,agent_path,task_name,task_excerpt,role,nickname,model,effort,status,depth,evidence_complete,title,tokens_json FROM agents WHERE root_id=?1 ORDER BY depth,id",
        )?;
        let agents = statement
            .query_map([root_id], |row| {
                Ok(AgentRecord {
                    id: row.get(0)?,
                    root_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    agent_path: row.get(3)?,
                    task_name: row.get(4)?,
                    task_excerpt: row.get(5)?,
                    role: row.get(6)?,
                    nickname: row.get(7)?,
                    model: row.get(8)?,
                    effort: row.get(9)?,
                    status: row.get(10)?,
                    depth: row.get(11)?,
                    evidence_complete: row.get(12)?,
                    title: row.get(13)?,
                    tokens: decode_json(row.get::<_, Option<String>>(14)?, ProfileFact::unknown())?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ConversationProfile {
            conversation,
            agents,
        }))
    }

    pub fn agent(&self, agent_id: &str) -> Result<Option<AgentRecord>, IndexError> {
        self.conn
            .query_row(
                "SELECT id,root_id,parent_id,agent_path,task_name,task_excerpt,role,nickname,model,effort,status,depth,evidence_complete,title,tokens_json FROM agents WHERE id=?1",
                [agent_id],
                |row| Ok(AgentRecord {
                    id: row.get(0)?, root_id: row.get(1)?, parent_id: row.get(2)?,
                    agent_path: row.get(3)?, task_name: row.get(4)?, task_excerpt: row.get(5)?,
                    role: row.get(6)?, nickname: row.get(7)?, model: row.get(8)?, effort: row.get(9)?,
                    status: row.get(10)?, depth: row.get(11)?, evidence_complete: row.get(12)?,
                    title: row.get(13)?, tokens: decode_json(row.get::<_, Option<String>>(14)?, ProfileFact::unknown())?,
                }),
            )
            .optional()
            .map_err(IndexError::from)
    }

    pub fn stats(&self) -> Result<IndexStats, IndexError> {
        Ok(IndexStats {
            conversations: self
                .conn
                .query_row("SELECT count(*) FROM conversations", [], |r| r.get(0))?,
            agents: self
                .conn
                .query_row("SELECT count(*) FROM agents", [], |r| r.get(0))?,
            sources: self
                .conn
                .query_row("SELECT count(*) FROM sources", [], |r| r.get(0))?,
            missing_sources: self.conn.query_row(
                "SELECT count(*) FROM sources WHERE missing=1",
                [],
                |r| r.get(0),
            )?,
        })
    }

    pub fn app_metadata_status(&self) -> Result<String, IndexError> {
        self.conn
            .query_row(
                "SELECT value FROM index_metadata WHERE key='app_metadata_status'",
                [],
                |row| row.get(0),
            )
            .map_err(IndexError::from)
    }

    /// Clear only the rebuildable catalog. Source rollouts and Codex state DBs are untouched.
    pub fn reset(&mut self) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM agents", [])?;
        tx.execute("DELETE FROM agent_versions", [])?;
        tx.execute("DELETE FROM conversations", [])?;
        tx.execute("DELETE FROM conversation_versions", [])?;
        tx.execute("DELETE FROM sources", [])?;
        tx.commit()?;
        Ok(())
    }

    pub fn prune_missing(&mut self, missing_before: i64) -> Result<usize, IndexError> {
        Ok(self.conn.execute(
            "DELETE FROM sources WHERE missing=1 AND missing_since <= ?1",
            [missing_before],
        )?)
    }
}

fn apply_batch<F: FnMut(RefreshEvent)>(
    tx: &Transaction<'_>,
    batch: &RefreshBatch,
    emit: &mut F,
) -> Result<(), IndexError> {
    let generation: i64 = tx.query_row(
        "SELECT MAX(value)+1 FROM (SELECT COALESCE(MAX(indexed_generation),0) value FROM conversation_versions UNION ALL SELECT COALESCE(MAX(indexed_generation),0) FROM agent_versions)",
        [],
        |r| r.get(0),
    )?;
    tx.execute(
        "UPDATE index_metadata SET value=?1 WHERE key='app_metadata_status'",
        [batch
            .app_metadata_diagnostic
            .as_deref()
            .unwrap_or(if batch.app_metadata_refreshed {
                "ready"
            } else {
                "unavailable"
            })],
    )?;
    for c in &batch.conversations {
        let (project_kind, project_id, project_name) = project_columns(&c.project);
        let token_total = c
            .tokens
            .usage
            .value
            .as_ref()
            .map(|usage| usage.total_tokens);
        tx.execute("INSERT INTO conversations(id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,indexed_generation,project_json,project_kind,project_id,project_name,tokens_json,token_total) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
          ON CONFLICT(id) DO UPDATE SET title=CASE WHEN ?20=0 AND conversations.title_source='app' THEN conversations.title ELSE excluded.title END,title_source=CASE WHEN ?20=0 AND conversations.title_source='app' THEN conversations.title_source ELSE excluded.title_source END,cwd=excluded.cwd,created_at=excluded.created_at,last_activity_at=excluded.last_activity_at,archived=excluded.archived,model=excluded.model,status=excluded.status,agent_count=excluded.agent_count,max_depth=excluded.max_depth,profile_complete=excluded.profile_complete,indexed_generation=excluded.indexed_generation,project_json=CASE WHEN ?20=0 THEN conversations.project_json ELSE excluded.project_json END,project_kind=CASE WHEN ?20=0 THEN conversations.project_kind ELSE excluded.project_kind END,project_id=CASE WHEN ?20=0 THEN conversations.project_id ELSE excluded.project_id END,project_name=CASE WHEN ?20=0 THEN conversations.project_name ELSE excluded.project_name END,tokens_json=CASE WHEN excluded.token_total IS NULL AND ?20=0 THEN conversations.tokens_json ELSE excluded.tokens_json END,token_total=CASE WHEN excluded.token_total IS NULL AND ?20=0 THEN conversations.token_total ELSE excluded.token_total END",
          params![c.id,c.title,c.title_source,c.cwd,c.created_at,c.last_activity_at,c.archived,c.model,c.status,c.agent_count,c.max_depth,c.profile_complete,generation,serde_json::to_string(&c.project).unwrap(),project_kind,project_id,project_name,serde_json::to_string(&c.tokens).unwrap(),token_total,batch.app_metadata_refreshed])?;
        tx.execute(
            "UPDATE conversations SET conversation_state=?2,profile_quality=?3 WHERE id=?1",
            params![
                c.id,
                if c.archived { "archived" } else { "active" },
                if c.profile_complete {
                    "complete"
                } else {
                    "partial"
                }
            ],
        )?;
        tx.execute("INSERT INTO conversation_versions(id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,conversation_state,profile_quality,indexed_generation,project_json,project_kind,project_id,project_name,tokens_json,token_total)
          SELECT id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,conversation_state,profile_quality,indexed_generation,project_json,project_kind,project_id,project_name,tokens_json,token_total FROM conversations WHERE id=?1",
          [&c.id])?;
    }
    // Keep snapshots bounded while always retaining the latest version of every
    // conversation. Cursors older than this explicit window expire cleanly.
    tx.execute(
        "DELETE FROM conversation_versions
         WHERE indexed_generation < ?1
           AND indexed_generation < (SELECT MAX(newer.indexed_generation) FROM conversation_versions newer WHERE newer.id=conversation_versions.id)",
        [generation.saturating_sub(SNAPSHOT_RETENTION_GENERATIONS - 1)],
    )?;
    for agent in &batch.agents {
        tx.execute(
            "INSERT INTO agents(id,root_id,parent_id,agent_path,task_name,task_excerpt,role,nickname,model,effort,status,depth,evidence_complete,title,tokens_json,token_total) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(id) DO UPDATE SET root_id=excluded.root_id,parent_id=excluded.parent_id,agent_path=excluded.agent_path,task_name=excluded.task_name,task_excerpt=excluded.task_excerpt,role=excluded.role,nickname=excluded.nickname,model=excluded.model,effort=excluded.effort,status=excluded.status,depth=excluded.depth,evidence_complete=excluded.evidence_complete,title=excluded.title,tokens_json=excluded.tokens_json,token_total=excluded.token_total",
            params![agent.id,agent.root_id,agent.parent_id,agent.agent_path,agent.task_name,agent.task_excerpt,agent.role,agent.nickname,agent.model,agent.effort,agent.status,agent.depth,agent.evidence_complete,agent.title,serde_json::to_string(&agent.tokens).unwrap(),agent.tokens.value.as_ref().map(|usage| usage.total_tokens)],
        )?;
        tx.execute(
            "INSERT INTO agent_versions(id,root_id,parent_id,agent_path,task_name,task_excerpt,role,nickname,model,effort,status,depth,evidence_complete,indexed_generation,title,tokens_json,token_total)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![agent.id,agent.root_id,agent.parent_id,agent.agent_path,agent.task_name,agent.task_excerpt,agent.role,agent.nickname,agent.model,agent.effort,agent.status,agent.depth,agent.evidence_complete,generation,agent.title,serde_json::to_string(&agent.tokens).unwrap(),agent.tokens.value.as_ref().map(|usage| usage.total_tokens)],
        )?;
    }
    tx.execute(
        "DELETE FROM agent_versions WHERE indexed_generation < ?1
         AND indexed_generation < (SELECT MAX(newer.indexed_generation) FROM agent_versions newer WHERE newer.id=agent_versions.id)",
        [generation.saturating_sub(SNAPSHOT_RETENTION_GENERATIONS - 1)],
    )?;
    for s in &batch.sources {
        tx.execute("INSERT INTO sources(logical_id,canonical_path,size,modified_ns,fingerprint,safe_offset,archived,missing,missing_since) VALUES(?1,?2,?3,?4,?5,?6,?7,0,NULL)
          ON CONFLICT(logical_id) DO UPDATE SET canonical_path=excluded.canonical_path,size=excluded.size,modified_ns=excluded.modified_ns,fingerprint=excluded.fingerprint,safe_offset=excluded.safe_offset,archived=excluded.archived,missing=0,missing_since=NULL",
          params![s.logical_id,s.canonical_path.to_string_lossy(),s.size,s.modified_ns,s.fingerprint,s.safe_offset,s.archived])?;
    }
    if batch.discovered_all_sources {
        tx.execute("UPDATE sources SET missing=1,missing_since=COALESCE(missing_since,unixepoch()) WHERE logical_id NOT IN (SELECT value FROM json_each(?1))",
          [serde_json::to_string(&batch.sources.iter().map(|s| &s.logical_id).collect::<Vec<_>>()).unwrap()])?;
        let mut stmt = tx.prepare("SELECT logical_id FROM sources WHERE missing=1")?;
        for id in stmt.query_map([], |r| r.get(0))? {
            emit(RefreshEvent::SourceMissing { logical_id: id? });
        }
    }
    Ok(())
}

fn apply_semantics(
    tx: &Transaction<'_>,
    records: &[ConversationSemantics],
) -> Result<(), IndexError> {
    for semantics in records {
        tx.execute(
            "UPDATE conversations SET conversation_state=?2,profile_quality=?3 WHERE id=?1",
            params![
                semantics.id,
                state_label(semantics.state),
                profile_label(semantics.profile)
            ],
        )?;
        tx.execute("UPDATE conversation_versions SET conversation_state=?2,profile_quality=?3 WHERE id=?1 AND indexed_generation=(SELECT indexed_generation FROM conversations WHERE id=?1)",
            params![semantics.id, state_label(semantics.state), profile_label(semantics.profile)])?;
    }
    Ok(())
}

fn max_generation(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT MAX(value) FROM (SELECT COALESCE(MAX(indexed_generation),0) value FROM conversation_versions UNION ALL SELECT COALESCE(MAX(indexed_generation),0) FROM agent_versions)",
        [],
        |r| r.get(0),
    )
}

fn generation_is_retained(conn: &Connection, generation: i64) -> Result<bool, IndexError> {
    let latest = max_generation(conn)?;
    Ok(generation <= latest
        && generation >= latest.saturating_sub(SNAPSHOT_RETENTION_GENERATIONS - 1))
}

fn filter_signature(filter: &ConversationFilter) -> String {
    serde_json::json!({
        "archived": filter.archived,
        "cwd": filter.cwd,
        "model": filter.model,
        "status": filter.status,
        "query": filter.query,
        "project": format!("{:?}", filter.project),
    })
    .to_string()
}

fn sort_expressions(field: SortField) -> (&'static str, &'static str, bool) {
    match field {
        SortField::Updated => ("last_activity_at", "CASE WHEN last_activity_at='' THEN 1 ELSE 0 END", false),
        SortField::Title => ("LOWER(title)", "CASE WHEN 1 THEN 0 END", false),
        SortField::Agents => ("agent_count", "CASE WHEN 1 THEN 0 END", true),
        SortField::Depth => ("max_depth", "CASE WHEN 1 THEN 0 END", true),
        SortField::State => ("CASE conversation_state WHEN 'active' THEN 0 WHEN 'archived' THEN 1 ELSE 2 END", "CASE WHEN 1 THEN 0 END", true),
        SortField::Profile => ("CASE profile_quality WHEN 'complete' THEN 0 WHEN 'partial' THEN 1 WHEN 'conflicting' THEN 2 WHEN 'updating' THEN 3 ELSE 4 END", "CASE WHEN 1 THEN 0 END", true),
        SortField::Project => ("LOWER(COALESCE(project_name,''))", "project_kind", false),
        SortField::Tokens => ("COALESCE(token_total,0)", "CASE WHEN token_total IS NULL THEN 1 ELSE 0 END", true),
    }
}

fn snapshot_total(
    conn: &Connection,
    generation: i64,
    filter: &ConversationFilter,
) -> Result<u64, IndexError> {
    let archived = filter.archived.map(i64::from);
    Ok(conn.query_row("WITH snapshot AS (
      SELECT cv.* FROM conversation_versions cv JOIN (
        SELECT id,MAX(indexed_generation) AS generation FROM conversation_versions WHERE indexed_generation<=?1 GROUP BY id
      ) latest ON latest.id=cv.id AND latest.generation=cv.indexed_generation
    ) SELECT COUNT(*) FROM snapshot WHERE (?2 IS NULL OR archived=?2) AND (?3 IS NULL OR cwd=?3) AND (?4 IS NULL OR model=?4) AND (?5 IS NULL OR status=?5)
      AND (?6 IS NULL OR title LIKE '%'||?6||'%' OR id LIKE '%'||?6||'%' OR cwd LIKE '%'||?6||'%' OR project_name LIKE '%'||?6||'%'
        OR EXISTS (SELECT 1 FROM agent_versions a JOIN (
          SELECT id,MAX(indexed_generation) AS generation FROM agent_versions WHERE indexed_generation<=?1 GROUP BY id
        ) latest_agent ON latest_agent.id=a.id AND latest_agent.generation=a.indexed_generation
        WHERE a.root_id=snapshot.id AND (a.id LIKE '%'||?6||'%' OR a.task_name LIKE '%'||?6||'%' OR a.role LIKE '%'||?6||'%' OR a.nickname LIKE '%'||?6||'%' OR a.model LIKE '%'||?6||'%' OR a.status LIKE '%'||?6||'%')))
      AND (?7 IS NULL OR project_kind=?7) AND (?8 IS NULL OR project_id=?8)",
      params![generation, archived, filter.cwd, filter.model, filter.status, filter.query, project_kind(filter), project_id(filter)], |row| row.get(0))?)
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    name: &str,
    declaration: &str,
) -> Result<(), IndexError> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .any(|column| column == name);
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {name} {declaration}"),
            [],
        )?;
    }
    Ok(())
}
fn row_to_conversation(r: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationRecord> {
    Ok(ConversationRecord {
        id: r.get(0)?,
        title: r.get(1)?,
        title_source: r.get(2)?,
        cwd: r.get(3)?,
        created_at: r.get(4)?,
        last_activity_at: r.get(5)?,
        archived: r.get(6)?,
        model: r.get(7)?,
        status: r.get(8)?,
        agent_count: r.get(9)?,
        max_depth: r.get(10)?,
        profile_complete: r.get(11)?,
        project: decode_json(r.get::<_, Option<String>>(12)?, ProfileFact::unknown())?,
        tokens: decode_json(
            r.get::<_, Option<String>>(13)?,
            TokenUsageSummary::default(),
        )?,
    })
}

fn decode_json<T: serde::de::DeserializeOwned>(
    value: Option<String>,
    default: T,
) -> rusqlite::Result<T> {
    Ok(value
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or(default))
}

fn project_columns(project: &ProfileFact<ProjectAssignment>) -> (i64, Option<&str>, Option<&str>) {
    match project.value.as_ref() {
        Some(ProjectAssignment::Assigned { id, name }) => (0, Some(id), Some(name)),
        Some(ProjectAssignment::Projectless) => (1, None, None),
        None => (2, None, None),
    }
}
fn project_kind(filter: &ConversationFilter) -> Option<i64> {
    filter.project.as_ref().map(|value| match value {
        ProjectFilter::Assigned(_) => 0,
        ProjectFilter::Projectless => 1,
        ProjectFilter::Unknown => 2,
    })
}
fn project_id(filter: &ConversationFilter) -> Option<&str> {
    match filter.project.as_ref() {
        Some(ProjectFilter::Assigned(id)) => Some(id),
        _ => None,
    }
}

fn parse_state(value: &str) -> ConversationState {
    match value {
        "archived" => ConversationState::Archived,
        "missing" => ConversationState::Missing,
        _ => ConversationState::Active,
    }
}
fn parse_profile(value: &str) -> ProfileQuality {
    match value {
        "complete" => ProfileQuality::Complete,
        "conflicting" => ProfileQuality::Conflicting,
        "updating" => ProfileQuality::Updating,
        "error" => ProfileQuality::Error,
        _ => ProfileQuality::Partial,
    }
}
fn state_label(value: ConversationState) -> &'static str {
    match value {
        ConversationState::Active => "active",
        ConversationState::Archived => "archived",
        ConversationState::Missing => "missing",
    }
}
fn profile_label(value: ProfileQuality) -> &'static str {
    match value {
        ProfileQuality::Complete => "complete",
        ProfileQuality::Partial => "partial",
        ProfileQuality::Conflicting => "conflicting",
        ProfileQuality::Updating => "updating",
        ProfileQuality::Error => "error",
    }
}

#[cfg(unix)]
fn set_dir_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
#[cfg(not(unix))]
fn set_dir_permissions(_: &Path) -> std::io::Result<()> {
    Ok(())
}
#[cfg(unix)]
fn set_file_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
fn set_file_permissions(_: &Path) -> std::io::Result<()> {
    Ok(())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS conversations(id TEXT PRIMARY KEY,title TEXT NOT NULL,title_source TEXT NOT NULL,cwd TEXT NOT NULL,created_at TEXT NOT NULL,last_activity_at TEXT NOT NULL,archived INTEGER NOT NULL,model TEXT,status TEXT,agent_count INTEGER NOT NULL,max_depth INTEGER NOT NULL,profile_complete INTEGER NOT NULL,indexed_generation INTEGER NOT NULL,conversation_state TEXT NOT NULL DEFAULT 'active',profile_quality TEXT NOT NULL DEFAULT 'partial',project_json TEXT,project_kind INTEGER NOT NULL DEFAULT 2,project_id TEXT,project_name TEXT,tokens_json TEXT,token_total INTEGER);
CREATE INDEX IF NOT EXISTS conversation_browse ON conversations(last_activity_at DESC,id DESC);
CREATE TABLE IF NOT EXISTS conversation_versions(id TEXT NOT NULL,title TEXT NOT NULL,title_source TEXT NOT NULL,cwd TEXT NOT NULL,created_at TEXT NOT NULL,last_activity_at TEXT NOT NULL,archived INTEGER NOT NULL,model TEXT,status TEXT,agent_count INTEGER NOT NULL,max_depth INTEGER NOT NULL,profile_complete INTEGER NOT NULL,conversation_state TEXT NOT NULL,profile_quality TEXT NOT NULL,indexed_generation INTEGER NOT NULL,project_json TEXT,project_kind INTEGER NOT NULL DEFAULT 2,project_id TEXT,project_name TEXT,tokens_json TEXT,token_total INTEGER,PRIMARY KEY(id,indexed_generation));
CREATE INDEX IF NOT EXISTS conversation_versions_snapshot ON conversation_versions(indexed_generation,id);
CREATE TABLE IF NOT EXISTS agents(id TEXT PRIMARY KEY,root_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,parent_id TEXT,agent_path TEXT,task_name TEXT,task_excerpt TEXT,role TEXT,nickname TEXT,model TEXT,effort TEXT,status TEXT NOT NULL,depth INTEGER NOT NULL,evidence_complete INTEGER NOT NULL,title TEXT NOT NULL DEFAULT '',tokens_json TEXT,token_total INTEGER);
CREATE INDEX IF NOT EXISTS agents_by_root ON agents(root_id,depth,id);
CREATE TABLE IF NOT EXISTS agent_versions(id TEXT NOT NULL,root_id TEXT NOT NULL,parent_id TEXT,agent_path TEXT,task_name TEXT,task_excerpt TEXT,role TEXT,nickname TEXT,model TEXT,effort TEXT,status TEXT NOT NULL,depth INTEGER NOT NULL,evidence_complete INTEGER NOT NULL,indexed_generation INTEGER NOT NULL,title TEXT NOT NULL DEFAULT '',tokens_json TEXT,token_total INTEGER,PRIMARY KEY(id,indexed_generation));
CREATE INDEX IF NOT EXISTS agent_versions_snapshot ON agent_versions(indexed_generation,id,root_id);
CREATE TABLE IF NOT EXISTS sources(logical_id TEXT PRIMARY KEY,canonical_path TEXT NOT NULL UNIQUE,size INTEGER NOT NULL,modified_ns INTEGER NOT NULL,fingerprint TEXT NOT NULL,safe_offset INTEGER NOT NULL,archived INTEGER NOT NULL,missing INTEGER NOT NULL DEFAULT 0,missing_since INTEGER);
CREATE TABLE IF NOT EXISTS index_metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
"#;

#[cfg(test)]
mod tests;
