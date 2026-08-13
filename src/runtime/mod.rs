use crate::cli::{Common, IndexAction};
use codex_spawns::{
    index::{
        AgentRecord, BrowseOrder, ConversationFilter, ConversationRecord, ConversationState,
        IndexOptions, ProfileIndex, ProfileQuality, RefreshBatch,
        SortDirection as IndexSortDirection, SortField, SourceRecord,
    },
    interactive::{self, App, Command, Event, Page, Preferences},
    FactConfidence, ProfileFact, ProjectAssignment, ScanResult, SpawnStatus, TokenUsageSummary,
};
use crossterm::{
    event::{self, Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    event::{DisableMouseCapture, EnableMouseCapture},
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
    time::{Duration, Instant},
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
    if let Some(path) = &common.index_path {
        return path.clone();
    }
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

fn ephemeral_index_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "codex-spawns-no-cache-{}.sqlite",
        std::process::id()
    ))
}

fn selected_index_path(common: &Common) -> PathBuf {
    if common.no_cache {
        ephemeral_index_path()
    } else {
        index_path(common)
    }
}

fn preferences_path(common: &Common) -> PathBuf {
    selected_index_path(common)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("preferences.conf")
}

fn load_preferences(common: &Common) -> Preferences {
    if common.no_cache {
        return Preferences::default();
    }
    fs::read_to_string(preferences_path(common))
        .map(|value| Preferences::from_toml_like(&value))
        .unwrap_or_default()
}

fn save_preferences(common: &Common, preferences: &Preferences) -> Result<(), String> {
    if common.no_cache {
        return Ok(());
    }
    let path = preferences_path(common);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, preferences.to_toml_like()).map_err(|error| error.to_string())
}

