use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::{
    agent_columns, root_columns, AgentStatus, App, ColumnKey, Focus, Screen, Sort, SortDirection,
};
use chrono::{DateTime, Local};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(7),
            Constraint::Length(3),
        ])
        .split(frame.area());
    frame.render_widget(
        Paragraph::new(app.breadcrumb())
            .block(Block::default().borders(Borders::ALL).title("Profile")),
        chunks[0],
    );
    match app.screen() {
        Screen::Conversations => render_conversations(frame, app, chunks[1]),
        Screen::Conversation => render_conversation(frame, app, chunks[1], false),
        Screen::AgentDetail => render_conversation(frame, app, chunks[1], true),
        Screen::Help => render_help(frame, app, chunks[1]),
    }
    frame.render_widget(Paragraph::new(status_lines(app)), chunks[2]);
}

fn render_conversations(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(3)])
        .split(area);
    let viewport = app.conversation_viewport();
    let body_height = parts[0].height.saturating_sub(3) as usize;
    let visible = app.visible_conversations();
    let start = viewport.row.min(visible.len());
    let end = (start + body_height).min(visible.len());
    let arrow = match app.sort_direction() {
        SortDirection::Ascending => "↑",
        SortDirection::Descending => "↓",
    };
    let title_width = super::table_title_width(app.root_title_width(), area.width as usize);
    let columns = root_columns(title_width);
    let headers = columns
        .iter()
        .map(|column| {
            header(
                column.header,
                matches!(
                    (column.key, app.preferences().sort),
                    (ColumnKey::Title, Sort::Title)
                        | (ColumnKey::Project, Sort::Project)
                        | (ColumnKey::Tokens, Sort::Tokens)
                        | (ColumnKey::Updated, Sort::Updated)
                        | (ColumnKey::State, Sort::State)
                        | (ColumnKey::Profile, Sort::Profile)
                        | (ColumnKey::Agents, Sort::Agents)
                        | (ColumnKey::Depth, Sort::Depth)
                ),
                arrow,
                column.width,
            )
        })
        .collect::<Vec<_>>();
    let mut lines = vec![Line::from(table_line(
        " ",
        &headers[0],
        &headers[1..].join(" | "),
        viewport.column,
        area.width as usize,
    ))];
    for (index, item) in visible[start..end].iter().enumerate() {
        let absolute = start + index;
        let marker = if absolute == app.selected_conversation_index() {
            ">"
        } else {
            " "
        };
        let title = fit(&item.title, title_width);
        let rest = columns[1..]
            .iter()
            .map(|column| fit(&root_cell(item, column.key), column.width))
            .collect::<Vec<_>>()
            .join(" | ");
        lines.push(styled(
            table_line(marker, &title, &rest, viewport.column, area.width as usize),
            absolute == app.selected_conversation_index(),
            app.preferences().color,
        ));
    }
    let title = match app.approximate_total() {
        Some(total) => format!(
            "Root Conversations (loaded {} / ~{total})",
            app.conversations().len()
        ),
        None => format!("Root Conversations (loaded {})", app.conversations().len()),
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        parts[0],
    );
    let preview = app
        .selected_conversation()
        .map(|c| {
            format!(
                "{}  cwd: {}  id: {}  title source: {}",
                c.title, c.cwd, c.id, c.title_source
            )
        })
        .unwrap_or_else(|| "No conversation selected".into());
    frame.render_widget(
        Paragraph::new(preview).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Selected conversation"),
        ),
        parts[1],
    );
    if app.sort_overlay() {
        render_sort_overlay(frame, app, parts[0]);
    }
}

fn render_sort_overlay(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let popup = Rect::new(
        area.x + 3,
        area.y + 2,
        30.min(area.width.saturating_sub(4)),
        9.min(area.height.saturating_sub(3)),
    );
    let selected = app.sort_selection();
    let text = [
        Sort::Updated,
        Sort::Title,
        Sort::Project,
        Sort::Tokens,
        Sort::Agents,
        Sort::Depth,
        Sort::State,
        Sort::Profile,
    ]
    .into_iter()
    .map(|s| format!("{} {:?}", if s == selected { ">" } else { " " }, s))
    .collect::<Vec<_>>()
    .join("\n");
    frame.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Sort (Enter toggles direction)"),
        ),
        popup,
    );
}

fn render_conversation(frame: &mut Frame<'_>, app: &App, area: Rect, detail_screen: bool) {
    // Agent Table is the browse surface at every terminal width. Details are
    // deliberately a separate full-screen state entered with Enter/double-click.
    if detail_screen {
        render_detail(frame, app, area)
    } else {
        render_tree(frame, app, area)
    }
}

