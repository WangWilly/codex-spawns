use clap::{Args, Parser, Subcommand, ValueEnum};
use codex_spawns::{scan_sources, ScanResult, SpawnAttempt};
use serde_json::{json, Value};
use std::{
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
};

#[derive(Parser, Debug)]
#[command(
    name = "codex-spawns",
    version,
    about = "Inspect Codex conversations and spawned agents"
)]
pub struct Cli {
    #[command(flatten)]
    pub common: Common,
    #[command(subcommand)]
    pub command: Option<Action>,
}

#[derive(Args, Clone, Debug, Default)]
pub struct Common {
    #[arg(long, global = true)]
    pub codex_home: Option<PathBuf>,
    #[arg(long = "sessions-dir", global = true)]
    pub sessions_dirs: Vec<PathBuf>,
    #[arg(long = "file", global = true)]
    pub files: Vec<PathBuf>,
    #[arg(long = "state-db", global = true)]
    pub state_dbs: Vec<PathBuf>,
    #[arg(long, global = true)]
    pub no_archived: bool,
    #[arg(long, global = true)]
    pub no_state_db: bool,
    #[arg(long, value_enum, default_value = "table", global = true)]
    pub format: Format,
    #[arg(long, global = true)]
    pub include_message: bool,
    #[arg(long, default_value_t=0, value_parser=clap::value_parser!(u64), global=true)]
    pub limit: u64,
    #[arg(long = "session", alias = "session-id", global = true)]
    pub session: Option<String>,
    #[arg(long = "parent", alias = "parent-thread-id", global = true)]
    pub parent: Option<String>,
    #[arg(long = "child", alias = "child-thread-id", global = true)]
    pub child: Option<String>,
    #[arg(long, alias = "workdir", global = true)]
    pub cwd: Option<String>,
    #[arg(long, global = true)]
    pub model: Option<String>,
    #[arg(long, global = true)]
    pub role: Option<String>,
    #[arg(long, global = true)]
    pub status: Option<String>,
    #[arg(long, global = true)]
    pub since: Option<String>,
    #[arg(long, global = true)]
    pub until: Option<String>,
    #[arg(long, global = true)]
    pub reverse: bool,
    #[arg(long, global = true)]
    pub no_cache: bool,
    #[arg(long, global = true)]
    pub index_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Format {
    #[default]
    Table,
    Json,
    Jsonl,
    Csv,
}

#[derive(Subcommand, Debug)]
pub enum Action {
    #[command(alias = "ls")]
    List,
    #[command(alias = "inspect")]
    Show {
        identifier: String,
        #[arg(long)]
        evidence: bool,
    },
    #[command(alias = "session")]
    Sessions,
    Doctor,
    #[command(alias = "ui")]
    Interactive,
    Index {
        #[command(subcommand)]
        action: IndexAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum IndexAction {
    Status,
    Refresh,
    Rebuild,
    Prune {
        #[arg(long, default_value_t = 0)]
        before: i64,
    },
}

pub fn run() -> Result<(), String> {
    let mut cli = Cli::parse();
    // clap propagates global arguments for validation, but flattened boolean
    // values can be reset while merging subcommand matches. Preserve this
    // compatibility flag explicitly until the command model is flattened.
    if std::env::args_os().any(|arg| arg == "--no-state-db") {
        cli.common.no_state_db = true;
    }
    if std::env::args_os().any(|arg| arg == "--no-archived") {
        cli.common.no_archived = true;
    }
    // Values accepted after a subcommand live in clap's propagated match set;
    // reconstruct the discovery paths so legacy option placement remains valid.
    let argv: Vec<_> = std::env::args_os().collect();
    cli.common.files = option_paths(&argv, "--file");
    cli.common.sessions_dirs = option_paths(&argv, "--sessions-dir");
    cli.common.state_dbs = option_paths(&argv, "--state-db");
    if let Some(path) = option_paths(&argv, "--codex-home").into_iter().last() {
        cli.common.codex_home = Some(path);
    }
    match cli.command {
        Some(Action::Interactive) => crate::runtime::run_tui(&cli.common),
        Some(Action::Index { action }) => crate::runtime::run_index(action, &cli.common),
        Some(action) => run_command(action, &cli.common),
        None if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() => {
            crate::runtime::run_tui(&cli.common)
        }
        None => run_command(Action::List, &cli.common),
    }
}

fn option_paths(argv: &[std::ffi::OsString], name: &str) -> Vec<PathBuf> {
    argv.windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| PathBuf::from(&pair[1]))
        .collect()
}

fn run_command(action: Action, common: &Common) -> Result<(), String> {
    let scan = load(common)?;
    match action {
        Action::List => print_attempts(filtered(&scan, common), &scan, common),
        Action::Show {
            identifier,
            evidence,
        } => {
            let records = filtered(&scan, common);
            let found: Vec<_> = records
                .into_iter()
                .enumerate()
                .filter(|(i, r)| {
                    identifier == (i + 1).to_string()
                        || r.id.starts_with(&identifier)
                        || r.child_thread_id
                            .as_deref()
                            .is_some_and(|v| v.starts_with(&identifier))
                })
                .map(|(_, r)| r)
                .collect();
            if found.is_empty() {
                return Err(format!("no spawn record matches {identifier:?}"));
            }
            let values: Vec<_> = found
                .iter()
                .map(|r| attempt_json(r, &scan, common.include_message, evidence))
                .collect();
            print_values(&values, common.format)
        }
        Action::Sessions => {
            let mut values: Vec<Value> = scan
                .root_conversations
                .iter()
                .map(|r| serde_json::to_value(r).unwrap())
                .collect();
            values.extend(
                scan.agent_sessions
                    .iter()
                    .map(|r| serde_json::to_value(r).unwrap()),
            );
            print_values(&values, common.format)
        }
        Action::Doctor => {
            println!("{}", serde_json::to_string_pretty(&json!({
            "rollout_files": scan.rollout_files, "rollout_file_count": scan.rollout_files.len(),
            "state_databases": scan.state_databases, "state_database_count": scan.state_databases.len(),
            "session_count": scan.root_conversations.len()+scan.agent_sessions.len(),
            "spawn_record_count": scan.spawn_attempts.len(), "diagnostics": scan.diagnostics
        })).unwrap());
            Ok(())
        }
        _ => unreachable!(),
    }
}

pub(crate) fn load(common: &Common) -> Result<ScanResult, String> {
    let (files, dbs) = discover(common)?;
    scan_sources(&files, &dbs).map_err(|e| e.to_string())
}

pub(crate) fn discover(common: &Common) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let home = common
        .codex_home
        .clone()
        .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".codex")
        });
    let mut files = common.files.clone();
    let roots = if common.sessions_dirs.is_empty() && common.files.is_empty() {
        let mut v = vec![home.join("sessions")];
        if !common.no_archived {
            v.push(home.join("archived_sessions"));
        }
        v
    } else {
        common.sessions_dirs.clone()
    };
    for root in roots {
        collect_jsonl(&root, &mut files).map_err(|e| e.to_string())?;
    }
    files.sort();
    files.dedup();
    let mut dbs = common.state_dbs.clone();
    if !common.no_state_db {
        if let Ok(entries) = fs::read_dir(&home) {
            for e in entries.flatten() {
                let p = e.path();
                if p.file_name()
                    .and_then(|x| x.to_str())
                    .is_some_and(|n| n.starts_with("state_") && n.ends_with(".sqlite"))
                {
                    dbs.push(p);
                }
            }
        }
    }
    Ok((files, dbs))
}

