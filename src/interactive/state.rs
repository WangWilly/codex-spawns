use std::cmp::min;

fn compact_tokens(value: u64) -> String {
    const UNITS: [&str; 4] = ["", "K", "M", "B"];
    let mut scaled = value as f64;
    let mut unit = 0;
    while scaled >= 1000.0 && unit < UNITS.len() - 1 {
        scaled /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        return value.to_string();
    }
    let mut rounded = (scaled * 10.0).round() / 10.0;
    if rounded >= 1000.0 && unit < UNITS.len() - 1 {
        scaled = rounded / 1000.0;
        unit += 1;
        rounded = (scaled * 10.0).round() / 10.0;
    }
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}{}", UNITS[unit])
    } else {
        format!("{rounded:.1}{}", UNITS[unit])
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TokenDisplay {
    Exact(u64),
    LowerBound(u64),
    #[default]
    Unknown,
}

impl TokenDisplay {
    pub fn compact(self) -> String {
        match self {
            Self::Exact(value) => compact_tokens(value),
            Self::LowerBound(value) => format!("≥{}", compact_tokens(value)),
            Self::Unknown => "unknown".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProjectDisplay {
    Assigned {
        id: String,
        name: String,
    },
    NoProject,
    #[default]
    Unknown,
}

impl ProjectDisplay {
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Assigned { id, .. } => Some(id),
            Self::NoProject | Self::Unknown => None,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Assigned { name, .. } => name,
            Self::NoProject => "No Project",
            Self::Unknown => "unknown",
        }
    }
}

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
    pub title_source: String,
    pub state: String,
    pub profile: String,
    pub project: ProjectDisplay,
    pub tokens: TokenDisplay,
    pub model: Option<String>,
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
    pub title: String,
    pub role: Option<String>,
    pub nickname: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub detail_loaded: bool,
    pub tokens: TokenDisplay,
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
    Updated,
    Title,
    Project,
    Tokens,
    Agents,
    Depth,
    State,
    Profile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Viewport {
    pub row: usize,
    pub column: usize,
    pub cursor_column: usize,
    pub height: usize,
    pub width: usize,
}

const ROOT_MOVING_COLUMN_WIDTHS: [usize; 9] = [18, 10, 16, 10, 10, 6, 5, 14, 12];
const AGENT_MOVING_COLUMN_WIDTHS: [usize; 8] = [20, 14, 14, 10, 14, 12, 10, 12];
const COLUMN_SEPARATOR_WIDTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub page_size: usize,
    pub filter: Filter,
    pub sort: Sort,
    pub pane_width_percent: u16,
    pub color: bool,
    pub sensitive_content_acknowledged: bool,
    pub title_width: usize,
    pub root_title_width: usize,
    pub agent_title_width: usize,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            page_size: 25,
            filter: Filter::All,
            sort: Sort::Updated,
            pane_width_percent: 38,
            color: std::env::var_os("NO_COLOR").is_none(),
            sensitive_content_acknowledged: false,
            title_width: 48,
            root_title_width: 48,
            agent_title_width: 36,
        }
    }
}

impl Preferences {
    /// Minimal persistence format deliberately excludes queries and selections.
    pub fn to_toml_like(&self) -> String {
        format!("page_size = {}\nfilter = \"{:?}\"\nsort = \"{:?}\"\npane_width_percent = {}\ncolor = {}\nsensitive_content_acknowledged = {}\ntitle_width = {}\nroot_title_width = {}\nagent_title_width = {}\n",
            self.page_size, self.filter, self.sort, self.pane_width_percent, self.color,
            self.sensitive_content_acknowledged, self.title_width, self.root_title_width,
            self.agent_title_width)
    }

