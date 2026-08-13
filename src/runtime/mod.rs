use crate::cli::{Common, IndexAction};
use codex_spawns::{
    index::{ConversationFilter, IndexOptions, ProfileIndex},
    interactive::{self, App, Command, Event, Page, Preferences},
};
use crossterm::{
    event::{self, Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

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
    let mut terminal = TerminalGuard::enter().map_err(|e| e.to_string())?;
    loop {
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
                        _ => {}
                    }
                }
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
            let page = index
                .browse(&ConversationFilter::default(), None, 1)
                .map_err(|e| e.to_string())?;
            println!(
                "index: {}\nstatus: ready\nhas_conversations: {}",
                path.display(),
                !page.conversations.is_empty()
            );
        }
        IndexAction::Refresh | IndexAction::Rebuild => {
            return Err("index refresh requires the ingestion adapter (not yet available)".into())
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
