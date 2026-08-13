use crate::cli::{Common, IndexAction};
use codex_spawns::{
    index::{
        AgentRecord, ConversationFilter, ConversationRecord, IndexOptions, ProfileIndex,
        RefreshBatch, SourceRecord,
    },
    interactive::{self, App, Command, Event, Page, Preferences},
    ScanResult, SpawnStatus,
};
use crossterm::{
    event::{self, Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread,
    time::Duration,
};

enum WorkerEvent {
    Progress {
        scanned: usize,
        total: Option<usize>,
    },
    Ready(codex_spawns::index::BrowsePage),
    Failed(String),
}

fn index_path(common: &Common) -> PathBuf {
    common
        .codex_home
        .clone()
        .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".codex")
        })
        .join("cache/codex-spawns/index.sqlite")
}

pub fn run_tui(common: &Common) -> Result<(), String> {
    if !io::stdout().is_terminal() {
        return Err("interactive mode requires a TTY".into());
    }
    let path = index_path(common);
    let index =
        ProfileIndex::open(IndexOptions { path: path.clone() }).map_err(|e| e.to_string())?;
    let page = index
        .browse(&ConversationFilter::default(), None, 25)
        .map_err(|e| e.to_string())?;
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(to_page(page)));
    // Stale-first: the cached page above is immediately usable while source
    // discovery and parsing happen on a worker thread.
    let mut refresh = Some(start_refresh(common.clone(), path.clone(), false));
    let mut terminal = TerminalGuard::enter().map_err(|e| e.to_string())?;
    loop {
        if let Some(receiver) = refresh.as_ref() {
            if drain_worker(receiver, &mut app)? {
                refresh = None;
            }
        }
        terminal
            .terminal
            .draw(|f| interactive::render(f, &app))
            .map_err(|e| e.to_string())?;
        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            if let Some(input) = map_event(event::read().map_err(|e| e.to_string())?) {
                for command in app.update(input) {
                    match command {
                        Command::Quit => return Ok(()),
                        Command::LoadMore { cursor } => {
                            let cursor = codex_spawns::index::BrowseCursor::decode(&cursor)
                                .map_err(|e| e.to_string())?;
                            let page = index
                                .browse(&ConversationFilter::default(), Some(&cursor), 25)
                                .map_err(|e| e.to_string())?;
                            app.update(Event::MoreConversationsLoaded(to_page(page)));
                        }
                        Command::LoadAgents { conversation_id } => {
                            if let Some(profile) =
                                index.profile(&conversation_id).map_err(|e| e.to_string())?
                            {
                                app.update(Event::AgentsLoaded {
                                    conversation_id,
                                    agents: profile
                                        .agents
                                        .into_iter()
                                        .map(|a| codex_spawns::interactive::AgentItem {
                                            id: a.id,
                                            parent_id: a.parent_id,
                                            depth: a.depth,
                                            status: status(&a.status),
                                            task_name: a
                                                .task_name
                                                .unwrap_or_else(|| "unnamed agent".into()),
                                            role: a.role,
                                            nickname: a.nickname,
                                            model: a.model,
                                            effort: a.effort,
                                            detail_loaded: false,
                                        })
                                        .collect(),
                                });
                            }
                        }
                        Command::LoadAgentDetail { agent_id } => {
                            if let Some(agent) =
                                index.agent(&agent_id).map_err(|e| e.to_string())?
                            {
                                app.update(Event::AgentDetailLoaded(summary_detail(agent)));
                            }
                        }
                        Command::OpenEvidence { agent_id } => {
                            let scan = crate::cli::load(common)?;
                            app.update(Event::AgentDetailLoaded(source_detail(
                                &scan, &agent_id, false,
                            )));
                        }
                        Command::OpenMessage { agent_id } => {
                            let scan = crate::cli::load(common)?;
                            app.update(Event::AgentDetailLoaded(source_detail(
                                &scan, &agent_id, true,
                            )));
                        }
                        Command::Refresh if refresh.is_none() => {
                            refresh = Some(start_refresh(common.clone(), path.clone(), false));
                        }
                        Command::Rebuild if refresh.is_none() => {
                            refresh = Some(start_refresh(common.clone(), path.clone(), true));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn summary_detail(agent: AgentRecord) -> codex_spawns::interactive::AgentDetail {
    let mut lines = vec![
        ("status".into(), agent.status),
        ("depth".into(), agent.depth.to_string()),
        (
            "parent".into(),
            agent.parent_id.unwrap_or_else(|| "unknown".into()),
        ),
        (
            "task".into(),
            agent.task_name.unwrap_or_else(|| "unknown".into()),
        ),
        (
            "model".into(),
            agent.model.unwrap_or_else(|| "unknown".into()),
        ),
        (
            "effort".into(),
            agent.effort.unwrap_or_else(|| "unknown".into()),
        ),
        (
            "role".into(),
            agent.role.unwrap_or_else(|| "unknown".into()),
        ),
        (
            "nickname".into(),
            agent.nickname.unwrap_or_else(|| "unknown".into()),
        ),
        (
            "agent path".into(),
            agent.agent_path.unwrap_or_else(|| "unknown".into()),
        ),
        (
            "evidence complete".into(),
            agent.evidence_complete.to_string(),
        ),
    ];
    if let Some(excerpt) = agent.task_excerpt {
        lines.push(("message excerpt".into(), excerpt));
    }
    codex_spawns::interactive::AgentDetail {
        agent_id: agent.id,
        lines,
    }
}

fn source_detail(
    scan: &ScanResult,
    agent_id: &str,
    include_message: bool,
) -> codex_spawns::interactive::AgentDetail {
    let attempt = scan
        .spawn_attempts
        .iter()
        .find(|a| a.child_thread_id.as_deref() == Some(agent_id) || a.id == agent_id);
    let session = scan.agent_sessions.iter().find(|a| a.id == agent_id);
    let mut lines = Vec::new();
    if let Some(a) = attempt {
        lines.push((
            "created".into(),
            a.created_at
                .value
                .clone()
                .unwrap_or_else(|| "unknown".into()),
        ));
        lines.push((
            "created confidence".into(),
            format!("{:?}", a.created_at.confidence).to_lowercase(),
        ));
        lines.push((
            "evidence sources".into(),
            serde_json::to_string(&a.evidence).unwrap_or_default(),
        ));
        if let Some(error) = a.output_error.value.clone() {
            lines.push(("output error".into(), error));
        }
        if include_message {
            lines.push((
                "message".into(),
                a.message.value.clone().unwrap_or_else(|| "unknown".into()),
            ));
        }
    }
    if let Some(s) = session {
        lines.push(("event count".into(), s.event_count.to_string()));
        lines.push(("parse errors".into(), s.parse_errors.to_string()));
        lines.push(("rollout".into(), s.path.display().to_string()));
        lines.push((
            "model provenance".into(),
            serde_json::to_string(&s.model.provenance).unwrap_or_default(),
        ));
    }
    if lines.is_empty() {
        lines.push(("source".into(), "No source evidence found".into()));
    }
    codex_spawns::interactive::AgentDetail {
        agent_id: agent_id.into(),
        lines,
    }
}

fn start_refresh(common: Common, path: PathBuf, rebuild: bool) -> Receiver<WorkerEvent> {
    let (sender, receiver) = mpsc::sync_channel(16);
    thread::spawn(move || {
        if let Err(error) = refresh_worker(&common, path, rebuild, &sender) {
            let _ = sender.send(WorkerEvent::Failed(error));
        }
    });
    receiver
}

fn refresh_worker(
    common: &Common,
    path: PathBuf,
    rebuild: bool,
    sender: &SyncSender<WorkerEvent>,
) -> Result<(), String> {
    sender
        .send(WorkerEvent::Progress {
            scanned: 0,
            total: None,
        })
        .map_err(|_| "interactive refresh was cancelled".to_string())?;
    let (files, dbs) = crate::cli::discover(common)?;
    let total = files.len() + dbs.len();
    sender
        .send(WorkerEvent::Progress {
            scanned: 0,
            total: Some(total),
        })
        .map_err(|_| "interactive refresh was cancelled".to_string())?;
    let scan = codex_spawns::scan_sources(&files, &dbs).map_err(|e| e.to_string())?;
    sender
        .send(WorkerEvent::Progress {
            scanned: total,
            total: Some(total),
        })
        .map_err(|_| "interactive refresh was cancelled".to_string())?;
    let batch = refresh_batch(&scan, &files, &dbs)?;
    let mut index = ProfileIndex::open(IndexOptions { path }).map_err(|e| e.to_string())?;
    if rebuild {
        index.reset().map_err(|e| e.to_string())?;
    }
    index.refresh(batch, |_| {}).map_err(|e| e.to_string())?;
    let page = index
        .browse(&ConversationFilter::default(), None, 25)
        .map_err(|e| e.to_string())?;
    sender
        .send(WorkerEvent::Ready(page))
        .map_err(|_| "interactive refresh was cancelled".to_string())
}

/// Returns true once this worker has reached a terminal state.
fn drain_worker(receiver: &Receiver<WorkerEvent>, app: &mut App) -> Result<bool, String> {
    loop {
        match receiver.try_recv() {
            Ok(WorkerEvent::Progress { scanned, total }) => {
                app.update(Event::RefreshProgress(
                    codex_spawns::interactive::RefreshProgress { scanned, total },
                ));
            }
            Ok(WorkerEvent::Ready(page)) => {
                app.update(Event::RefreshReady(to_page(page)));
                return Ok(true);
            }
            Ok(WorkerEvent::Failed(error)) => return Err(error),
            Err(TryRecvError::Empty) => return Ok(false),
            Err(TryRecvError::Disconnected) => {
                return Err("interactive refresh worker stopped unexpectedly".into())
            }
        }
    }
}
fn status(s: &str) -> codex_spawns::interactive::AgentStatus {
    use codex_spawns::interactive::AgentStatus::*;
    match s {
        "requested" => Requested,
        "failed" => Failed,
        "state-only" => StateOnly,
        "orphan" => Orphan,
        "complete" => Complete,
        _ => Spawned,
    }
}
fn to_page(
    p: codex_spawns::index::BrowsePage,
) -> Page<codex_spawns::interactive::ConversationItem> {
    Page {
        items: p
            .conversations
            .into_iter()
            .map(|c| codex_spawns::interactive::ConversationItem {
                id: c.id,
                title: c.title,
                cwd: c.cwd,
                last_activity_at: c.last_activity_at,
                archived: c.archived,
                agent_count: c.agent_count as usize,
                max_depth: c.max_depth,
                profile_complete: c.profile_complete,
            })
            .collect(),
        next_cursor: p.next_cursor.map(|c| c.encode()),
        approximate_total: None,
    }
}
fn map_event(e: TerminalEvent) -> Option<Event> {
    match e {
        TerminalEvent::Resize(width, height) => Some(Event::Resize { width, height }),
        TerminalEvent::Key(k) if k.kind == KeyEventKind::Press => match k.code {
            KeyCode::Up => Some(Event::Up),
            KeyCode::Down => Some(Event::Down),
            KeyCode::Enter => Some(Event::Enter),
            KeyCode::Esc | KeyCode::Backspace => Some(Event::Back),
            KeyCode::Tab if k.modifiers.contains(KeyModifiers::SHIFT) => Some(Event::BackTab),
            KeyCode::Tab => Some(Event::Tab),
            KeyCode::Char(c) => Some(Event::Key(c)),
            _ => None,
        },
        _ => None,
    }
}
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}
impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen)?;
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(out))?,
        })
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