fn render_tree(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let viewport = app.tree_viewport();
    let height = area.height.saturating_sub(2) as usize;
    let title_width = super::table_title_width(app.agent_title_width(), area.width as usize);
    let columns = agent_columns(title_width);
    let headers = columns
        .iter()
        .map(|column| header(column.header, false, "", column.width))
        .collect::<Vec<_>>();
    let mut lines = vec![Line::from(table_line(
        " ",
        &headers[0],
        &headers[1..].join(" | "),
        viewport.column,
        area.width as usize,
    ))];
    for (index, agent) in app
        .visible_agents()
        .iter()
        .enumerate()
        .skip(viewport.row)
        .take(height.saturating_sub(1))
    {
        let branch = agent_branch(app.visible_agents(), index);
        let title = fit(&format!("{branch}{}", agent.title), title_width);
        let rest = columns[1..]
            .iter()
            .map(|column| fit(&agent_cell(agent, column.key), column.width))
            .collect::<Vec<_>>()
            .join(" | ");
        let marker = if index == app.selected_agent_index() {
            ">"
        } else {
            " "
        };
        lines.push(styled(
            table_line(marker, &title, &rest, viewport.column, area.width as usize),
            index == app.selected_agent_index(),
            app.preferences().color,
        ));
    }
    let focus = if app.focus() == Focus::Tree { " *" } else { "" };
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Agent Table{focus}")),
        ),
        area,
    );
}

fn render_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut values = vec![];
    if let Some(agent) = app.selected_agent() {
        values.extend([
            format!("{} {}", agent.status.cue(), agent.id),
            format!("title: {}", agent.title),
            format!("task: {}", agent.task_name),
            format!("parent: {}", agent.parent_id.as_deref().unwrap_or("root")),
            format!("role: {}", agent.role.as_deref().unwrap_or("unknown")),
            format!("tokens: {}", agent.tokens.compact()),
            format!(
                "model: {} / effort: {}",
                agent.model.as_deref().unwrap_or("unknown"),
                agent.effort.as_deref().unwrap_or("unknown")
            ),
        ]);
        if let Some(detail) = app.selected_detail() {
            values.extend(detail.lines.iter().map(|(k, v)| format!("{k}: {v}")));
        } else {
            values.push("detail: press Enter to load".into());
        }
    } else {
        values.push("Loading agent tree…".into());
    }
    let viewport = app.detail_viewport();
    let inner_width = area.width.saturating_sub(2) as usize;
    let lines = if app.detail_wrap() {
        values
            .into_iter()
            .flat_map(|v| wrap_display(&v, inner_width))
            .skip(viewport.row)
            .take(area.height.saturating_sub(2) as usize)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        values
            .into_iter()
            .skip(viewport.row)
            .map(|v| horizontal_slice(&v, viewport.column, inner_width))
            .take(area.height.saturating_sub(2) as usize)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Selected Agent Detail"),
    );
    frame.render_widget(
        if app.detail_wrap() {
            paragraph.wrap(Wrap { trim: false })
        } else {
            paragraph
        },
        area,
    );
}

fn render_help(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let help = [
        "↑↓/j/k  Move",
        "PgUp/PgDn or Ctrl+U/Ctrl+D  Page",
        "←→/H/L  Scroll horizontally",
        "Home/End or g/G  First/last",
        "Enter  Open",
        "Esc/Backspace/h  Back",
        "s  Sort",
        "/  Search",
        "w  Toggle detail wrap",
        "?  Help",
        "q  Quit",
    ];
    frame.render_widget(
        Paragraph::new(
            help.into_iter()
                .skip(app.help_viewport().row)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Keyboard help"),
        ),
        area,
    );
}

fn status_lines(app: &App) -> String {
    let controls = match app.screen() {
        Screen::Conversations => "↑↓/jk Move  PgUp/PgDn Page  ←→ Scroll  Enter Open  s Sort  / Search  ? Help  q Quit",
        Screen::Help => "PgUp/PgDn Scroll  Esc/h Back  q Quit",
        _ => "↑↓/jk Move  PgUp/PgDn Page  ←→ Scroll  Enter Open  Esc/h Back  [/] Title  w Wrap  ? Help",
    };
    let viewport = app.conversation_viewport();
    let total = app.approximate_total().unwrap_or(app.conversations().len());
    let end = (viewport.row + viewport.height.max(1)).min(app.conversations().len());
    let horizontal = if viewport.column > 0 {
        " ◀ horizontal ▶"
    } else {
        " horizontal ▶"
    };
    let status = if app.rebuild_confirmation() {
        "Confirm rebuild: press R again".into()
    } else if let Some(p) = app.refresh_progress() {
        format!(
            "Refreshing {} / {}",
            p.scanned,
            p.total.map_or_else(|| "?".into(), |n| n.to_string())
        )
    } else {
        format!(
            "Rows {}–{} of ~{}  {:?} {:?}  {:?}{}{}",
            if total == 0 { 0 } else { viewport.row + 1 },
            end,
            total,
            app.preferences().sort,
            app.sort_direction(),
            app.filter(),
            horizontal,
            if app.has_pending_snapshot() {
                "  Refresh ready"
            } else {
                ""
            }
        )
    };
    format!("{controls}\n{status}")
}

