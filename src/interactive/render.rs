use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::{AgentStatus, App, Focus, Screen};

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(app.breadcrumb())
            .block(Block::default().borders(Borders::ALL).title("Profile")),
        chunks[0],
    );
    match app.screen() {
        Screen::Conversations => render_conversations(frame, app, chunks[1]),
        Screen::Conversation => render_conversation(frame, app, chunks[1], false),
        Screen::AgentDetail => render_conversation(frame, app, chunks[1], true),
        Screen::Help => render_help(frame, chunks[1]),
    }
    frame.render_widget(Paragraph::new(status_line(app)), chunks[2]);
}

fn render_conversations(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let selected = app.selected_conversation_index();
    let visible = app.visible_conversations();
    let items = visible.into_iter().enumerate().map(|(index, item)| {
        let marker = if index == selected { ">" } else { " " };
        let state = if item.archived { "archived" } else { "active" };
        let quality = if item.profile_complete {
            "complete"
        } else {
            "incomplete"
        };
        let line = format!(
            "{marker} {}  {}  [{}] [{}]  agents:{} depth:{}  {}",
            item.title, item.id, state, quality, item.agent_count, item.max_depth, item.cwd
        );
        ListItem::new(styled(line, index == selected, app.preferences().color))
    });
    let title = match app.approximate_total() {
        Some(total) => format!(
            "Root Conversations (loaded {} / ~{total})",
            app.conversations().len()
        ),
        None => format!("Root Conversations (loaded {})", app.conversations().len()),
    };
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn render_conversation(frame: &mut Frame<'_>, app: &App, area: Rect, detail_screen: bool) {
    if app.is_narrow() {
        if detail_screen {
            render_detail(frame, app, area);
        } else {
            render_tree(frame, app, area);
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
    let selected = app.selected_agent_index();
    let items = app
        .visible_agents()
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            let branch = if agent.depth == 0 {
                "".into()
            } else {
                format!("{}└─ ", "  ".repeat(agent.depth.saturating_sub(1) as usize))
            };
            let marker = if index == selected { ">" } else { " " };
            let line = format!(
                "{marker} {branch}{} {} {}",
                agent.task_name,
                agent.status.cue(),
                agent.id
            );
            ListItem::new(styled(line, index == selected, app.preferences().color))
        });
    let focus = if app.focus() == Focus::Tree { " *" } else { "" };
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Agent Tree{focus}")),
        ),
        area,
    );
}

fn render_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = vec![];
    if let Some(agent) = app.selected_agent() {
        lines.push(Line::from(format!("{} {}", agent.status.cue(), agent.id)));
        lines.push(Line::from(format!("task: {}", agent.task_name)));
        lines.push(Line::from(format!(
            "parent: {}",
            agent.parent_id.as_deref().unwrap_or("root")
        )));
        lines.push(Line::from(format!(
            "role: {}",
            agent.role.as_deref().unwrap_or("unknown")
        )));
        lines.push(Line::from(format!(
            "model: {} / effort: {}",
            agent.model.as_deref().unwrap_or("unknown"),
            agent.effort.as_deref().unwrap_or("unknown")
        )));
        if let Some(detail) = app.selected_detail() {
            for (key, value) in &detail.lines {
                lines.push(Line::from(format!("{key}: {value}")));
            }
        } else {
            lines.push(Line::from("detail: press Enter to load"));
        }
    } else {
        lines.push(Line::from("Loading agent tree…"));
    }
    let focus = if app.focus() == Focus::Detail {
        " *"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Selected Agent Detail{focus}")),
        ),
        area,
    );
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Paragraph::new("j/k or arrows move   Enter open   Esc/Backspace back\n/ search   f filter   r refresh   R rebuild\ne evidence   m full message   Tab switch pane   q quit")
        .block(Block::default().borders(Borders::ALL).title("Keyboard help")), area);
}

fn status_line(app: &App) -> String {
    if app.rebuild_confirmation() {
        return "[confirm] Press R again to rebuild the profile index".into();
    }
    if let Some(progress) = app.refresh_progress() {
        return match progress.total {
            Some(total) => format!("Refreshing: {} / {total} scanned", progress.scanned),
            None => format!("Refreshing: {} scanned", progress.scanned),
        };
    }
    if app.has_pending_snapshot() {
        return "[update-ready] Press Enter/apply to use refreshed snapshot".into();
    }
    if app.search_editing() {
        return format!("Search: {}", app.search());
    }
    format!(
        "Filter: {:?}   / search  f filter  ? help  q quit",
        app.filter()
    )
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