fn cleanup_ephemeral_index(common: &Common, path: &Path) {
    if common.no_cache {
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}

pub fn run_tui(common: &Common) -> Result<(), String> {
    if !io::stdout().is_terminal() {
        return Err("interactive mode requires a TTY".into());
    }
    let path = selected_index_path(common);
    cleanup_ephemeral_index(common, &path);
    let index =
        ProfileIndex::open(IndexOptions { path: path.clone() }).map_err(|e| e.to_string())?;
    let page = index
        .browse_ordered(
            &ConversationFilter::default(),
            None,
            25,
            BrowseOrder::default(),
        )
        .map_err(|e| e.to_string())?;
    let mut app = App::new(load_preferences(common));
    app.update(Event::ConversationsLoaded(to_page(page)));
    // Stale-first: the cached page above is immediately usable while source
    // discovery and parsing happen on a worker thread.
    let mut refresh = Some(start_refresh(
        common.clone(),
        path.clone(),
        false,
        ConversationFilter::default(),
        BrowseOrder::default(),
        app.preferences().page_size,
    ));
    let mut terminal = TerminalGuard::enter().map_err(|e| e.to_string())?;
    let mut clicks = ClickTracker::default();
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
            if let Some(input) =
                map_event(event::read().map_err(|e| e.to_string())?, &app, &mut clicks)
            {
                for command in app.update(input) {
                    match command {
                        Command::Quit => {
                            save_preferences(common, app.preferences())?;
                            drop(index);
                            cleanup_ephemeral_index(common, &path);
                            return Ok(());
                        }
                        Command::LoadMore { cursor } => {
                            let cursor = codex_spawns::index::BrowseCursor::decode(&cursor)
                                .map_err(|e| e.to_string())?;
                            let page = index
                                .browse_ordered(
                                    &browse_filter(
                                        app.filter(),
                                        app.project_filter(),
                                        app.search().to_owned(),
                                    ),
                                    Some(&cursor),
                                    app.preferences().page_size,
                                    app_browse_order(&app),
                                )
                                .map_err(|e| e.to_string())?;
                            app.update(Event::MoreConversationsLoaded(to_page(page)));
                        }
                        Command::Search {
                            query,
                            filter,
                            project,
                        } => {
                            let page = index
                                .browse_ordered(
                                    &browse_filter(filter, &project, query),
                                    None,
                                    app.preferences().page_size,
                                    app_browse_order(&app),
                                )
                                .map_err(|e| e.to_string())?;
                            app.update(Event::ConversationsLoaded(to_page(page)));
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
                                            title: a.title,
                                            role: a.role,
                                            nickname: a.nickname,
                                            model: a.model,
                                            effort: a.effort,
                                            detail_loaded: false,
                                            tokens: token_display(&a.tokens, 1, 1),
                                        })
                                        .collect(),
                                });
                            }
                        }
                        Command::LoadAgentDetail { agent_id } => {
                            if let Some(agent) =
                                index.agent(&agent_id).map_err(|e| e.to_string())?
                            {
                                let conversation_tokens = if agent.id == agent.root_id {
                                    index
                                        .profile(&agent.root_id)
                                        .map_err(|e| e.to_string())?
                                        .map(|profile| profile.conversation.tokens)
                                } else {
                                    None
                                };
                                app.update(Event::AgentDetailLoaded(summary_detail(
                                    agent,
                                    conversation_tokens.as_ref(),
                                )));
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
                            refresh = Some(start_refresh(
                                common.clone(),
                                path.clone(),
                                false,
                                browse_filter(
                                    app.filter(),
                                    app.project_filter(),
                                    app.search().to_owned(),
                                ),
                                app_browse_order(&app),
                                app.preferences().page_size,
                            ));
                        }
                        Command::Rebuild if refresh.is_none() => {
                            refresh = Some(start_refresh(
                                common.clone(),
                                path.clone(),
                                true,
                                browse_filter(
                                    app.filter(),
                                    app.project_filter(),
                                    app.search().to_owned(),
                                ),
                                app_browse_order(&app),
                                app.preferences().page_size,
                            ));
                        }
                        Command::Sort { .. } => {
                            let page = index
                                .browse_ordered(
                                    &browse_filter(
                                        app.filter(),
                                        app.project_filter(),
                                        app.search().to_owned(),
                                    ),
                                    None,
                                    app.preferences().page_size,
                                    app_browse_order(&app),
                                )
                                .map_err(|e| e.to_string())?;
                            app.update(Event::ConversationsLoaded(to_page(page)));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn browse_filter(
    filter: codex_spawns::interactive::Filter,
    project: &codex_spawns::interactive::ProjectFilter,
    query: String,
) -> ConversationFilter {
    use codex_spawns::{
        index::ProjectFilter as IndexProjectFilter,
        interactive::{Filter, ProjectFilter},
    };
    ConversationFilter {
        archived: match filter {
            Filter::All => None,
            Filter::ActiveOnly => Some(false),
            Filter::ArchivedOnly => Some(true),
        },
        query: (!query.is_empty()).then_some(query),
        project: match project {
            ProjectFilter::All => None,
            ProjectFilter::Assigned(id) => Some(IndexProjectFilter::Assigned(id.clone())),
            ProjectFilter::NoProject => Some(IndexProjectFilter::Projectless),
            ProjectFilter::Unknown => Some(IndexProjectFilter::Unknown),
        },
        ..Default::default()
    }
}

fn app_browse_order(app: &App) -> BrowseOrder {
    use codex_spawns::interactive::{Sort, SortDirection};
    BrowseOrder {
        field: match app.preferences().sort {
            Sort::Updated => SortField::Updated,
            Sort::Title => SortField::Title,
            Sort::Project => SortField::Project,
            Sort::Tokens => SortField::Tokens,
            Sort::Agents => SortField::Agents,
            Sort::Depth => SortField::Depth,
            Sort::State => SortField::State,
            Sort::Profile => SortField::Profile,
        },
        direction: match app.sort_direction() {
            SortDirection::Ascending => IndexSortDirection::Asc,
            SortDirection::Descending => IndexSortDirection::Desc,
        },
    }
}

fn summary_detail(
    agent: AgentRecord,
    conversation_tokens: Option<&TokenUsageSummary>,
) -> codex_spawns::interactive::AgentDetail {
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
    lines.push((
        "tokens confidence".into(),
        format!("{:?}", agent.tokens.confidence).to_lowercase(),
    ));
    for conflict in &agent.tokens.conflicting_values {
        lines.push((
            "conflicting total tokens".into(),
            conflict.total_tokens.to_string(),
        ));
    }
    if let Some(usage) = agent.tokens.value {
        for (label, value) in [
            ("input tokens", usage.input_tokens),
            ("cached input tokens", usage.cached_input_tokens),
            ("output tokens", usage.output_tokens),
            ("reasoning output tokens", usage.reasoning_output_tokens),
            ("model context window", usage.model_context_window),
        ] {
            lines.push((
                label.into(),
                value.map_or_else(|| "unknown".into(), |value| value.to_string()),
            ));
        }
        lines.push(("total tokens".into(), usage.total_tokens.to_string()));
    }
    if let Some(summary) = conversation_tokens {
        lines.push((
            "conversation token coverage".into(),
            format!(
                "{}/{} sessions",
                summary.covered_sessions, summary.total_sessions
            ),
        ));
        if let Some(usage) = &summary.usage.value {
            lines.push((
                "conversation total tokens".into(),
                usage.total_tokens.to_string(),
            ));
        }
        for conflict in &summary.usage.conflicting_values {
            lines.push((
                "conversation conflicting total tokens".into(),
                conflict.total_tokens.to_string(),
            ));
        }
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

fn start_refresh(
    common: Common,
    path: PathBuf,
    rebuild: bool,
    filter: ConversationFilter,
    order: BrowseOrder,
    page_size: usize,
) -> Receiver<WorkerEvent> {
    let (sender, receiver) = mpsc::sync_channel(16);
    thread::spawn(move || {
        if let Err(error) =
            refresh_worker(&common, path, rebuild, filter, order, page_size, &sender)
        {
            let _ = sender.send(WorkerEvent::Failed(error));
        }
    });
    receiver
}

fn refresh_worker(
    common: &Common,
    path: PathBuf,
    rebuild: bool,
    filter: ConversationFilter,
    order: BrowseOrder,
    page_size: usize,
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
    let mut index = ProfileIndex::open(IndexOptions { path }).map_err(|e| e.to_string())?;
    if rebuild {
        index.reset().map_err(|e| e.to_string())?;
    }
    let reproject = index.needs_reprojection().map_err(|e| e.to_string())?;
    let (changed_files, changed_dbs) = if reproject {
        (files.clone(), dbs.clone())
    } else {
        changed_sources(&index, &files, &dbs)?
    };
    let app_candidate = app_ok_candidate(common);
    let (scan, app_ok) = crate::cli::scan_with_optional_app(
        common,
        if app_candidate {
            &files
        } else {
            &changed_files
        },
        if app_candidate { &dbs } else { &changed_dbs },
    )?;
    sender
        .send(WorkerEvent::Progress {
            scanned: total,
            total: Some(total),
        })
        .map_err(|_| "interactive refresh was cancelled".to_string())?;
    let mut batch = refresh_batch(&scan, &files, &dbs)?;
    batch.app_metadata_refreshed = app_ok;
    batch.app_metadata_diagnostic = app_metadata_diagnostic(&scan);
    batch.preserve_profile_evidence = !reproject && !app_ok;
    apply_refresh(&mut index, batch, reproject)?;
    let page = index
        .browse_ordered(&filter, None, page_size, order)
        .map_err(|e| e.to_string())?;
    sender
        .send(WorkerEvent::Ready(page))
        .map_err(|_| "interactive refresh was cancelled".to_string())
}

fn changed_sources(
    index: &ProfileIndex,
    files: &[PathBuf],
    dbs: &[PathBuf],
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    fn changed(index: &ProfileIndex, paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
        let mut result = Vec::new();
        for path in paths {
            let candidate = source_record(path, is_archived(path))?;
            if !matches!(
                index.source_change(&candidate).map_err(|e| e.to_string())?,
                codex_spawns::index::SourceChange::Unchanged
                    | codex_spawns::index::SourceChange::Moved { .. }
            ) {
                result.push(path.clone());
            }
        }
        Ok(result)
    }
    Ok((changed(index, files)?, changed(index, dbs)?))
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
    let semantics = p.semantics;
    Page {
        items: p
            .conversations
            .into_iter()
            .map(|c| {
                let semantic = semantics.get(&c.id).copied().unwrap_or((
                    if c.archived {
                        ConversationState::Archived
                    } else {
                        ConversationState::Active
                    },
                    if c.profile_complete {
                        ProfileQuality::Complete
                    } else {
                        ProfileQuality::Partial
                    },
                ));
                codex_spawns::interactive::ConversationItem {
                    id: c.id,
                    title: c.title,
                    title_source: c.title_source,
                    cwd: c.cwd,
                    last_activity_at: c.last_activity_at,
                    archived: c.archived,
                    agent_count: c.agent_count as usize,
                    max_depth: c.max_depth,
                    profile_complete: c.profile_complete,
                    state: conversation_state_label(semantic.0).into(),
                    profile: profile_quality_label(semantic.1).into(),
                    project: project_display(&c.project),
                    tokens: token_display(
                        &c.tokens.usage,
                        c.tokens.covered_sessions,
                        c.tokens.total_sessions,
                    ),
                    model: c.model,
                }
            })
            .collect(),
        next_cursor: p.next_cursor.map(|c| c.encode()),
        approximate_total: usize::try_from(p.approximate_total).ok(),
    }
}

fn project_display(
    project: &ProfileFact<ProjectAssignment>,
) -> codex_spawns::interactive::ProjectDisplay {
    match project.value.as_ref() {
        Some(ProjectAssignment::Assigned { id, name }) => {
            codex_spawns::interactive::ProjectDisplay::Assigned {
                id: id.clone(),
                name: name.clone(),
            }
        }
        Some(ProjectAssignment::Projectless) => {
            codex_spawns::interactive::ProjectDisplay::NoProject
        }
        None => codex_spawns::interactive::ProjectDisplay::Unknown,
    }
}

fn token_display(
    tokens: &ProfileFact<codex_spawns::TokenUsage>,
    covered: usize,
    total: usize,
) -> codex_spawns::interactive::TokenDisplay {
    match tokens.value.as_ref() {
        Some(usage) if total > 0 && covered < total => {
            codex_spawns::interactive::TokenDisplay::LowerBound(usage.total_tokens)
        }
        Some(usage) => codex_spawns::interactive::TokenDisplay::Exact(usage.total_tokens),
        None => codex_spawns::interactive::TokenDisplay::Unknown,
    }
}

fn conversation_state_label(state: ConversationState) -> &'static str {
    match state {
        ConversationState::Active => "active",
        ConversationState::Archived => "archived",
        ConversationState::Missing => "missing",
    }
}

fn profile_quality_label(quality: ProfileQuality) -> &'static str {
    match quality {
        ProfileQuality::Complete => "complete",
        ProfileQuality::Partial => "partial",
        ProfileQuality::Conflicting => "conflict",
        ProfileQuality::Updating => "updating",
        ProfileQuality::Error => "error",
    }
}
#[derive(Default)]
struct ClickTracker {
    last: Option<(Instant, u16, u16, crossterm::event::MouseButton)>,
}

fn map_event(e: TerminalEvent, app: &App, clicks: &mut ClickTracker) -> Option<Event> {
    map_event_at(e, app, clicks, Instant::now())
}

fn map_event_at(
    e: TerminalEvent,
    app: &App,
    clicks: &mut ClickTracker,
    now: Instant,
) -> Option<Event> {
    match e {
        TerminalEvent::Resize(width, height) => Some(Event::Resize { width, height }),
        TerminalEvent::Key(k) if k.kind == KeyEventKind::Press => match k.code {
            KeyCode::Up => Some(Event::Up),
            KeyCode::Down => Some(Event::Down),
            KeyCode::PageUp => Some(Event::PageUp),
            KeyCode::PageDown => Some(Event::PageDown),
            KeyCode::Home => Some(Event::Home),
            KeyCode::End => Some(Event::End),
            KeyCode::Left if k.modifiers.contains(KeyModifiers::SHIFT) => {
                Some(Event::ScrollLeftPage)
            }
            KeyCode::Right if k.modifiers.contains(KeyModifiers::SHIFT) => {
                Some(Event::ScrollRightPage)
            }
            KeyCode::Left => Some(Event::ScrollLeft),
            KeyCode::Right => Some(Event::ScrollRight),
            KeyCode::Enter => Some(Event::Enter),
            KeyCode::Esc => Some(Event::Back),
            KeyCode::Backspace if app.search_editing() => Some(Event::Key('\u{8}')),
            KeyCode::Backspace => Some(Event::Back),
            KeyCode::Tab if k.modifiers.contains(KeyModifiers::SHIFT) => Some(Event::BackTab),
            KeyCode::Tab => Some(Event::Tab),
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Event::PageUp)
            }
            KeyCode::Char('d') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Event::PageDown)
            }
            KeyCode::Char(c) => Some(Event::Key(c)),
            _ => None,
        },
        TerminalEvent::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => Some(Event::Up),
            MouseEventKind::ScrollDown => Some(Event::Down),
            MouseEventKind::Down(button) => {
                if button != crossterm::event::MouseButton::Left {
                    return None;
                }
                let event = mouse_event(app, mouse.column, mouse.row)?;
                let double = clicks.last.is_some_and(|(at, column, row, previous)| {
                    previous == button
                        && column == mouse.column
                        && row == mouse.row
                        && now.saturating_duration_since(at) <= Duration::from_millis(500)
                });
                if double {
                    clicks.last = None;
                    match event {
                        Event::MouseSelect { index } => Some(Event::MouseDoubleClick { index }),
                        other => Some(other),
                    }
                } else {
                    clicks.last = Some((now, mouse.column, mouse.row, button));
                    Some(event)
                }
            }
            _ => None,
        },
        _ => None,
    }
}