fn header(name: &str, selected: bool, arrow: &str, width: usize) -> String {
    fit(
        &format!("{name}{}", if selected { arrow } else { "" }),
        width,
    )
}

fn agent_branch(agents: &[super::AgentItem], index: usize) -> String {
    let agent = &agents[index];
    if agent.depth == 0 {
        return String::new();
    }
    let has_sibling_after = agents[index + 1..]
        .iter()
        .any(|next| next.depth == agent.depth && next.parent_id == agent.parent_id);
    let glyph = if has_sibling_after {
        "├─ "
    } else {
        "└─ "
    };
    format!(
        "{}{}",
        "  ".repeat(agent.depth.saturating_sub(1) as usize),
        glyph
    )
}

fn root_cell(item: &super::ConversationItem, key: ColumnKey) -> String {
    match key {
        ColumnKey::Project => item.project.name().into(),
        ColumnKey::Tokens => item.tokens.compact(),
        ColumnKey::Updated => display_time(&item.last_activity_at),
        ColumnKey::State => item.state.clone(),
        ColumnKey::Profile => item.profile.clone(),
        ColumnKey::Agents => item.agent_count.to_string(),
        ColumnKey::Depth => item.max_depth.to_string(),
        ColumnKey::Model => item.model.clone().unwrap_or_else(|| "unknown".into()),
        ColumnKey::Id => short_id(&item.id),
        ColumnKey::Title
        | ColumnKey::AgentName
        | ColumnKey::Nickname
        | ColumnKey::Effort
        | ColumnKey::Role
        | ColumnKey::Status => String::new(),
    }
}

fn agent_cell(agent: &super::AgentItem, key: ColumnKey) -> String {
    match key {
        ColumnKey::AgentName => agent.task_name.clone(),
        ColumnKey::Nickname => agent.nickname.clone().unwrap_or_else(|| "unknown".into()),
        ColumnKey::Model => agent.model.clone().unwrap_or_else(|| "unknown".into()),
        ColumnKey::Effort => agent.effort.clone().unwrap_or_else(|| "unknown".into()),
        ColumnKey::Role => agent.role.clone().unwrap_or_else(|| "unknown".into()),
        ColumnKey::Status => agent.status.cue().into(),
        ColumnKey::Tokens => agent.tokens.compact(),
        ColumnKey::Id => short_id(&agent.id),
        ColumnKey::Title
        | ColumnKey::Project
        | ColumnKey::Updated
        | ColumnKey::State
        | ColumnKey::Profile
        | ColumnKey::Agents
        | ColumnKey::Depth => String::new(),
    }
}

fn fit(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let truncated = UnicodeWidthStr::width(value) > width;
    let target = if truncated {
        width.saturating_sub(1)
    } else {
        width
    };
    let mut s = horizontal_slice(value, 0, target);
    if truncated {
        s.push('…');
    }
    s.push_str(&" ".repeat(width.saturating_sub(UnicodeWidthStr::width(s.as_str()))));
    s
}
fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}
fn display_time(value: &str) -> String {
    if value.is_empty() {
        "—".into()
    } else {
        DateTime::parse_from_rfc3339(value)
            .map(|time| {
                time.with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|_| fit(value, 16).trim_end().to_owned())
    }
}
fn horizontal_slice(value: &str, offset: usize, width: usize) -> String {
    let mut passed = 0;
    let mut used = 0;
    let mut result = String::new();
    for ch in value.chars() {
        let cells = UnicodeWidthChar::width(ch).unwrap_or(0);
        if passed + cells <= offset || passed < offset {
            passed += cells;
            continue;
        }
        if used + cells > width {
            break;
        }
        used += cells;
        result.push(ch);
    }
    result
}
fn table_line(marker: &str, title: &str, rest: &str, offset: usize, width: usize) -> String {
    let frozen = format!("{marker} {title} ");
    let available = width.saturating_sub(UnicodeWidthStr::width(frozen.as_str()) + 2);
    let left = if offset > 0 { "◀" } else { " " };
    let mut moving = horizontal_slice(rest, offset, available.saturating_sub(2));
    if UnicodeWidthStr::width(rest) > offset + UnicodeWidthStr::width(moving.as_str()) {
        moving.push('▶');
    }
    format!("{frozen}{left}{moving}")
}
fn wrap_display(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut result = Vec::new();
    let mut current = String::new();
    let mut cells = 0;
    for ch in value.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if cells + w > width && !current.is_empty() {
            result.push(std::mem::take(&mut current));
            cells = 0;
        }
        current.push(ch);
        cells += w;
    }
    result.push(current);
    result
}
fn styled(text: String, selected: bool, color: bool) -> Line<'static> {
    let style = if selected {
        let style = Style::default().add_modifier(Modifier::BOLD);
        if color {
            style.fg(Color::Cyan)
        } else {
            style
        }
    } else {
        Style::default()
    };
    Line::from(Span::styled(text, style))
}
#[allow(dead_code)]
fn _status_is_exhaustive(status: AgentStatus) -> &'static str {
    status.cue()
}
