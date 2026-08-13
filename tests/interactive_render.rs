use codex_spawns::interactive::{render, App, ConversationItem, Event, Page, Preferences};
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn renders_title_identity_status_and_non_color_cues() {
    let mut app = App::new(Preferences {
        color: false,
        ..Preferences::default()
    });
    app.update(Event::ConversationsLoaded(Page {
        items: vec![ConversationItem {
            id: "root-123".into(),
            title: "Profiler redesign".into(),
            cwd: "/repo".into(),
            last_activity_at: "now".into(),
            archived: false,
            agent_count: 3,
            max_depth: 2,
            profile_complete: false,
        }],
        next_cursor: None,
        approximate_total: Some(1),
    }));
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let output = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(output.contains("Profiler redesign"));
    assert!(output.contains("root-123"));
    assert!(output.contains("[incomplete]"));
}