fn mouse_event(app: &App, column: u16, row: u16) -> Option<Event> {
    if app.screen() == codex_spawns::interactive::Screen::Conversations && row == 4 {
        return conversation_header_sort(
            column,
            app.conversation_viewport().column,
            codex_spawns::interactive::table_title_width(
                app.root_title_width(),
                app.conversation_viewport().width,
            ),
        )
        .map(Event::SelectSort);
    }
    if app.screen() == codex_spawns::interactive::Screen::Conversations && row >= 5 {
        return Some(Event::MouseSelect {
            index: app.conversation_viewport().row + row.saturating_sub(5) as usize,
        });
    }
    if app.screen() == codex_spawns::interactive::Screen::Conversation && row >= 5 {
        return Some(Event::MouseSelect {
            index: app.tree_viewport().row + row.saturating_sub(5) as usize,
        });
    }
    None
}

fn conversation_header_sort(
    column: u16,
    horizontal_offset: usize,
    title_width: usize,
) -> Option<codex_spawns::interactive::Sort> {
    use codex_spawns::interactive::{root_columns, ColumnKey, Sort};
    let column = column as usize;
    let columns = root_columns(title_width);
    let title = columns.first()?;
    if (3..3 + title.width).contains(&column) {
        return Some(Sort::Title);
    }
    let logical = column
        .saturating_sub(3 + title.width + 2)
        .saturating_add(horizontal_offset);
    let mut start = 0;
    for descriptor in columns.iter().filter(|descriptor| !descriptor.frozen) {
        if (start..start + descriptor.width).contains(&logical) {
            return match descriptor.key {
                ColumnKey::Project => Some(Sort::Project),
                ColumnKey::Tokens => Some(Sort::Tokens),
                ColumnKey::Updated => Some(Sort::Updated),
                ColumnKey::State => Some(Sort::State),
                ColumnKey::Profile => Some(Sort::Profile),
                ColumnKey::Agents => Some(Sort::Agents),
                ColumnKey::Depth => Some(Sort::Depth),
                _ => None,
            };
        }
        start += descriptor.width + 3;
    }
    None
}
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}
impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(out))?,
        })
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