pub fn run_index(action: IndexAction, common: &Common) -> Result<(), String> {
    let path = index_path(common);
    match action {
        IndexAction::Status => {
            let index = ProfileIndex::open(IndexOptions { path: path.clone() })
                .map_err(|e| e.to_string())?;
            let stats = index.stats().map_err(|e| e.to_string())?;
            println!(
                "index: {}\nstatus: ready\nconversations: {}\nagents: {}\nsources: {}\nmissing_sources: {}",
                path.display(),
                stats.conversations, stats.agents, stats.sources, stats.missing_sources
            );
        }
        IndexAction::Refresh | IndexAction::Rebuild => {
            let rebuild = matches!(action, IndexAction::Rebuild);
            let (files, dbs) = crate::cli::discover(common)?;
            let scan = codex_spawns::scan_sources(&files, &dbs).map_err(|e| e.to_string())?;
            let batch = refresh_batch(&scan, &files, &dbs)?;
            let mut index = ProfileIndex::open(IndexOptions { path }).map_err(|e| e.to_string())?;
            if rebuild {
                index.reset().map_err(|e| e.to_string())?;
            }
            index.refresh(batch, |_| {}).map_err(|e| e.to_string())?;
            let stats = index.stats().map_err(|e| e.to_string())?;
            println!(
                "indexed: {} conversations, {} agents, {} sources",
                stats.conversations, stats.agents, stats.sources
            );
        }
        IndexAction::Prune { before } => {
            let mut index = ProfileIndex::open(IndexOptions { path }).map_err(|e| e.to_string())?;
            println!(
                "pruned: {}",
                index.prune_missing(before).map_err(|e| e.to_string())?
            );
        }
    }
    Ok(())
}

