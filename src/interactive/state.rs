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
    pub title_source: String,
    pub state: String,
    pub profile: String,
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
    Updated,
    Title,
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
    pub height: usize,
    pub width: usize,
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
            sort: Sort::Updated,
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
                Screen::Conversation | Screen::AgentDetail => {
                    self.agent_selection = min(index, self.agents.len().saturating_sub(1));
                    Self::follow_selection(self.agent_selection, &mut self.tree_viewport);
                }
                Screen::Help => {}
            },
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
            Event::ScrollLeft => self.scroll_horizontal(false, false),
            Event::ScrollRight => self.scroll_horizontal(true, false),
            Event::ScrollLeftPage => self.scroll_horizontal(false, true),
            Event::ScrollRightPage => self.scroll_horizontal(true, true),
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
                self.scroll_horizontal(false, false);
                vec![]
            }
            'L' => {
                self.scroll_horizontal(true, false);
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
        if let Some(page) = self.pending_snapshot.take() {
            self.replace_page(page);
            return vec![];
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
                Self::follow_selection(self.conversation_selection, &mut self.conversation_viewport)
            }
            Screen::Conversation => {
                Self::follow_selection(self.agent_selection, &mut self.tree_viewport)
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
    fn scroll_horizontal(&mut self, right: bool, page: bool) {
        let viewport = self.active_viewport_mut();
        let amount = if page { viewport.width.max(1) } else { 4 };
        viewport.column = if right {
            viewport.column.saturating_add(amount)
        } else {
            viewport.column.saturating_sub(amount)
        };
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
        const SORTS: [Sort; 6] = [
            Sort::Updated,
            Sort::Title,
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
    pub fn conversation_viewport(&self) -> Viewport {
        self.conversation_viewport
    }
    pub fn tree_viewport(&self) -> Viewport {
        self.tree_viewport
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