pub fn run_index(action: IndexAction, common: &Common) -> Result<(), String> {
    let path = selected_index_path(common);
    cleanup_ephemeral_index(common, &path);
    match action {
        IndexAction::Status => {
            let index = ProfileIndex::open(IndexOptions { path: path.clone() })
                .map_err(|e| e.to_string())?;
            let stats = index.stats().map_err(|e| e.to_string())?;
            let projection = index.projection_status().map_err(|e| e.to_string())?;
            println!(
                "index: {}\nstatus: ready\napp_metadata: {}\nconversations: {}\nagents: {}\nsources: {}\nmissing_sources: {}\nprojection_version: {}\nrequired_projection_version: {}\nneeds_reprojection: {}",
                path.display(),
                index.app_metadata_status().map_err(|e| e.to_string())?,
                stats.conversations, stats.agents, stats.sources, stats.missing_sources,
                projection.current, projection.required, projection.needs_reprojection()
            );
        }
        IndexAction::Refresh | IndexAction::Rebuild => {
            let rebuild = matches!(action, IndexAction::Rebuild);
            let (files, dbs) = crate::cli::discover(common)?;
            let mut index = ProfileIndex::open(IndexOptions { path: path.clone() })
                .map_err(|e| e.to_string())?;
            if rebuild {
                index.reset().map_err(|e| e.to_string())?;
            }
            let reproject = index.needs_reprojection().map_err(|e| e.to_string())?;
            let (changed_files, changed_dbs) = if reproject {
                (files.clone(), dbs.clone())
            } else {
                changed_sources(&index, &files, &dbs)?
            };
            let paths = crate::cli::app_metadata_paths(common);
            let app_candidate = paths.thread_catalog.exists() && paths.global_state.exists();
            let (scan, app_ok) = crate::cli::scan_with_optional_app(
                common,
                if app_candidate {
                    &files
                } else {
                    &changed_files
                },
                if app_candidate { &dbs } else { &changed_dbs },
            )?;
            let mut batch = refresh_batch(&scan, &files, &dbs)?;
            batch.app_metadata_refreshed = app_ok;
            batch.app_metadata_diagnostic = app_metadata_diagnostic(&scan);
            batch.preserve_profile_evidence = !reproject && !app_ok;
            apply_refresh(&mut index, batch, reproject)?;
            let stats = index.stats().map_err(|e| e.to_string())?;
            println!(
                "indexed: {} conversations, {} agents, {} sources",
                stats.conversations, stats.agents, stats.sources
            );
        }
        IndexAction::Prune { before } => {
            let mut index = ProfileIndex::open(IndexOptions { path: path.clone() })
                .map_err(|e| e.to_string())?;
            println!(
                "pruned: {}",
                index.prune_missing(before).map_err(|e| e.to_string())?
            );
        }
    }
    cleanup_ephemeral_index(common, &path);
    Ok(())
}