fn collect_jsonl(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_file() {
        if path.extension().is_some_and(|x| x == "jsonl") {
            out.push(path.to_path_buf())
        }
        return Ok(());
    }
    for e in fs::read_dir(path)? {
        let p = e?.path();
        if p.is_dir() {
            collect_jsonl(&p, out)?
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p)
        }
    }
    Ok(())
}

fn filtered<'a>(scan: &'a ScanResult, c: &Common) -> Vec<&'a SpawnAttempt> {
    let mut v: Vec<_> = scan
        .spawn_attempts
        .iter()
        .filter(|r| {
            c.session.as_ref().is_none_or(|x| {
                r.parent_thread_id.as_ref() == Some(x) || r.child_thread_id.as_ref() == Some(x)
            }) && c
                .parent
                .as_ref()
                .is_none_or(|x| r.parent_thread_id.as_ref() == Some(x))
                && c.child
                    .as_ref()
                    .is_none_or(|x| r.child_thread_id.as_ref() == Some(x))
                && c.model.as_ref().is_none_or(|x| {
                    [
                        r.requested_model.value.as_ref(),
                        r.effective_model.value.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|v| v.to_lowercase().contains(&x.to_lowercase()))
                })
                && c.role.as_ref().is_none_or(|x| {
                    [
                        r.agent_type.value.as_ref(),
                        r.agent_role.value.as_ref(),
                        r.agent_nickname.value.as_ref(),
                        r.task_name.value.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|v| v.to_lowercase().contains(&x.to_lowercase()))
                })
                && c.status
                    .as_ref()
                    .is_none_or(|x| format!("{:?}", r.status).eq_ignore_ascii_case(x))
                && c.cwd.as_ref().is_none_or(|wanted| {
                    let wanted = Path::new(wanted);
                    session_cwd(scan, r.parent_thread_id.as_deref())
                        .into_iter()
                        .chain(session_cwd(scan, r.child_thread_id.as_deref()))
                        .any(|actual| Path::new(actual) == wanted)
                })
                && c.since.as_ref().is_none_or(|since| {
                    r.created_at
                        .value
                        .as_ref()
                        .is_some_and(|value| value >= since)
                })
                && c.until.as_ref().is_none_or(|until| {
                    r.created_at
                        .value
                        .as_ref()
                        .is_some_and(|value| value <= until)
                })
        })
        .collect();
    v.sort_by_key(|r| r.created_at.value.clone());
    if c.reverse {
        v.reverse()
    }
    if c.limit > 0 {
        v.truncate(c.limit as usize)
    }
    v
}

