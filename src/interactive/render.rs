use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::{AgentStatus, App, Focus, Screen, Sort, SortDirection};

const TITLE_WIDTH: usize = 48;

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
    let headers = [
        header(
            "Title",
            app.preferences().sort == Sort::Title,
            arrow,
            TITLE_WIDTH,
        ),
        header(
            "Updated",
            app.preferences().sort == Sort::Updated,
            arrow,
            16,
        ),
        header("Agents", app.preferences().sort == Sort::Agents, arrow, 6),
        header("Depth", app.preferences().sort == Sort::Depth, arrow, 5),
        header("State", app.preferences().sort == Sort::State, arrow, 9),
        header(
            "Profile",
            app.preferences().sort == Sort::Profile,
            arrow,
            10,
        ),
        format!("{:<12}", "ID"),
    ];
    let mut lines = vec![Line::from(table_line(
        " ",
        &headers[0],
        &headers[1..].join(" "),
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
        let title = fit(&item.title, TITLE_WIDTH);
        let state = if item.archived { "archived" } else { "active" };
        let profile = if item.profile_complete {
            "complete"
        } else {
            "partial"
        };
        let rest = format!(
            "{:<16} {:>6} {:>5} {:<9} {:<10} {:<12}",
            display_time(&item.last_activity_at),
            item.agent_count,
            item.max_depth,
            state,
            profile,
            short_id(&item.id)
        );
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
        .map(|c| format!("{}  cwd: {}  id: {}", c.title, c.cwd, c.id))
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
    let selected = app.preferences().sort;
    let text = [
        Sort::Updated,
        Sort::Title,
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
    if app.is_narrow() {
        if detail_screen {
            render_detail(frame, app, area)
        } else {
            render_tree(frame, app, area)
        }
        return;
    }
    let width = app.preferences().pane_width_percent.clamp(20, 70);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(width),
            Constraint::Percentage(100 - width),
        ])
        .split(area);
    render_tree(frame, app, panes[0]);
    render_detail(frame, app, panes[1]);
}

fn render_tree(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let viewport = app.tree_viewport();
    let height = area.height.saturating_sub(2) as usize;
    let lines = app
        .visible_agents()
        .iter()
        .enumerate()
        .skip(viewport.row)
        .take(height)
        .map(|(index, agent)| {
            let branch = if agent.depth == 0 {
                String::new()
            } else {
                format!("{}└─ ", "  ".repeat(agent.depth.saturating_sub(1) as usize))
            };
            let marker = if index == app.selected_agent_index() {
                ">"
            } else {
                " "
            };
            styled(
                horizontal_slice(
                    &format!(
                        "{marker} {branch}{} {} {}",
                        agent.task_name,
                        agent.status.cue(),
                        agent.id
                    ),
                    viewport.column,
                    area.width.saturating_sub(2) as usize,
                ),
                index == app.selected_agent_index(),
                app.preferences().color,
            )
        })
        .collect::<Vec<_>>();
    let focus = if app.focus() == Focus::Tree { " *" } else { "" };
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Agent Tree{focus}")),
        ),
        area,
    );
}

fn render_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut values = vec![];
    if let Some(agent) = app.selected_agent() {
        values.extend([
            format!("{} {}", agent.status.cue(), agent.id),
            format!("task: {}", agent.task_name),
            format!("parent: {}", agent.parent_id.as_deref().unwrap_or("root")),
            format!("role: {}", agent.role.as_deref().unwrap_or("unknown")),
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
    let lines = values
        .into_iter()
        .skip(viewport.row)
        .map(|v| {
            if app.detail_wrap() {
                v
            } else {
                horizontal_slice(&v, viewport.column, area.width.saturating_sub(2) as usize)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let focus = if app.focus() == Focus::Detail {
        " *"
    } else {
        ""
    };
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Selected Agent Detail{focus}")),
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
        _ => "↑↓/jk Move  PgUp/PgDn Page  ←→ Scroll  Enter Open  Esc/h Back  Tab Pane  w Wrap  ? Help",
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
fn fit(value: &str, width: usize) -> String {
    let mut s: String = value.chars().take(width).collect();
    if value.chars().count() > width && width > 0 {
        s.pop();
        s.push('…');
    }
    format!("{s:<width$}")
}
fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}
fn display_time(value: &str) -> String {
    if value.is_empty() {
        "—".into()
    } else {
        value
            .replace('T', " ")
            .trim_end_matches('Z')
            .chars()
            .take(16)
            .collect()
    }
}
fn horizontal_slice(value: &str, offset: usize, width: usize) -> String {
    value.chars().skip(offset).take(width).collect()
}
fn table_line(marker: &str, title: &str, rest: &str, offset: usize, width: usize) -> String {
    let frozen = format!("{marker} {title} ");
    let available = width.saturating_sub(frozen.chars().count() + 2);
    let left = if offset > 0 { "◀" } else { " " };
    let mut moving = horizontal_slice(rest, offset, available.saturating_sub(2));
    if rest.chars().count() > offset + moving.chars().count() {
        moving.push('▶');
    }
    format!("{frozen}{left}{moving}")
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
