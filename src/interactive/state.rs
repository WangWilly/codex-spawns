use std::cmp::min;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationItem {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub last_activity_at: String,
    pub archived: bool,
    pub agent_count: usize,
    pub max_depth: u32,
    pub profile_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Requested,
    Spawned,
    Complete,
    Failed,
    StateOnly,
    Orphan,
}

impl AgentStatus {
    pub fn cue(self) -> &'static str {
        match self {
            Self::Requested => "[requested]",
            Self::Spawned => "[spawned]",
            Self::Complete => "[complete]",
            Self::Failed => "[failed]",
            Self::StateOnly => "[state-only]",
            Self::Orphan => "[orphan]",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentItem {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: u32,
    pub status: AgentStatus,
    pub task_name: String,
    pub role: Option<String>,
    pub nickname: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub detail_loaded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentDetail {
    pub agent_id: String,
    pub lines: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub approximate_total: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    ActiveOnly,
    ArchivedOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Conversations,
    Conversation,
    AgentDetail,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Detail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Recent,
    Oldest,
    Title,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub page_size: usize,
    pub filter: Filter,
    pub sort: Sort,
    pub pane_width_percent: u16,
    pub color: bool,
    pub sensitive_content_acknowledged: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            page_size: 25,
            filter: Filter::All,
            sort: Sort::Recent,
            pane_width_percent: 38,
            color: std::env::var_os("NO_COLOR").is_none(),
            sensitive_content_acknowledged: false,
        }
    }
}

impl Preferences {
    /// Minimal persistence format deliberately excludes queries and selections.
    pub fn to_toml_like(&self) -> String {
        format!("page_size = {}\nfilter = \"{:?}\"\nsort = \"{:?}\"\npane_width_percent = {}\ncolor = {}\nsensitive_content_acknowledged = {}\n",
            self.page_size, self.filter, self.sort, self.pane_width_percent, self.color,
            self.sensitive_content_acknowledged)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefreshProgress {
    pub scanned: usize,
    pub total: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Key(char),
    Up,
    Down,
    Enter,
    Back,
    Tab,
    BackTab,
    ClearSearch,
    MouseSelect {
        index: usize,
    },
    Resize {
        width: u16,
        height: u16,
    },
    ConversationsLoaded(Page<ConversationItem>),
    MoreConversationsLoaded(Page<ConversationItem>),
    AgentsLoaded {
        conversation_id: String,
        agents: Vec<AgentItem>,
    },
    AgentDetailLoaded(AgentDetail),
    RefreshProgress(RefreshProgress),
    RefreshReady(Page<ConversationItem>),
    ApplySnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Quit,
    LoadMore { cursor: String },
    LoadAgents { conversation_id: String },
    LoadAgentDetail { agent_id: String },
    Refresh,
    Rebuild,
    OpenEvidence { agent_id: String },
    OpenMessage { agent_id: String },
}

#[derive(Debug)]
pub struct App {
    preferences: Preferences,
    conversations: Vec<ConversationItem>,
    conversation_selection: usize,
    next_cursor: Option<String>,
    loading_more: bool,
    approximate_total: Option<usize>,
    selected_root_id: Option<String>,
    agents: Vec<AgentItem>,
    agent_selection: usize,
    details: Vec<AgentDetail>,
    screen: Screen,
    previous_screen: Screen,
    focus: Focus,
    width: u16,
    search_editing: bool,
    search: String,
    refresh_progress: Option<RefreshProgress>,
    pending_snapshot: Option<Page<ConversationItem>>,
    rebuild_confirmation: bool,
}

impl App {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            preferences,
            conversations: vec![],
            conversation_selection: 0,
            next_cursor: None,
            loading_more: false,
            approximate_total: None,
            selected_root_id: None,
            agents: vec![],
            agent_selection: 0,
            details: vec![],
            screen: Screen::Conversations,
            previous_screen: Screen::Conversations,
            focus: Focus::Tree,
            width: 100,
            search_editing: false,
            search: String::new(),
            refresh_progress: None,
            pending_snapshot: None,
            rebuild_confirmation: false,
        }
    }