fn app_ok_candidate(common: &Common) -> bool {
    let paths = crate::cli::app_metadata_paths(common);
    paths.thread_catalog.exists() && paths.global_state.exists()
}

fn app_metadata_diagnostic(scan: &ScanResult) -> Option<String> {
    scan.diagnostics
        .iter()
        .find(|value| {
            value.starts_with("App metadata unavailable:")
                || value.starts_with("App metadata ready after fallback:")
        })
        .cloned()
}

fn apply_refresh(
    index: &mut ProfileIndex,
    batch: RefreshBatch,
    reproject: bool,
) -> Result<(), String> {
    if reproject {
        let semantics = projection_semantics(&batch);
        index
            .complete_reprojection(
                ProfileIndex::required_projection_version(),
                batch,
                &semantics,
                |_| {},
            )
            .map_err(|e| e.to_string())
    } else {
        index.refresh(batch, |_| {}).map_err(|e| e.to_string())
    }
}

fn projection_semantics(batch: &RefreshBatch) -> Vec<codex_spawns::index::ConversationSemantics> {
    batch
        .conversations
        .iter()
        .map(|conversation| codex_spawns::index::ConversationSemantics {
            id: conversation.id.clone(),
            state: if conversation.archived {
                ConversationState::Archived
            } else {
                ConversationState::Active
            },
            profile: conversation.profile_quality(),
        })
        .collect()
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
            title: conversation_title(root, &cwd, &created),
            title_source: if scan.app_titles.contains_key(&root.id) {
                "app"
            } else if root.title.value.is_some() {
                "official"
            } else if root.first_user_message.value.is_some() {
                "user message"
            } else if Path::new(&cwd)
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| !value.is_empty())
            {
                "cwd/time"
            } else {
                "id"
            }
            .into(),
            cwd,
            created_at: created.clone(),
            last_activity_at: root.last_event_at.value.clone().unwrap_or(created),
            archived: is_archived(&root.path),
            model: root.model.value.clone(),
            status: None,
            agent_count: related,
            max_depth,
            profile_complete: root.parse_errors == 0
                && scan.projects.get(&root.id).is_some_and(|fact| {
                    fact.value.is_some() && fact.confidence != FactConfidence::Conflicting
                })
                && scan
                    .conversation_tokens
                    .get(&root.id)
                    .is_some_and(|summary| {
                        summary.usage.value.is_some()
                            && summary.covered_sessions == summary.total_sessions
                            && summary.usage.confidence != FactConfidence::Conflicting
                    }),
            project: scan
                .projects
                .get(&root.id)
                .cloned()
                .unwrap_or_else(ProfileFact::unknown),
            tokens: scan
                .conversation_tokens
                .get(&root.id)
                .cloned()
                .unwrap_or_default(),
        });
        agents.push(AgentRecord {
            id: root.id.clone(),
            root_id: root.id.clone(),
            parent_id: None,
            agent_path: None,
            task_name: Some("root conversation".into()),
            task_excerpt: None,
            title: conversation_title(
                root,
                &root.cwd.value.clone().unwrap_or_default(),
                &root.created_at.value.clone().unwrap_or_default(),
            ),
            role: Some("root".into()),
            nickname: None,
            model: root.model.value.clone(),
            effort: root.effort.value.clone(),
            status: "complete".into(),
            depth: 0,
            evidence_complete: root.parse_errors == 0,
            tokens: scan
                .session_tokens
                .get(&root.id)
                .cloned()
                .unwrap_or_else(ProfileFact::unknown),
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
            project: ProfileFact::unknown(),
            tokens: TokenUsageSummary::default(),
        });
    }
    let session_by_id: HashMap<_, _> = scan.agent_sessions.iter().map(|a| (&a.id, a)).collect();
    let mut fulfilled_sessions = HashSet::new();
    for attempt in &scan.spawn_attempts {
        // A fulfilled attempt adopts its session ID so the two evidence streams
        // merge into one row. Every additional attempt remains independently
        // addressable by its stable attempt ID.
        let id = attempt.child_thread_id.as_ref().map_or_else(
            || attempt.id.clone(),
            |child| {
                if fulfilled_sessions.insert(child.clone()) {
                    child.clone()
                } else {
                    attempt.id.clone()
                }
            },
        );
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
            id: id.clone(),
            root_id,
            parent_id: attempt.parent_thread_id.clone(),
            agent_path: attempt.agent_path.value.clone(),
            task_name: attempt.task_name.value.clone(),
            task_excerpt: excerpt(attempt.message.value.as_deref()),
            title: attempt
                .message
                .value
                .as_deref()
                .and_then(codex_spawns::project_plain_text)
                .or_else(|| attempt.task_name.value.clone())
                .unwrap_or_else(|| "unnamed agent".into()),
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
            tokens: scan
                .session_tokens
                .get(&id)
                .cloned()
                .unwrap_or_else(ProfileFact::unknown),
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
        app_metadata_refreshed: false,
        app_metadata_diagnostic: None,
        preserve_profile_evidence: false,
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
fn conversation_title(root: &codex_spawns::RootConversation, cwd: &str, created: &str) -> String {
    root.title
        .value
        .clone()
        .or_else(|| {
            root.first_user_message.value.as_deref().map(|message| {
                let one_line = message.split_whitespace().collect::<Vec<_>>().join(" ");
                if one_line.chars().count() > 80 {
                    format!("{}…", one_line.chars().take(79).collect::<String>())
                } else {
                    one_line
                }
            })
        })
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| fallback_title(cwd, created, &root.id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use codex_spawns::{FactConfidence, ProfileFact};

    fn root() -> codex_spawns::RootConversation {
        codex_spawns::RootConversation {
            id: "root-id-long".into(),
            path: "/tmp/root.jsonl".into(),
            created_at: ProfileFact::unknown(),
            cwd: ProfileFact::unknown(),
            model: ProfileFact::unknown(),
            effort: ProfileFact::unknown(),
            title: ProfileFact::unknown(),
            first_user_message: ProfileFact::unknown(),
            last_event_at: ProfileFact::unknown(),
            event_count: 0,
            parse_errors: 0,
        }
    }

    #[test]
    fn conversation_title_uses_the_confirmed_priority_order() {
        let mut item = root();
        assert_eq!(
            conversation_title(&item, "/work/project", "2026-01-01"),
            "project · 2026-01-01"
        );
        item.first_user_message.value = Some("  Explain   this conversation  ".into());
        item.first_user_message.confidence = FactConfidence::Observed;
        assert_eq!(
            conversation_title(&item, "/work/project", "2026-01-01"),
            "Explain this conversation"
        );
        item.title.value = Some("Official".into());
        item.title.confidence = FactConfidence::Observed;
        assert_eq!(
            conversation_title(&item, "/work/project", "2026-01-01"),
            "Official"
        );
    }

    #[test]
    fn explicit_index_path_wins_over_codex_home() {
        let common = Common {
            index_path: Some("/tmp/custom.sqlite".into()),
            codex_home: Some("/other".into()),
            ..Default::default()
        };
        assert_eq!(index_path(&common), PathBuf::from("/tmp/custom.sqlite"));
    }

    #[test]
    fn preferences_are_loaded_and_saved_beside_the_profile_index() {
        let dir = tempfile::tempdir().unwrap();
        let common = Common {
            index_path: Some(dir.path().join("index.sqlite")),
            ..Default::default()
        };
        let preferences = Preferences {
            title_width: 72,
            page_size: 40,
            ..Preferences::default()
        };
        save_preferences(&common, &preferences).unwrap();
        let loaded = load_preferences(&common);
        assert_eq!(loaded.title_width, 72);
        assert_eq!(loaded.page_size, 40);
        assert!(!loaded.to_toml_like().contains("conversation"));
    }

    #[test]
    fn browse_total_is_forwarded_to_the_interactive_footer_model() {
        let page = to_page(codex_spawns::index::BrowsePage {
            conversations: vec![],
            next_cursor: None,
            semantics: Default::default(),
            approximate_total: 588,
        });
        assert_eq!(page.approximate_total, Some(588));
    }

    #[test]
    fn refresh_skips_unchanged_sources_and_revisits_appends() {
        let dir = tempfile::tempdir().unwrap();
        let rollout = dir.path().join("rollout.jsonl");
        fs::write(&rollout, "one\n").unwrap();
        let mut index = ProfileIndex::open(IndexOptions {
            path: dir.path().join("index.sqlite"),
        })
        .unwrap();
        let source = source_record(&rollout, false).unwrap();
        index
            .refresh(
                RefreshBatch {
                    sources: vec![source],
                    ..Default::default()
                },
                |_| {},
            )
            .unwrap();
        assert!(changed_sources(&index, std::slice::from_ref(&rollout), &[])
            .unwrap()
            .0
            .is_empty());
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(&rollout)
            .unwrap()
            .write_all(b"two\n")
            .unwrap();
        assert_eq!(
            changed_sources(&index, std::slice::from_ref(&rollout), &[])
                .unwrap()
                .0,
            vec![rollout]
        );
    }

    #[test]
    fn stale_projection_is_completed_with_semantic_rows_in_one_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = ProfileIndex::open(IndexOptions {
            path: dir.path().join("index.sqlite"),
        })
        .unwrap();
        assert!(index.needs_reprojection().unwrap());
        let conversation = ConversationRecord {
            id: "root".into(),
            title: "Readable title".into(),
            title_source: "user message".into(),
            cwd: "/work".into(),
            created_at: "2026-08-13T01:00:00Z".into(),
            last_activity_at: "2026-08-13T02:00:00Z".into(),
            archived: true,
            model: None,
            status: None,
            agent_count: 0,
            max_depth: 0,
            profile_complete: false,
            project: ProfileFact::unknown(),
            tokens: TokenUsageSummary::default(),
        };
        apply_refresh(
            &mut index,
            RefreshBatch {
                conversations: vec![conversation],
                discovered_all_sources: true,
                ..Default::default()
            },
            true,
        )
        .unwrap();
        assert!(!index.needs_reprojection().unwrap());
        let page = index
            .browse(&ConversationFilter::default(), None, 25)
            .unwrap();
        assert_eq!(
            page.semantics.get("root"),
            Some(&(ConversationState::Archived, ProfileQuality::Partial))
        );
    }

    #[test]
    fn terminal_mouse_scroll_maps_to_keyboard_equivalent_navigation() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent};
        let mut app = App::new(Preferences::default());
        let mut clicks = ClickTracker::default();
        app.update(Event::SetViewport {
            width: 80,
            height: 3,
        });
        assert_eq!(
            map_event(
                TerminalEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 0,
                    row: 0,
                    modifiers: KeyModifiers::NONE
                }),
                &app,
                &mut clicks,
            ),
            Some(Event::Down)
        );
        assert_eq!(
            map_event(
                TerminalEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 4,
                    row: 6,
                    modifiers: KeyModifiers::NONE
                }),
                &app,
                &mut clicks,
            ),
            Some(Event::MouseSelect { index: 1 })
        );
    }

    #[test]
    fn terminal_keys_cover_page_home_end_back_and_horizontal_navigation() {
        use crossterm::event::KeyEvent;
        let app = App::new(Preferences::default());
        let mut clicks = ClickTracker::default();
        let mut key = |code, modifiers| {
            map_event(
                TerminalEvent::Key(KeyEvent::new(code, modifiers)),
                &app,
                &mut clicks,
            )
        };
        assert_eq!(
            key(KeyCode::PageUp, KeyModifiers::NONE),
            Some(Event::PageUp)
        );
        assert_eq!(
            key(KeyCode::PageDown, KeyModifiers::NONE),
            Some(Event::PageDown)
        );
        assert_eq!(key(KeyCode::Home, KeyModifiers::NONE), Some(Event::Home));
        assert_eq!(key(KeyCode::End, KeyModifiers::NONE), Some(Event::End));
        assert_eq!(
            key(KeyCode::Backspace, KeyModifiers::NONE),
            Some(Event::Back)
        );
        assert_eq!(
            key(KeyCode::Left, KeyModifiers::NONE),
            Some(Event::ScrollLeft)
        );
        assert_eq!(
            key(KeyCode::Right, KeyModifiers::SHIFT),
            Some(Event::ScrollRightPage)
        );
        assert_eq!(
            key(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Some(Event::PageUp)
        );
        assert_eq!(
            key(KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(Event::PageDown)
        );
    }

    #[test]
    fn backspace_edits_search_before_it_becomes_navigation() {
        use crossterm::event::KeyEvent;
        let mut app = App::new(Preferences::default());
        app.update(Event::Key('/'));
        app.update(Event::Key('x'));
        let mut clicks = ClickTracker::default();
        assert_eq!(
            map_event(
                TerminalEvent::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
                &app,
                &mut clicks,
            ),
            Some(Event::Key('\u{8}'))
        );
    }

    #[test]
    fn conversation_mouse_hits_headers_and_viewport_relative_rows() {
        let mut app = App::new(Preferences::default());
        app.update(Event::SetViewport {
            width: 80,
            height: 2,
        });
        assert_eq!(
            mouse_event(&app, 3, 4),
            Some(Event::SelectSort(codex_spawns::interactive::Sort::Title))
        );
        use codex_spawns::interactive::{root_columns, ColumnKey, Sort};
        let expected = [
            (ColumnKey::Project, Sort::Project),
            (ColumnKey::Tokens, Sort::Tokens),
            (ColumnKey::Updated, Sort::Updated),
            (ColumnKey::State, Sort::State),
            (ColumnKey::Profile, Sort::Profile),
            (ColumnKey::Agents, Sort::Agents),
            (ColumnKey::Depth, Sort::Depth),
        ];
        let title_width = codex_spawns::interactive::table_title_width(
            app.root_title_width(),
            app.conversation_viewport().width,
        );
        let mut x = 3 + title_width + 2;
        for descriptor in root_columns(title_width)
            .into_iter()
            .filter(|column| !column.frozen)
        {
            if let Some((_, sort)) = expected.iter().find(|(key, _)| *key == descriptor.key) {
                assert_eq!(
                    mouse_event(&app, x as u16, 4),
                    Some(Event::SelectSort(*sort)),
                    "header {}",
                    descriptor.header
                );
            }
            x += descriptor.width + 3;
        }
        assert_eq!(
            mouse_event(&app, 3, 6),
            Some(Event::MouseSelect { index: 1 })
        );
        app.update(Event::SetViewport {
            width: 30,
            height: 2,
        });
        let narrow_title = codex_spawns::interactive::table_title_width(app.root_title_width(), 30);
        assert_eq!(
            mouse_event(&app, (3 + narrow_title + 2) as u16, 4),
            Some(Event::SelectSort(Sort::Project))
        );
    }

    #[test]
    fn duplicate_spawn_attempts_remain_rows_while_one_merges_with_the_session() {
        let fixture = |name: &str| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name)
        };
        let parent = fixture("parent.jsonl");
        let child = fixture("child.jsonl");
        let mut scan = codex_spawns::scan_sources(&[parent.clone(), child.clone()], &[]).unwrap();
        let mut retry = scan.spawn_attempts[0].clone();
        retry.id = "retry-attempt".into();
        retry.call_id = Some("call-retry".into());
        scan.spawn_attempts.push(retry);
        let batch = refresh_batch(&scan, &[parent, child], &[]).unwrap();
        let child_id = "01900000-0000-7000-8000-000000000002";
        assert_eq!(
            batch
                .agents
                .iter()
                .filter(|agent| agent.id == child_id)
                .count(),
            1
        );
        assert!(batch.agents.iter().any(|agent| agent.id == "retry-attempt"));
        assert_eq!(batch.agents.len(), 3);
    }

    #[test]
    fn tui_project_filters_map_to_stable_index_project_filters() {
        use codex_spawns::{
            index::ProjectFilter as IndexProjectFilter, interactive::ProjectFilter,
        };
        let cases = [
            (
                ProjectFilter::Assigned("project-id".into()),
                Some(IndexProjectFilter::Assigned("project-id".into())),
            ),
            (
                ProjectFilter::NoProject,
                Some(IndexProjectFilter::Projectless),
            ),
            (ProjectFilter::Unknown, Some(IndexProjectFilter::Unknown)),
            (ProjectFilter::All, None),
        ];
        for (project, expected) in cases {
            assert_eq!(
                browse_filter(
                    codex_spawns::interactive::Filter::All,
                    &project,
                    String::new()
                )
                .project,
                expected
            );
        }
    }

    #[test]
    fn agent_and_conversation_details_keep_both_conflicting_token_totals_visible() {
        let mut agent = AgentRecord {
            id: "root".into(),
            root_id: "root".into(),
            parent_id: None,
            agent_path: None,
            task_name: None,
            task_excerpt: None,
            title: "Root".into(),
            role: None,
            nickname: None,
            model: None,
            effort: None,
            status: "complete".into(),
            depth: 0,
            evidence_complete: true,
            tokens: ProfileFact::observed(
                codex_spawns::TokenUsage {
                    total_tokens: 120,
                    ..Default::default()
                },
                codex_spawns::SourceRef::Derived {
                    rule: "rollout".into(),
                },
            ),
        };
        agent.tokens.confidence = FactConfidence::Conflicting;
        agent
            .tokens
            .conflicting_values
            .push(codex_spawns::TokenUsage {
                total_tokens: 100,
                ..Default::default()
            });
        let summary = TokenUsageSummary {
            usage: agent.tokens.clone(),
            covered_sessions: 1,
            total_sessions: 2,
        };
        let detail = summary_detail(agent, Some(&summary));
        assert!(detail
            .lines
            .contains(&("total tokens".into(), "120".into())));
        assert!(detail
            .lines
            .contains(&("conflicting total tokens".into(), "100".into())));
        assert!(detail
            .lines
            .contains(&("conversation total tokens".into(), "120".into())));
        assert!(detail
            .lines
            .contains(&("conversation conflicting total tokens".into(), "100".into())));
    }

    #[test]
    fn agent_mouse_rows_are_viewport_relative_and_bounded_double_click_opens() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent};
        let mut app = App::new(Preferences::default());
        app.update(Event::ConversationsLoaded(Page {
            items: vec![codex_spawns::interactive::ConversationItem {
                id: "root".into(),
                title: "Root".into(),
                cwd: String::new(),
                last_activity_at: String::new(),
                archived: false,
                agent_count: 2,
                max_depth: 1,
                profile_complete: true,
                title_source: "fixture".into(),
                state: "active".into(),
                profile: "complete".into(),
                project: Default::default(),
                tokens: Default::default(),
                model: None,
            }],
            next_cursor: None,
            approximate_total: Some(1),
        }));
        app.update(Event::Enter);
        app.update(Event::AgentsLoaded {
            conversation_id: "root".into(),
            agents: vec![
                codex_spawns::interactive::AgentItem {
                    id: "root".into(),
                    parent_id: None,
                    depth: 0,
                    status: codex_spawns::interactive::AgentStatus::Complete,
                    task_name: "root conversation".into(),
                    title: "Root".into(),
                    role: None,
                    nickname: None,
                    model: None,
                    effort: None,
                    detail_loaded: false,
                    tokens: Default::default(),
                },
                codex_spawns::interactive::AgentItem {
                    id: "child".into(),
                    parent_id: Some("root".into()),
                    depth: 1,
                    status: codex_spawns::interactive::AgentStatus::Complete,
                    task_name: "child".into(),
                    title: "Child".into(),
                    role: None,
                    nickname: None,
                    model: None,
                    effort: None,
                    detail_loaded: false,
                    tokens: Default::default(),
                },
            ],
        });
        app.update(Event::SetViewport {
            width: 80,
            height: 1,
        });
        app.update(Event::Down);
        assert_eq!(app.tree_viewport().row, 1);
        let click = |row| {
            TerminalEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        let start = Instant::now();
        let mut tracker = ClickTracker::default();
        assert_eq!(
            map_event_at(click(5), &app, &mut tracker, start),
            Some(Event::MouseSelect { index: 1 })
        );
        assert_eq!(
            map_event_at(
                click(5),
                &app,
                &mut tracker,
                start + Duration::from_millis(300)
            ),
            Some(Event::MouseDoubleClick { index: 1 })
        );
        assert_eq!(
            map_event_at(click(5), &app, &mut tracker, start + Duration::from_secs(2)),
            Some(Event::MouseSelect { index: 1 })
        );
        assert_eq!(
            map_event_at(
                click(6),
                &app,
                &mut tracker,
                start + Duration::from_millis(2100)
            ),
            Some(Event::MouseSelect { index: 2 })
        );
    }
}
