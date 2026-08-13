//! Rebuildable SQLite-backed catalog for conversation profiles.
//!
//! The repository stores display metadata and excerpts only. Source rollouts remain
//! authoritative and are never opened for writing.

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

const SCHEMA_VERSION: i64 = 1;
pub const REQUIRED_PROJECTION_VERSION: u32 = 2;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRecord {
    pub id: String,
    pub root_id: String,
    pub parent_id: Option<String>,
    pub agent_path: Option<String>,
    pub task_name: Option<String>,
    pub task_excerpt: Option<String>,
    pub role: Option<String>,
    pub nickname: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub status: String,
    pub depth: u32,
    pub evidence_complete: bool,
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
        let _ = conn.execute(
            "ALTER TABLE conversations ADD COLUMN conversation_state TEXT NOT NULL DEFAULT 'active'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE conversations ADD COLUMN profile_quality TEXT NOT NULL DEFAULT 'partial'",
            [],
        );
        conn.execute(
            "INSERT OR IGNORE INTO index_metadata(key,value) VALUES('projection_version','0')",
            [],
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    pub fn refresh<F>(&mut self, batch: RefreshBatch, mut emit: F) -> Result<(), IndexError>
    where
        F: FnMut(RefreshEvent),
    {
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
        let snapshot_at = cursor
            .map(|c| c.snapshot_at.clone())
            .unwrap_or_else(|| max_generation(&self.conn).unwrap_or_default().to_string());
        let generation = snapshot_at
            .parse::<i64>()
            .map_err(|_| IndexError::InvalidCursor)?;
        if cursor.is_some_and(|cursor| cursor.order != order) {
            return Err(IndexError::InvalidCursor);
        }
        let mut stmt = self.conn.prepare(
            "SELECT id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,conversation_state,profile_quality
             FROM conversations WHERE indexed_generation <= ?1
             AND (?2 IS NULL OR archived=?2) AND (?3 IS NULL OR cwd=?3) AND (?4 IS NULL OR model=?4) AND (?5 IS NULL OR status=?5)
             AND (?6 IS NULL OR title LIKE '%'||?6||'%' OR id LIKE '%'||?6||'%' OR cwd LIKE '%'||?6||'%'
               OR EXISTS (SELECT 1 FROM agents a WHERE a.root_id=conversations.id AND
                 (a.id LIKE '%'||?6||'%' OR a.task_name LIKE '%'||?6||'%' OR a.role LIKE '%'||?6||'%'
                  OR a.nickname LIKE '%'||?6||'%' OR a.model LIKE '%'||?6||'%' OR a.status LIKE '%'||?6||'%')))")?;
        let archived = filter.archived.map(i64::from);
        let rows = stmt.query_map(
            params![
                generation,
                archived,
                filter.cwd,
                filter.model,
                filter.status,
                filter.query,
            ],
            |row| {
                Ok((
                    row_to_conversation(row)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                ))
            },
        )?;
        let mut indexed = rows.collect::<Result<Vec<_>, _>>()?;
        indexed.sort_by(|left, right| compare_indexed(left, right, order));
        let start = match cursor {
            Some(cursor) => {
                indexed
                    .iter()
                    .position(|(conversation, _, _)| conversation.id == cursor.id)
                    .ok_or(IndexError::InvalidCursor)?
                    + 1
            }
            None => 0,
        };
        let has_more = indexed.len() > start + limit;
        indexed = indexed.into_iter().skip(start).take(limit).collect();
        let semantics = indexed
            .iter()
            .map(|(record, state, profile)| {
                (
                    record.id.clone(),
                    (parse_state(state), parse_profile(profile)),
                )
            })
            .collect();
        let conversations: Vec<_> = indexed.into_iter().map(|(record, _, _)| record).collect();
        let next_cursor = if has_more {
            conversations.last().map(|c| BrowseCursor {
                snapshot_at,
                order,
                id: c.id.clone(),
            })
        } else {
            None
        };
        Ok(BrowsePage {
            conversations,
            next_cursor,
            semantics,
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
        apply_semantics(&tx, semantics)?;
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
                "SELECT id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete FROM conversations WHERE id=?1",
                [root_id],
                row_to_conversation,
            )
            .optional()?;
        let Some(conversation) = conversation else {
            return Ok(None);
        };
        let mut statement = self.conn.prepare(
            "SELECT id,root_id,parent_id,agent_path,task_name,task_excerpt,role,nickname,model,effort,status,depth,evidence_complete FROM agents WHERE root_id=?1 ORDER BY depth,id",
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
                "SELECT id,root_id,parent_id,agent_path,task_name,task_excerpt,role,nickname,model,effort,status,depth,evidence_complete FROM agents WHERE id=?1",
                [agent_id],
                |row| Ok(AgentRecord {
                    id: row.get(0)?, root_id: row.get(1)?, parent_id: row.get(2)?,
                    agent_path: row.get(3)?, task_name: row.get(4)?, task_excerpt: row.get(5)?,
                    role: row.get(6)?, nickname: row.get(7)?, model: row.get(8)?, effort: row.get(9)?,
                    status: row.get(10)?, depth: row.get(11)?, evidence_complete: row.get(12)?,
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

    /// Clear only the rebuildable catalog. Source rollouts and Codex state DBs are untouched.
    pub fn reset(&mut self) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM agents", [])?;
        tx.execute("DELETE FROM conversations", [])?;
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
        "SELECT COALESCE(MAX(indexed_generation),0)+1 FROM conversations",
        [],
        |r| r.get(0),
    )?;
    for c in &batch.conversations {
        tx.execute("INSERT INTO conversations(id,title,title_source,cwd,created_at,last_activity_at,archived,model,status,agent_count,max_depth,profile_complete,indexed_generation) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
          ON CONFLICT(id) DO UPDATE SET title=excluded.title,title_source=excluded.title_source,cwd=excluded.cwd,created_at=excluded.created_at,last_activity_at=excluded.last_activity_at,archived=excluded.archived,model=excluded.model,status=excluded.status,agent_count=excluded.agent_count,max_depth=excluded.max_depth,profile_complete=excluded.profile_complete,indexed_generation=excluded.indexed_generation",
          params![c.id,c.title,c.title_source,c.cwd,c.created_at,c.last_activity_at,c.archived,c.model,c.status,c.agent_count,c.max_depth,c.profile_complete,generation])?;
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
    }
    for agent in &batch.agents {
        tx.execute(
            "INSERT INTO agents VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET root_id=excluded.root_id,parent_id=excluded.parent_id,agent_path=excluded.agent_path,task_name=excluded.task_name,task_excerpt=excluded.task_excerpt,role=excluded.role,nickname=excluded.nickname,model=excluded.model,effort=excluded.effort,status=excluded.status,depth=excluded.depth,evidence_complete=excluded.evidence_complete",
            params![agent.id,agent.root_id,agent.parent_id,agent.agent_path,agent.task_name,agent.task_excerpt,agent.role,agent.nickname,agent.model,agent.effort,agent.status,agent.depth,agent.evidence_complete],
        )?;
    }
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
    }
    Ok(())
}

fn max_generation(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COALESCE(MAX(indexed_generation),0) FROM conversations",
        [],
        |r| r.get(0),
    )
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
    })
}

fn compare_indexed(
    left: &(ConversationRecord, String, String),
    right: &(ConversationRecord, String, String),
    order: BrowseOrder,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (left_record, left_state, left_profile) = left;
    let (right_record, right_state, right_profile) = right;
    let primary = match order.field {
        SortField::Updated => match (
            left_record.last_activity_at.is_empty(),
            right_record.last_activity_at.is_empty(),
        ) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => left_record
                .last_activity_at
                .cmp(&right_record.last_activity_at),
        },
        SortField::Title => left_record
            .title
            .to_lowercase()
            .cmp(&right_record.title.to_lowercase()),
        SortField::Agents => left_record.agent_count.cmp(&right_record.agent_count),
        SortField::Depth => left_record.max_depth.cmp(&right_record.max_depth),
        SortField::State => parse_state(left_state).cmp(&parse_state(right_state)),
        SortField::Profile => parse_profile(left_profile).cmp(&parse_profile(right_profile)),
    };
    let primary = if order.field == SortField::Updated
        && (left_record.last_activity_at.is_empty() || right_record.last_activity_at.is_empty())
    {
        primary
    } else if order.direction == SortDirection::Desc {
        primary.reverse()
    } else {
        primary
    };
    primary.then_with(|| left_record.id.cmp(&right_record.id))
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
CREATE TABLE IF NOT EXISTS conversations(id TEXT PRIMARY KEY,title TEXT NOT NULL,title_source TEXT NOT NULL,cwd TEXT NOT NULL,created_at TEXT NOT NULL,last_activity_at TEXT NOT NULL,archived INTEGER NOT NULL,model TEXT,status TEXT,agent_count INTEGER NOT NULL,max_depth INTEGER NOT NULL,profile_complete INTEGER NOT NULL,indexed_generation INTEGER NOT NULL,conversation_state TEXT NOT NULL DEFAULT 'active',profile_quality TEXT NOT NULL DEFAULT 'partial');
CREATE INDEX IF NOT EXISTS conversation_browse ON conversations(last_activity_at DESC,id DESC);
CREATE TABLE IF NOT EXISTS agents(id TEXT PRIMARY KEY,root_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,parent_id TEXT,agent_path TEXT,task_name TEXT,task_excerpt TEXT,role TEXT,nickname TEXT,model TEXT,effort TEXT,status TEXT NOT NULL,depth INTEGER NOT NULL,evidence_complete INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS agents_by_root ON agents(root_id,depth,id);
CREATE TABLE IF NOT EXISTS sources(logical_id TEXT PRIMARY KEY,canonical_path TEXT NOT NULL UNIQUE,size INTEGER NOT NULL,modified_ns INTEGER NOT NULL,fingerprint TEXT NOT NULL,safe_offset INTEGER NOT NULL,archived INTEGER NOT NULL,missing INTEGER NOT NULL DEFAULT 0,missing_since INTEGER);
CREATE TABLE IF NOT EXISTS index_metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
"#;

#[cfg(test)]
mod tests;