    pub fn update(&mut self, event: Event) -> Vec<Command> {
        match event {
            Event::ConversationsLoaded(page) => self.replace_page(page),
            Event::MoreConversationsLoaded(page) => {
                self.conversations.extend(page.items);
                self.next_cursor = page.next_cursor;
                self.approximate_total = page.approximate_total;
                self.loading_more = false;
            }
            Event::AgentsLoaded {
                conversation_id,
                agents,
            } if self.selected_root_id.as_deref() == Some(&conversation_id) => {
                self.agents = agents;
                self.agent_selection = 0;
            }
            Event::AgentsLoaded { .. } => {}
            Event::AgentDetailLoaded(detail) => {
                if let Some(a) = self.agents.iter_mut().find(|a| a.id == detail.agent_id) {
                    a.detail_loaded = true;
                }
                self.details.retain(|d| d.agent_id != detail.agent_id);
                self.details.push(detail);
                self.screen = Screen::AgentDetail;
            }
            Event::Resize { width, .. } => self.width = width,
            Event::RefreshProgress(progress) => self.refresh_progress = Some(progress),
            Event::RefreshReady(page) => {
                self.pending_snapshot = Some(page);
                self.refresh_progress = None;
            }
            Event::ApplySnapshot => {
                if let Some(page) = self.pending_snapshot.take() {
                    self.replace_page(page);
                }
            }
            Event::ClearSearch => {
                self.search.clear();
                self.search_editing = false;
                self.clamp_selection();
            }
            Event::MouseSelect { index } => match self.screen {
                Screen::Conversations => {
                    self.conversation_selection =
                        min(index, self.visible_conversations().len().saturating_sub(1))
                }
                Screen::Conversation | Screen::AgentDetail => {
                    self.agent_selection = min(index, self.agents.len().saturating_sub(1))
                }
                Screen::Help => {}
            },
            Event::Down => return self.move_down(),
            Event::Up => {
                self.move_up();
            }
            Event::Tab if !self.is_narrow() => self.focus = Focus::Detail,
            Event::BackTab if !self.is_narrow() => self.focus = Focus::Tree,
            Event::Enter => return self.enter(),
            Event::Back => self.back(),
            Event::Key(key) => return self.key(key),
            Event::Tab | Event::BackTab => {}
        }
        vec![]
    }

    fn key(&mut self, key: char) -> Vec<Command> {
        if self.search_editing {
            match key {
                '\u{8}' => {
                    self.search.pop();
                }
                _ => self.search.push(key),
            }
            self.clamp_selection();
            return vec![];
        }
        match key {
            'q' => vec![Command::Quit],
            '/' => {
                self.search_editing = true;
                vec![]
            }
            'f' if self.screen == Screen::Conversations => {
                self.preferences.filter = match self.preferences.filter {
                    Filter::All => Filter::ActiveOnly,
                    Filter::ActiveOnly => Filter::ArchivedOnly,
                    Filter::ArchivedOnly => Filter::All,
                };
                self.clamp_selection();
                vec![]
            }
            'j' => self.move_down(),
            'k' => {
                self.move_up();
                vec![]
            }
            'r' => vec![Command::Refresh],
            'R' if self.rebuild_confirmation => {
                self.rebuild_confirmation = false;
                vec![Command::Rebuild]
            }
            'R' => {
                self.rebuild_confirmation = true;
                vec![]
            }
            '?' => {
                self.previous_screen = self.screen;
                self.screen = Screen::Help;
                vec![]
            }
            'e' => self
                .selected_agent()
                .map(|a| {
                    vec![Command::OpenEvidence {
                        agent_id: a.id.clone(),
                    }]
                })
                .unwrap_or_default(),
            'm' => self
                .selected_agent()
                .map(|a| {
                    vec![Command::OpenMessage {
                        agent_id: a.id.clone(),
                    }]
                })
                .unwrap_or_default(),
            _ => vec![],
        }
    }