    pub fn from_toml_like(value: &str) -> Self {
        let mut preferences = Self::default();
        for line in value.lines() {
            let Some((key, raw)) = line.split_once('=') else {
                continue;
            };
            let raw = raw.trim().trim_matches('"');
            match key.trim() {
                "page_size" => preferences.page_size = raw.parse().unwrap_or(preferences.page_size),
                "filter" => {
                    preferences.filter = match raw {
                        "ActiveOnly" => Filter::ActiveOnly,
                        "ArchivedOnly" => Filter::ArchivedOnly,
                        _ => Filter::All,
                    }
                }
                "sort" => {
                    preferences.sort = match raw {
                        "Title" => Sort::Title,
                        "Project" => Sort::Project,
                        "Tokens" => Sort::Tokens,
                        "Agents" => Sort::Agents,
                        "Depth" => Sort::Depth,
                        "State" => Sort::State,
                        "Profile" => Sort::Profile,
                        _ => Sort::Updated,
                    }
                }
                "pane_width_percent" => {
                    preferences.pane_width_percent =
                        raw.parse().unwrap_or(preferences.pane_width_percent)
                }
                "color" => preferences.color = raw.parse().unwrap_or(preferences.color),
                "sensitive_content_acknowledged" => {
                    preferences.sensitive_content_acknowledged = raw.parse().unwrap_or(false)
                }
                "title_width" => {
                    preferences.title_width = raw.parse::<usize>().unwrap_or(48).clamp(24, 100)
                }
                "root_title_width" => {
                    preferences.root_title_width = raw.parse::<usize>().unwrap_or(48).clamp(24, 100)
                }
                "agent_title_width" => {
                    preferences.agent_title_width =
                        raw.parse::<usize>().unwrap_or(36).clamp(24, 100)
                }
                _ => {}
            }
        }
        preferences
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
    PageUp,
    PageDown,
    Home,
    End,
    ScrollLeft,
    ScrollRight,
    ScrollLeftPage,
    ScrollRightPage,
    Enter,
    Back,
    Tab,
    BackTab,
    ClearSearch,
    MouseSelect {
        index: usize,
    },
    /// A single click only moves the row cursor; opening is an explicit action.
    MouseDoubleClick {
        index: usize,
    },
    /// Alias used by terminal adapters that report a semantic open gesture.
    MouseOpen {
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
    SetViewport {
        width: usize,
        height: usize,
    },
    SelectSort(Sort),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Quit,
    LoadMore {
        cursor: String,
    },
    LoadAgents {
        conversation_id: String,
    },
    LoadAgentDetail {
        agent_id: String,
    },
    Refresh,
    Rebuild,
    OpenEvidence {
        agent_id: String,
    },
    OpenMessage {
        agent_id: String,
    },
    Search {
        query: String,
        filter: Filter,
    },
    Sort {
        field: Sort,
        direction: SortDirection,
    },
}

#[derive(Clone, Debug)]
struct NavigationState {
    screen: Screen,
    focus: Focus,
    conversation_selection: usize,
    agent_selection: usize,
    conversation_viewport: Viewport,
    tree_viewport: Viewport,
    detail_viewport: Viewport,
    help_viewport: Viewport,
    conversations: Vec<ConversationItem>,
    next_cursor: Option<String>,
    approximate_total: Option<usize>,
    preferences: Preferences,
    sort_direction: SortDirection,
    search: String,
    detail_wrap: bool,
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
    navigation: Vec<NavigationState>,
    focus: Focus,
    width: u16,
    search_editing: bool,
    search: String,
    refresh_progress: Option<RefreshProgress>,
    pending_snapshot: Option<Page<ConversationItem>>,
    rebuild_confirmation: bool,
    conversation_viewport: Viewport,
    tree_viewport: Viewport,
    detail_viewport: Viewport,
    help_viewport: Viewport,
    detail_wrap: bool,
    sort_direction: SortDirection,
    sort_overlay: bool,
    sort_selection: Sort,
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
            navigation: vec![],
            focus: Focus::Tree,
            width: 100,
            search_editing: false,
            search: String::new(),
            refresh_progress: None,
            pending_snapshot: None,
            rebuild_confirmation: false,
            conversation_viewport: Viewport::default(),
            tree_viewport: Viewport::default(),
            detail_viewport: Viewport::default(),
            help_viewport: Viewport::default(),
            detail_wrap: true,
            sort_direction: SortDirection::Descending,
            sort_overlay: false,
            sort_selection: Sort::Updated,
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
                self.agents = self.normalize_agents(agents);
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
            Event::Resize { width, height } => {
                self.width = width;
                self.set_active_viewport(width as usize, height.saturating_sub(7) as usize);
            }
            Event::SetViewport { width, height } => self.set_active_viewport(width, height),
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
                        min(index, self.visible_conversations().len().saturating_sub(1));
                    Self::follow_selection(
                        self.conversation_selection,
                        &mut self.conversation_viewport,
                    );
                }
                Screen::Conversation => {
                    self.agent_selection = min(index, self.agents.len().saturating_sub(1));
                    Self::follow_selection(self.agent_selection, &mut self.tree_viewport);
                }
                Screen::AgentDetail => {}
                Screen::Help => {}
            },
            Event::MouseDoubleClick { index } | Event::MouseOpen { index } => {
                match self.screen {
                    Screen::Conversations => {
                        self.conversation_selection =
                            min(index, self.visible_conversations().len().saturating_sub(1));
                        Self::follow_selection(
                            self.conversation_selection,
                            &mut self.conversation_viewport,
                        );
                    }
                    Screen::Conversation => {
                        self.agent_selection = min(index, self.agents.len().saturating_sub(1));
                        Self::follow_selection(self.agent_selection, &mut self.tree_viewport);
                    }
                    Screen::AgentDetail => return vec![],
                    Screen::Help => {}
                }
                return self.enter();
            }
            Event::Down if self.sort_overlay => self.move_sort_selection(true),
            Event::Down => return self.move_down(),
            Event::Up if self.sort_overlay => self.move_sort_selection(false),
            Event::Up => {
                self.move_up();
            }
            Event::PageUp => {
                self.move_page(false);
            }
            Event::PageDown => return self.move_page(true),
            Event::Home => self.move_home(),
            Event::End => self.move_end(),
            Event::ScrollLeft => self.move_horizontal(false, false),
            Event::ScrollRight => self.move_horizontal(true, false),
            Event::ScrollLeftPage => self.move_horizontal(false, true),
            Event::ScrollRightPage => self.move_horizontal(true, true),
            Event::Tab if !self.is_narrow() => self.focus = Focus::Detail,
            Event::BackTab if !self.is_narrow() => self.focus = Focus::Tree,
            Event::Enter => return self.enter(),
            Event::Back => self.back(),
            Event::Key(key) => return self.key(key),
            Event::SelectSort(field) => return self.apply_sort(field),
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
        if self.sort_overlay {
            return match key {
                'j' => {
                    self.move_sort_selection(true);
                    vec![]
                }
                'k' => {
                    self.move_sort_selection(false);
                    vec![]
                }
                '\n' => self.apply_sort(self.sort_selection),
                's' => {
                    self.sort_overlay = false;
                    vec![]
                }
                _ => vec![],
            };
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
                vec![Command::Search {
                    query: self.search.clone(),
                    filter: self.preferences.filter,
                }]
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
                self.push_navigation();
                self.screen = Screen::Help;
                vec![]
            }
            'h' => {
                self.back();
                vec![]
            }
            's' if self.screen == Screen::Conversations => {
                self.sort_overlay = !self.sort_overlay;
                self.sort_selection = self.preferences.sort;
                vec![]
            }
            'w' if matches!(self.screen, Screen::Conversation | Screen::AgentDetail) => {
                self.detail_wrap = !self.detail_wrap;
                vec![]
            }
            'g' => {
                self.move_home();
                vec![]
            }
            'G' => {
                self.move_end();
                vec![]
            }
            'H' => {
                self.move_horizontal(false, false);
                vec![]
            }
            'L' => {
                self.move_horizontal(true, false);
                vec![]
            }
            '[' if self.screen == Screen::Conversations => {
                self.preferences.title_width =
                    self.preferences.title_width.saturating_sub(4).max(24);
                self.preferences.root_title_width = self.preferences.title_width;
                vec![]
            }
            ']' if self.screen == Screen::Conversations => {
                self.preferences.title_width = (self.preferences.title_width + 4).min(100);
                self.preferences.root_title_width = self.preferences.title_width;
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
            return vec![Command::Search {
                query: self.search.clone(),
                filter: self.preferences.filter,
            }];
        }
        if self.sort_overlay {
            return self.apply_sort(self.sort_selection);
        }
        if self.screen == Screen::Conversations {
            if let Some(page) = self.pending_snapshot.take() {
                self.replace_page(page);
                return vec![];
            }
        }
        match self.screen {
            Screen::Conversations => {
                if let Some(item) = self.selected_conversation().cloned() {
                    self.push_navigation();
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
                    self.push_navigation();
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
        if self.sort_overlay {
            self.sort_overlay = false;
            return;
        }
        if let Some(state) = self.navigation.pop() {
            self.screen = state.screen;
            self.focus = state.focus;
            self.conversation_selection = state.conversation_selection;
            self.agent_selection = state.agent_selection;
            self.conversation_viewport = state.conversation_viewport;
            self.tree_viewport = state.tree_viewport;
            self.detail_viewport = state.detail_viewport;
            self.help_viewport = state.help_viewport;
            self.conversations = state.conversations;
            self.next_cursor = state.next_cursor;
            self.approximate_total = state.approximate_total;
            self.preferences = state.preferences;
            self.sort_direction = state.sort_direction;
            self.search = state.search;
            self.detail_wrap = state.detail_wrap;
            if self.screen == Screen::Conversations {
                self.selected_root_id = None;
            }
        }
    }

    fn move_down(&mut self) -> Vec<Command> {
        match self.screen {
            Screen::Conversations => {
                let len = self.visible_conversations().len();
                if self.conversation_selection + 1 < len {
                    self.conversation_selection += 1;
                    Self::follow_selection(
                        self.conversation_selection,
                        &mut self.conversation_viewport,
                    );
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
            Screen::Conversation if self.focus == Focus::Tree => {
                if self.agent_selection + 1 < self.agents.len() {
                    self.agent_selection += 1;
                    Self::follow_selection(self.agent_selection, &mut self.tree_viewport);
                }
                vec![]
            }
            Screen::Conversation | Screen::AgentDetail => {
                self.detail_viewport.row = self.detail_viewport.row.saturating_add(1);
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
            Screen::Conversation if self.focus == Focus::Tree => {
                self.agent_selection = self.agent_selection.saturating_sub(1)
            }
            Screen::Conversation | Screen::AgentDetail => {
                self.detail_viewport.row = self.detail_viewport.row.saturating_sub(1)
            }
            Screen::Help => {}
        }
        match self.screen {
            Screen::Conversations => {
                Self::follow_selection(self.conversation_selection, &mut self.conversation_viewport)
            }
            Screen::Conversation if self.focus == Focus::Tree => {
                Self::follow_selection(self.agent_selection, &mut self.tree_viewport)
            }
            Screen::AgentDetail | Screen::Conversation => {}
            Screen::Help => {}
        }
    }
    fn push_navigation(&mut self) {
        self.navigation.push(NavigationState {
            screen: self.screen,
            focus: self.focus,
            conversation_selection: self.conversation_selection,
            agent_selection: self.agent_selection,
            conversation_viewport: self.conversation_viewport,
            tree_viewport: self.tree_viewport,
            detail_viewport: self.detail_viewport,
            help_viewport: self.help_viewport,
            conversations: self.conversations.clone(),
            next_cursor: self.next_cursor.clone(),
            approximate_total: self.approximate_total,
            preferences: self.preferences.clone(),
            sort_direction: self.sort_direction,
            search: self.search.clone(),
            detail_wrap: self.detail_wrap,
        });
    }
    fn active_viewport_mut(&mut self) -> &mut Viewport {
        match self.screen {
            Screen::Conversations => &mut self.conversation_viewport,
            Screen::Conversation => match self.focus {
                Focus::Tree => &mut self.tree_viewport,
                Focus::Detail => &mut self.detail_viewport,
            },
            Screen::AgentDetail => &mut self.detail_viewport,
            Screen::Help => &mut self.help_viewport,
        }
    }
    fn set_active_viewport(&mut self, width: usize, height: usize) {
        let viewport = self.active_viewport_mut();
        viewport.width = width;
        viewport.height = height.max(1);
        match self.screen {
            Screen::Conversations => {
                Self::follow_selection(
                    self.conversation_selection,
                    &mut self.conversation_viewport,
                );
                self.clamp_conversation_columns();
            }
            Screen::Conversation => {
                Self::follow_selection(self.agent_selection, &mut self.tree_viewport);
                if self.focus == Focus::Tree {
                    self.clamp_agent_columns();
                }
            }
            _ => {}
        }
    }
    fn follow_selection(selection: usize, viewport: &mut Viewport) {
        if selection < viewport.row {
            viewport.row = selection;
        }
        let height = viewport.height.max(1);
        if selection >= viewport.row + height {
            viewport.row = selection + 1 - height;
        }
    }
    fn move_page(&mut self, down: bool) -> Vec<Command> {
        let amount = self.active_viewport_mut().height.max(1);
        match self.screen {
            Screen::Conversations => {
                let len = self.visible_conversations().len();
                self.conversation_selection = if down {
                    (self.conversation_selection + amount).min(len.saturating_sub(1))
                } else {
                    self.conversation_selection.saturating_sub(amount)
                };
                Self::follow_selection(
                    self.conversation_selection,
                    &mut self.conversation_viewport,
                );
                if down
                    && self.conversation_selection + 1 == len
                    && self.next_cursor.is_some()
                    && !self.loading_more
                {
                    return self.move_down();
                }
            }
            Screen::Conversation | Screen::AgentDetail if self.focus == Focus::Tree => {
                self.agent_selection = if down {
                    (self.agent_selection + amount).min(self.agents.len().saturating_sub(1))
                } else {
                    self.agent_selection.saturating_sub(amount)
                };
                Self::follow_selection(self.agent_selection, &mut self.tree_viewport);
            }
            _ => {
                let viewport = self.active_viewport_mut();
                viewport.row = if down {
                    viewport.row.saturating_add(amount)
                } else {
                    viewport.row.saturating_sub(amount)
                };
            }
        }
        vec![]
    }
    fn move_home(&mut self) {
        match self.screen {
            Screen::Conversations => {
                self.conversation_selection = 0;
                self.conversation_viewport.row = 0;
            }
            Screen::Conversation | Screen::AgentDetail if self.focus == Focus::Tree => {
                self.agent_selection = 0;
                self.tree_viewport.row = 0;
            }
            _ => self.active_viewport_mut().row = 0,
        }
    }
    fn move_end(&mut self) {
        match self.screen {
            Screen::Conversations => {
                self.conversation_selection = self.visible_conversations().len().saturating_sub(1);
                Self::follow_selection(
                    self.conversation_selection,
                    &mut self.conversation_viewport,
                );
            }
            Screen::Conversation | Screen::AgentDetail if self.focus == Focus::Tree => {
                self.agent_selection = self.agents.len().saturating_sub(1);
                Self::follow_selection(self.agent_selection, &mut self.tree_viewport);
            }
            _ => {}
        }
    }
    fn move_horizontal(&mut self, right: bool, page: bool) {
        match self.screen {
            Screen::Conversations => {
                let frozen = self.root_title_width().saturating_add(4);
                move_table_column(
                    &mut self.conversation_viewport,
                    frozen,
                    &ROOT_MOVING_COLUMN_WIDTHS,
                    right,
                    page,
                );
            }
            Screen::Conversation if self.focus == Focus::Tree => {
                let frozen = self.agent_title_width().saturating_add(4);
                move_table_column(
                    &mut self.tree_viewport,
                    frozen,
                    &AGENT_MOVING_COLUMN_WIDTHS,
                    right,
                    page,
                );
            }
            Screen::Conversation | Screen::AgentDetail => {
                self.scroll_detail_horizontal(right, page)
            }
            Screen::Help => self.scroll_detail_horizontal(right, page),
        }
    }

    fn scroll_detail_horizontal(&mut self, right: bool, page: bool) {
        let viewport = self.active_viewport_mut();
        let amount = if page { viewport.width.max(1) } else { 4 };
        viewport.column = if right {
            viewport.column.saturating_add(amount)
        } else {
            viewport.column.saturating_sub(amount)
        };
    }

    fn clamp_conversation_columns(&mut self) {
        let frozen = self.root_title_width().saturating_add(4);
        clamp_table_column(
            &mut self.conversation_viewport,
            frozen,
            &ROOT_MOVING_COLUMN_WIDTHS,
        );
    }

    fn clamp_agent_columns(&mut self) {
        let frozen = self.agent_title_width().saturating_add(4);
        clamp_table_column(&mut self.tree_viewport, frozen, &AGENT_MOVING_COLUMN_WIDTHS);
    }
    fn apply_sort(&mut self, field: Sort) -> Vec<Command> {
        self.sort_overlay = false;
        if self.preferences.sort == field {
            self.sort_direction = match self.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.preferences.sort = field;
            self.sort_direction = match field {
                Sort::Updated => SortDirection::Descending,
                _ => SortDirection::Ascending,
            };
        }
        self.conversation_selection = 0;
        self.conversation_viewport.row = 0;
        vec![Command::Sort {
            field,
            direction: self.sort_direction,
        }]
    }
    fn move_sort_selection(&mut self, down: bool) {
        const SORTS: [Sort; 8] = [
            Sort::Updated,
            Sort::Title,
            Sort::Project,
            Sort::Tokens,
            Sort::Agents,
            Sort::Depth,
            Sort::State,
            Sort::Profile,
        ];
        let current = SORTS
            .iter()
            .position(|s| *s == self.sort_selection)
            .unwrap_or(0);
        let next = if down {
            (current + 1) % SORTS.len()
        } else {
            (current + SORTS.len() - 1) % SORTS.len()
        };
        self.sort_selection = SORTS[next];
    }
    fn replace_page(&mut self, page: Page<ConversationItem>) {
        self.conversations = page.items;
        self.next_cursor = page.next_cursor;
        self.approximate_total = page.approximate_total;
        self.conversation_selection = 0;
    }

    fn normalize_agents(&self, mut agents: Vec<AgentItem>) -> Vec<AgentItem> {
        let Some(root_id) = self.selected_root_id.as_deref() else {
            return agents;
        };
        let conversation = self.conversations.iter().find(|item| item.id == root_id);

        // Keep the catalog's stable order within each sibling group while making
        // the parent-first relationship explicit for the table. A malformed
        // cycle or missing parent is still rendered, never silently dropped.
        for agent in &mut agents {
            if agent.title.trim().is_empty() {
                agent.title = agent.task_name.clone();
            }
        }
        let root_position = agents.iter().position(|agent| agent.id == root_id);
        if let Some(position) = root_position {
            let mut root = agents.remove(position);
            root.depth = 0;
            root.parent_id = None;
            root.task_name = "root conversation".into();
            root.title = conversation
                .map(|item| item.title.clone())
                .unwrap_or_else(|| "root conversation".into());
            root.model = conversation.and_then(|item| item.model.clone());
            root.tokens = conversation.map(|item| item.tokens).unwrap_or_default();
            agents.insert(0, root);
        } else {
            agents.insert(
                0,
                AgentItem {
                    id: root_id.into(),
                    parent_id: None,
                    depth: 0,
                    status: AgentStatus::StateOnly,
                    task_name: "root conversation".into(),
                    title: conversation
                        .map(|item| item.title.clone())
                        .unwrap_or_else(|| "root conversation".into()),
                    role: Some("root".into()),
                    nickname: None,
                    model: conversation.and_then(|item| item.model.clone()),
                    effort: None,
                    detail_loaded: false,
                    tokens: conversation.map(|item| item.tokens).unwrap_or_default(),
                },
            );
        }

        let mut ordered = Vec::with_capacity(agents.len());
        ordered.push(agents.remove(0));
        let mut emitted = std::collections::HashSet::new();
        emitted.insert(root_id.to_owned());
        while !agents.is_empty() {
            let mut progressed = false;
            let mut index = 0;
            while index < agents.len() {
                let parent_is_ready = agents[index]
                    .parent_id
                    .as_deref()
                    .map(|parent| emitted.contains(parent))
                    .unwrap_or(true);
                if parent_is_ready {
                    let mut agent = agents.remove(index);
                    agent.depth = agent
                        .parent_id
                        .as_deref()
                        .and_then(|parent| {
                            ordered.iter().find(|item: &&AgentItem| item.id == parent)
                        })
                        .map(|parent| parent.depth + 1)
                        .unwrap_or(agent.depth);
                    emitted.insert(agent.id.clone());
                    ordered.push(agent);
                    progressed = true;
                } else {
                    index += 1;
                }
            }
            if !progressed {
                // Orphans/cycles retain source order and their evidence status.
                ordered.append(&mut agents);
            }
        }
        ordered
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
        Self::follow_selection(self.conversation_selection, &mut self.conversation_viewport);
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
    pub fn root_title_width(&self) -> usize {
        // `title_width` was the pre-table preference. Honor it when loading an
        // older preference file while exposing the independent root/agent knobs
        // to newer callers.
        if self.preferences.root_title_width == 48 && self.preferences.title_width != 48 {
            self.preferences.title_width
        } else {
            self.preferences.root_title_width
        }
    }
    pub fn agent_title_width(&self) -> usize {
        self.preferences.agent_title_width
    }
    pub fn conversation_viewport(&self) -> Viewport {
        self.conversation_viewport
    }
    pub fn conversation_focused_column(&self) -> usize {
        self.conversation_viewport.cursor_column
    }
    pub fn conversation_column_cursor(&self) -> usize {
        self.conversation_focused_column()
    }
    pub fn tree_viewport(&self) -> Viewport {
        self.tree_viewport
    }
    pub fn agent_focused_column(&self) -> usize {
        self.tree_viewport.cursor_column
    }
    pub fn agent_column_cursor(&self) -> usize {
        self.agent_focused_column()
    }
    pub fn focused_column(&self) -> usize {
        match self.screen {
            Screen::Conversations => self.conversation_focused_column(),
            Screen::Conversation if self.focus == Focus::Tree => self.agent_focused_column(),
            _ => self.detail_viewport.cursor_column,
        }
    }
    pub fn detail_viewport(&self) -> Viewport {
        self.detail_viewport
    }
    pub fn help_viewport(&self) -> Viewport {
        self.help_viewport
    }
    pub fn detail_wrap(&self) -> bool {
        self.detail_wrap
    }
    pub fn sort_direction(&self) -> SortDirection {
        self.sort_direction
    }
    pub fn sort_overlay(&self) -> bool {
        self.sort_overlay
    }
    pub fn sort_selection(&self) -> Sort {
        self.sort_selection
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

fn column_starts(widths: &[usize]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(widths.len());
    let mut offset = 0;
    for width in widths {
        starts.push(offset);
        offset += *width + COLUMN_SEPARATOR_WIDTH;
    }
    starts
}

fn visible_column_range(offset: usize, capacity: usize, widths: &[usize]) -> (usize, usize) {
    let starts = column_starts(widths);
    let first = starts
        .iter()
        .rposition(|start| *start <= offset)
        .unwrap_or(0);
    let edge = offset.saturating_add(capacity.max(1));
    let mut last = first;
    for (index, start) in starts.iter().enumerate().skip(first + 1) {
        if start.saturating_add(widths[index]) <= edge {
            last = index;
        } else {
            break;
        }
    }
    (first, last)
}

fn clamp_table_column(viewport: &mut Viewport, frozen: usize, widths: &[usize]) {
    let starts = column_starts(widths);
    let capacity = viewport.width.saturating_sub(frozen);
    viewport.cursor_column = viewport.cursor_column.min(widths.len().saturating_sub(1));
    viewport.column = viewport.column.min(starts.last().copied().unwrap_or(0));
    let (first, last) = visible_column_range(viewport.column, capacity, widths);
    viewport.column = starts[first];
    if viewport.cursor_column < first {
        viewport.cursor_column = first;
    }
    if viewport.cursor_column > last {
        viewport.cursor_column = last;
    }
}

fn move_table_column(
    viewport: &mut Viewport,
    frozen: usize,
    widths: &[usize],
    right: bool,
    page: bool,
) {
    if widths.is_empty() {
        return;
    }
    let starts = column_starts(widths);
    let capacity = viewport.width.saturating_sub(frozen).max(1);
    clamp_table_column(viewport, frozen, widths);
    let (first, last) = visible_column_range(viewport.column, capacity, widths);
    if page {
        if right {
            let next = (last + 1).min(widths.len() - 1);
            viewport.column = starts[next];
            viewport.cursor_column = next;
        } else if first > 0 {
            let previous = first - 1;
            viewport.column = starts[previous];
            viewport.cursor_column = previous;
        }
        return;
    }
    if right {
        if viewport.cursor_column < last {
            viewport.cursor_column += 1;
        } else if last + 1 < widths.len() {
            viewport.cursor_column += 1;
            viewport.column = starts[viewport.cursor_column];
        }
    } else if viewport.cursor_column > first {
        viewport.cursor_column -= 1;
    } else if first > 0 {
        viewport.cursor_column -= 1;
        viewport.column = starts[viewport.cursor_column];
    }
}
