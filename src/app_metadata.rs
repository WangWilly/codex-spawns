use crate::{projection::project_plain_text, ProjectAssignment, TokenUsage};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppMetadataPaths {
    pub thread_catalog: PathBuf,
    pub global_state: PathBuf,
}

impl AppMetadataPaths {
    pub fn new(thread_catalog: impl Into<PathBuf>, global_state: impl Into<PathBuf>) -> Self {
        Self {
            thread_catalog: thread_catalog.into(),
            global_state: global_state.into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppThreadMetadata {
    pub title: Option<String>,
    /// Preserves zero separately from absent/null.
    pub tokens_used: Option<u64>,
    pub rowid: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppMetadataSnapshot {
    pub threads: HashMap<String, AppThreadMetadata>,
    pub projects: HashMap<String, ProjectAssignment>,
    pub thread_catalog_path: PathBuf,
    pub global_state_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum AppMetadataError {
    #[error("cannot read App thread catalog {path}: {source}")]
    Sqlite {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("cannot read App global state {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse App global state {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Read both App stores into a new snapshot. Callers only replace prior state on success.
pub fn load_app_metadata(
    paths: &AppMetadataPaths,
) -> Result<AppMetadataSnapshot, AppMetadataError> {
    let threads = read_threads(&paths.thread_catalog)?;
    let raw = fs::read(&paths.global_state).map_err(|source| AppMetadataError::Io {
        path: paths.global_state.clone(),
        source,
    })?;
    let state: Value = serde_json::from_slice(&raw).map_err(|source| AppMetadataError::Json {
        path: paths.global_state.clone(),
        source,
    })?;
    let catalog = state.get("local-projects").and_then(Value::as_object);
    let names = catalog
        .into_iter()
        .flatten()
        .map(|(id, value)| {
            let raw = value.get("name").and_then(Value::as_str).unwrap_or(id);
            (
                id.clone(),
                project_plain_text(raw).unwrap_or_else(|| id.clone()),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut projects = HashMap::new();
    if let Some(assignments) = state
        .get("thread-project-assignments")
        .and_then(Value::as_object)
    {
        for (thread, project) in assignments {
            // Older global-state snapshots stored the project id directly as
            // a string. Current App snapshots store an assignment object with
            // the id in `projectId`; accept both representations so stale or
            // missing catalog entries still fall through to Unknown.
            let id = project
                .as_str()
                .or_else(|| project.get("projectId").and_then(Value::as_str));
            if let Some(id) = id.and_then(|id| names.get(id).map(|_| id)) {
                let name = &names[id];
                projects.insert(
                    thread.clone(),
                    ProjectAssignment::Assigned {
                        id: id.into(),
                        name: name.clone(),
                    },
                );
            }
        }
    }
    let projectless = state
        .get("projectless-thread-ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<HashSet<_>>();
    for thread in projectless {
        projects.insert(thread.into(), ProjectAssignment::Projectless);
    }
    Ok(AppMetadataSnapshot {
        threads,
        projects,
        thread_catalog_path: paths.thread_catalog.clone(),
        global_state_path: paths.global_state.clone(),
    })
}

fn read_threads(path: &Path) -> Result<HashMap<String, AppThreadMetadata>, AppMetadataError> {
    let uri = format!("file:{}?mode=ro", path.to_string_lossy());
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|source| AppMetadataError::Sqlite {
        path: path.into(),
        source,
    })?;
    let mut stmt = conn
        .prepare("SELECT rowid, id, title, tokens_used FROM threads")
        .map_err(|source| AppMetadataError::Sqlite {
            path: path.into(),
            source,
        })?;
    let rows = stmt
        .query_map([], |row| {
            let title: Option<String> = row.get(2)?;
            let tokens: Option<i64> = row.get(3)?;
            Ok((
                row.get::<_, String>(1)?,
                AppThreadMetadata {
                    title: title.and_then(|value| project_plain_text(&value)),
                    tokens_used: tokens.and_then(|value| u64::try_from(value).ok()),
                    rowid: row.get(0)?,
                },
            ))
        })
        .map_err(|source| AppMetadataError::Sqlite {
            path: path.into(),
            source,
        })?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(|source| AppMetadataError::Sqlite {
            path: path.into(),
            source,
        })
}

pub(crate) fn app_total(total: u64) -> TokenUsage {
    TokenUsage {
        total_tokens: total,
        ..Default::default()
    }
}