    fn enter(&mut self) -> Vec<Command> {
        if self.search_editing {
            self.search_editing = false;
            return vec![];
        }
        if let Some(page) = self.pending_snapshot.take() {
            self.replace_page(page);
            return vec![];
        }
        match self.screen {
            Screen::Conversations => {
                if let Some(item) = self.selected_conversation().cloned() {
                    self.selected_root_id = Some(item.id.clone());
                    self.agents.clear();
                    self.screen = Screen::Conversation;
                    vec![Command::LoadAgents {
                        conversation_id: item.id,
                    }]
                } else {
                    vec![]
                }
            }
            Screen::Conversation => {
                if let Some(agent) = self.selected_agent().cloned() {
                    self.screen = Screen::AgentDetail;
                    if agent.detail_loaded {
                        vec![]
                    } else {
                        vec![Command::LoadAgentDetail { agent_id: agent.id }]
                    }
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }

    fn back(&mut self) {
        if self.search_editing {
            self.search_editing = false;
            return;
        }
        match self.screen {
            Screen::AgentDetail => self.screen = Screen::Conversation,
            Screen::Conversation => {
                self.screen = Screen::Conversations;
                self.selected_root_id = None;
                self.agents.clear();
            }
            Screen::Help => self.screen = self.previous_screen,
            Screen::Conversations => {}
        }
    }

    fn move_down(&mut self) -> Vec<Command> {
        match self.screen {
            Screen::Conversations => {
                let len = self.visible_conversations().len();
                if self.conversation_selection + 1 < len {
                    self.conversation_selection += 1;
                    vec![]
                } else if !self.loading_more {
                    let Some(cursor) = self.next_cursor.clone() else {
                        return vec![];
                    };
                    self.loading_more = true;
                    vec![Command::LoadMore { cursor }]
                } else {
                    vec![]
                }
            }
            Screen::Conversation | Screen::AgentDetail => {
                if self.agent_selection + 1 < self.agents.len() {
                    self.agent_selection += 1;
                }
                vec![]
            }
            Screen::Help => vec![],
        }
    }

    fn move_up(&mut self) {
        match self.screen {
            Screen::Conversations => {
                self.conversation_selection = self.conversation_selection.saturating_sub(1)
            }
            Screen::Conversation | Screen::AgentDetail => {
                self.agent_selection = self.agent_selection.saturating_sub(1)
            }
            Screen::Help => {}
        }
    }
    fn replace_page(&mut self, page: Page<ConversationItem>) {
        self.conversations = page.items;
        self.next_cursor = page.next_cursor;
        self.approximate_total = page.approximate_total;
        self.conversation_selection = 0;
    }
    fn clamp_selection(&mut self) {
        self.conversation_selection = min(
            self.conversation_selection,
            self.visible_conversations().len().saturating_sub(1),
        );
    }

    pub fn conversations(&self) -> &[ConversationItem] {
        &self.conversations
    }
    pub fn visible_conversations(&self) -> Vec<&ConversationItem> {
        let needle = self.search.to_lowercase();
        self.conversations
            .iter()
            .filter(|c| {
                let filter = match self.preferences.filter {
                    Filter::All => true,
                    Filter::ActiveOnly => !c.archived,
                    Filter::ArchivedOnly => c.archived,
                };
                filter
                    && (needle.is_empty()
                        || c.title.to_lowercase().contains(&needle)
                        || c.id.to_lowercase().contains(&needle)
                        || c.cwd.to_lowercase().contains(&needle))
            })
            .collect()
    }
    pub fn selected_conversation(&self) -> Option<&ConversationItem> {
        self.visible_conversations()
            .get(self.conversation_selection)
            .copied()
    }
    pub fn visible_agents(&self) -> &[AgentItem] {
        &self.agents
    }
    pub fn selected_agent(&self) -> Option<&AgentItem> {
        self.agents.get(self.agent_selection)
    }
    pub fn selected_detail(&self) -> Option<&AgentDetail> {
        let id = &self.selected_agent()?.id;
        self.details.iter().find(|d| &d.agent_id == id)
    }
    pub fn selected_conversation_index(&self) -> usize {
        self.conversation_selection
    }
    pub fn selected_agent_index(&self) -> usize {
        self.agent_selection
    }
    pub fn select_last(&mut self) {
        self.conversation_selection = self.visible_conversations().len().saturating_sub(1);
    }
    pub fn screen(&self) -> Screen {
        self.screen
    }
    pub fn focus(&self) -> Focus {
        self.focus
    }
    pub fn filter(&self) -> Filter {
        self.preferences.filter
    }
    pub fn preferences(&self) -> &Preferences {
        &self.preferences
    }
    pub fn is_narrow(&self) -> bool {
        self.width < 90
    }
    pub fn has_pending_snapshot(&self) -> bool {
        self.pending_snapshot.is_some()
    }
    pub fn rebuild_confirmation(&self) -> bool {
        self.rebuild_confirmation
    }
    pub fn search(&self) -> &str {
        &self.search
    }
    pub fn search_editing(&self) -> bool {
        self.search_editing
    }
    pub fn refresh_progress(&self) -> Option<RefreshProgress> {
        self.refresh_progress
    }
    pub fn approximate_total(&self) -> Option<usize> {
        self.approximate_total
    }
    pub fn breadcrumb(&self) -> String {
        let Some(id) = self.selected_root_id.as_deref() else {
            return "Conversations".into();
        };
        let title = self
            .conversations
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.title.as_str())
            .unwrap_or("Unknown");
        format!("Conversations / {title} / {id}")
    }
}