fn session_cwd<'a>(scan: &'a ScanResult, id: Option<&str>) -> Option<&'a str> {
    let id = id?;
    scan.root_conversations
        .iter()
        .find(|s| s.id == id)
        .and_then(|s| s.cwd.value.as_deref())
        .or_else(|| {
            scan.agent_sessions
                .iter()
                .find(|s| s.id == id)
                .and_then(|s| s.cwd.value.as_deref())
        })
}

fn rollout_path(scan: &ScanResult, id: Option<&str>) -> Option<String> {
    let id = id?;
    scan.root_conversations
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.path.display().to_string())
        .or_else(|| {
            scan.agent_sessions
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.path.display().to_string())
        })
}

fn attempt_json(r: &SpawnAttempt, scan: &ScanResult, message: bool, evidence: bool) -> Value {
    let parent_line = r.evidence.iter().find_map(|source| match source {
        codex_spawns::SourceRef::Rollout { line, .. } => *line,
        _ => None,
    });
    let child = r
        .child_thread_id
        .as_deref()
        .and_then(|id| scan.agent_sessions.iter().find(|s| s.id == id));
    let child_line = child.and_then(|s| {
        s.parent_thread_id
            .provenance
            .iter()
            .find_map(|source| match source {
                codex_spawns::SourceRef::Rollout { line, .. } => *line,
                _ => None,
            })
    });
    let state_source = r.evidence.iter().find_map(|source| match source {
        codex_spawns::SourceRef::StateDatabase { path, .. } => Some(path.display().to_string()),
        _ => None,
    });
    let source = if parent_line.is_some() {
        "rollout"
    } else {
        "state"
    };
    let mut v = json!({"id":r.id,"created_at":r.created_at.value,"status":format!("{:?}",r.status).to_lowercase(),"parent_thread_id":r.parent_thread_id,"child_thread_id":r.child_thread_id,"parent_path":rollout_path(scan,r.parent_thread_id.as_deref()),"child_path":rollout_path(scan,r.child_thread_id.as_deref()),"parent_cwd":session_cwd(scan,r.parent_thread_id.as_deref()),"child_cwd":session_cwd(scan,r.child_thread_id.as_deref()),"task_name":r.task_name.value,"message_excerpt":r.message.value.as_deref().map(|s| if s.chars().count()>180 {format!("{}…",s.chars().take(179).collect::<String>())} else{s.into()}),"agent_type":r.agent_type.value,"agent_role":r.agent_role.value,"agent_nickname":r.agent_nickname.value,"agent_path":r.agent_path.value,"requested_model":r.requested_model.value,"requested_effort":r.requested_effort.value,"fork_turns":r.fork_turns.value,"effective_model":r.effective_model.value,"effective_effort":r.effective_effort.value,"multi_agent_version":child.and_then(|s|s.multi_agent_version.value.as_deref()),"depth":r.depth.value,"source":source,"call_id":r.call_id,"parent_line":parent_line,"child_line":child_line,"output_line":r.output_line,"output_error":r.output_error.value,"state_status":r.state_status.value,"state_source":state_source});
    if message {
        v["message"] = json!(r.message.value)
    }
    if evidence {
        v["evidence"] = json!(r.evidence)
    }
    v
}
fn print_attempts(
    records: Vec<&SpawnAttempt>,
    scan: &ScanResult,
    c: &Common,
) -> Result<(), String> {
    let values: Vec<_> = records
        .iter()
        .map(|r| attempt_json(r, scan, c.include_message, false))
        .collect();
    match c.format {Format::Json=>println!("{}",serde_json::to_string_pretty(&json!({"records":values,"count":values.len(),"scanned_rollout_files":scan.rollout_files.len(),"diagnostics":scan.diagnostics})).unwrap()), _=>return print_values(&values,c.format)}
    Ok(())
}
fn print_values(values: &[Value], format: Format) -> Result<(), String> {
    match format {
        Format::Json => println!("{}", serde_json::to_string_pretty(values).unwrap()),
        Format::Jsonl => {
            for v in values {
                println!("{}", serde_json::to_string(v).unwrap())
            }
        }
        Format::Csv => {
            if let Some(Value::Object(first)) = values.first() {
                let keys: Vec<_> = first.keys().collect();
                println!(
                    "{}",
                    keys.iter()
                        .map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                for v in values {
                    println!(
                        "{}",
                        keys.iter()
                            .map(|k| v.get(*k).map(|x| x.to_string()).unwrap_or_default())
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                }
            }
        }
        Format::Table => {
            if values.is_empty() {
                println!("No records found.")
            } else {
                for (i, v) in values.iter().enumerate() {
                    println!(
                        "{}  {}  {}  {}",
                        i + 1,
                        v["created_at"].as_str().unwrap_or("-"),
                        v["task_name"].as_str().unwrap_or("-"),
                        v["status"].as_str().unwrap_or("-")
                    );
                }
            }
        }
    }
    Ok(())
}