pub(crate) fn refresh_batch(
    scan: &ScanResult,
    files: &[PathBuf],
    dbs: &[PathBuf],
) -> Result<RefreshBatch, String> {
    let mut roots = Vec::new();
    let mut agents = Vec::new();
    let mut root_for_agent = HashMap::<String, String>::new();
    for root in &scan.root_conversations {
        root_for_agent.insert(root.id.clone(), root.id.clone());
    }
    // Resolve child ancestry repeatedly because rollout discovery order is not guaranteed.
    for _ in 0..=scan.agent_sessions.len() {
        let mut changed = false;
        for agent in &scan.agent_sessions {
            if root_for_agent.contains_key(&agent.id) {
                continue;
            }
            if let Some(parent) = agent.parent_thread_id.value.as_ref() {
                if let Some(root) = root_for_agent.get(parent).cloned() {
                    root_for_agent.insert(agent.id.clone(), root);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let unresolved = scan
        .agent_sessions
        .iter()
        .any(|a| !root_for_agent.contains_key(&a.id))
        || scan.spawn_attempts.iter().any(|a| {
            a.parent_thread_id
                .as_ref()
                .is_none_or(|p| !root_for_agent.contains_key(p))
        });
    for root in &scan.root_conversations {
        let created = root.created_at.value.clone().unwrap_or_default();
        let cwd = root.cwd.value.clone().unwrap_or_default();
        let related = scan
            .spawn_attempts
            .iter()
            .filter(|a| {
                a.parent_thread_id
                    .as_ref()
                    .and_then(|p| root_for_agent.get(p))
                    == Some(&root.id)
            })
            .count() as u64;
        let max_depth = scan
            .agent_sessions
            .iter()
            .filter(|a| root_for_agent.get(&a.id) == Some(&root.id))
            .filter_map(|a| a.depth.value)
            .max()
            .unwrap_or(0);
        roots.push(ConversationRecord {
            id: root.id.clone(),
            title: fallback_title(&cwd, &created, &root.id),
            title_source: "derived".into(),
            cwd,
            created_at: created.clone(),
            last_activity_at: created,
            archived: is_archived(&root.path),
            model: root.model.value.clone(),
            status: None,
            agent_count: related,
            max_depth,
            profile_complete: root.parse_errors == 0,
        });
        agents.push(AgentRecord {
            id: root.id.clone(),
            root_id: root.id.clone(),
            parent_id: None,
            agent_path: None,
            task_name: Some("root conversation".into()),
            task_excerpt: None,
            role: Some("root".into()),
            nickname: None,
            model: root.model.value.clone(),
            effort: root.effort.value.clone(),
            status: "complete".into(),
            depth: 0,
            evidence_complete: root.parse_errors == 0,
        });
    }
    if unresolved {
        roots.push(ConversationRecord {
            id: "__unresolved__".into(),
            title: "Unresolved Agents".into(),
            title_source: "virtual".into(),
            cwd: String::new(),
            created_at: String::new(),
            last_activity_at: String::new(),
            archived: false,
            model: None,
            status: Some("orphan".into()),
            agent_count: 0,
            max_depth: 0,
            profile_complete: false,
        });
    }
    let session_by_id: HashMap<_, _> = scan.agent_sessions.iter().map(|a| (&a.id, a)).collect();
    let mut added = HashSet::new();
    for attempt in &scan.spawn_attempts {
        let id = attempt
            .child_thread_id
            .clone()
            .unwrap_or_else(|| attempt.id.clone());
        if !added.insert(id.clone()) {
            continue;
        }
        let root_id = attempt
            .parent_thread_id
            .as_ref()
            .and_then(|p| root_for_agent.get(p))
            .cloned()
            .unwrap_or_else(|| "__unresolved__".into());
        let session = attempt
            .child_thread_id
            .as_ref()
            .and_then(|id| session_by_id.get(id).copied());
        agents.push(AgentRecord {
            id,
            root_id,
            parent_id: attempt.parent_thread_id.clone(),
            agent_path: attempt.agent_path.value.clone(),
            task_name: attempt.task_name.value.clone(),
            task_excerpt: excerpt(attempt.message.value.as_deref()),
            role: attempt.agent_role.value.clone(),
            nickname: attempt.agent_nickname.value.clone(),
            model: attempt
                .effective_model
                .value
                .clone()
                .or_else(|| attempt.requested_model.value.clone()),
            effort: attempt
                .effective_effort
                .value
                .clone()
                .or_else(|| attempt.requested_effort.value.clone()),
            status: spawn_status(&attempt.status).into(),
            depth: attempt
                .depth
                .value
                .or_else(|| session.and_then(|s| s.depth.value))
                .unwrap_or(1),
            evidence_complete: attempt.output_error.value.is_none() && session.is_some(),
        });
    }
    let mut sources = Vec::new();
    for path in files.iter().chain(dbs.iter()) {
        sources.push(source_record(path, is_archived(path))?);
    }
    Ok(RefreshBatch {
        conversations: roots,
        agents,
        sources,
        discovered_all_sources: true,
        reject_reason: None,
    })
}

fn fallback_title(cwd: &str, created: &str, id: &str) -> String {
    let name = Path::new(cwd)
        .file_name()
        .and_then(|v| v.to_str())
        .filter(|v| !v.is_empty());
    match (name, created.is_empty()) {
        (Some(n), false) => format!("{n} · {created}"),
        (Some(n), true) => n.into(),
        _ => id.chars().take(12).collect(),
    }
}
fn excerpt(value: Option<&str>) -> Option<String> {
    value.map(|s| {
        if s.chars().count() > 120 {
            format!("{}…", s.chars().take(119).collect::<String>())
        } else {
            s.into()
        }
    })
}
fn spawn_status(s: &SpawnStatus) -> &'static str {
    match s {
        SpawnStatus::Requested => "requested",
        SpawnStatus::Spawned => "spawned",
        SpawnStatus::Failed => "failed",
        SpawnStatus::StateOnly => "state-only",
        SpawnStatus::Orphan => "orphan",
    }
}
fn is_archived(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == "archived_sessions")
}
fn source_record(path: &Path, archived: bool) -> Result<SourceRecord, String> {
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let metadata =
        fs::metadata(path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
    let mut file =
        fs::File::open(path).map_err(|e| format!("cannot fingerprint {}: {e}", path.display()))?;
    let mut prefix = vec![0; metadata.len().min(4096) as usize];
    file.read_exact(&mut prefix)
        .map_err(|e| format!("cannot fingerprint {}: {e}", path.display()))?;
    let logical_id = format!(
        "{}:{}",
        if path.extension().is_some_and(|e| e == "jsonl") {
            "rollout"
        } else {
            "state"
        },
        canonical_path.display()
    );
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0);
    Ok(SourceRecord {
        logical_id,
        canonical_path,
        size: metadata.len(),
        modified_ns,
        fingerprint: blake3::hash(&prefix).to_hex().to_string(),
        safe_offset: metadata.len(),
        archived,
    })
}
