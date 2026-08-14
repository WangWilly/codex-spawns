use codex_spawns::interactive::{
    render, AgentItem, AgentStatus, App, ConversationItem, Event, Page, Preferences,
    ProjectDisplay, TokenDisplay,
};
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
            title_source: "user message".into(),
            state: "active".into(),
            profile: "partial".into(),
            project: ProjectDisplay::Unknown,
            tokens: TokenDisplay::Unknown,
            model: None,
        }],
        next_cursor: None,
        approximate_total: Some(1),
    }));
    let backend = TestBackend::new(150, 18);
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
    assert!(output.contains("partial"));
    assert!(output.contains("Title"));
    assert!(output.contains("Updated↓"));
    assert!(output.contains("Agents"));
    assert!(output.contains("Depth"));
    assert!(output.contains("State"));
    assert!(output.contains("Profile"));
    assert!(output.contains("Selected conversation"));
    assert!(output.contains("cwd: /repo"));
    assert!(output.contains("PgUp/PgDn"));
}

#[test]
fn narrow_table_keeps_title_frozen_and_cues_horizontal_overflow() {
    let mut app = App::new(Preferences {
        color: false,
        ..Preferences::default()
    });
    app.update(Event::ConversationsLoaded(Page {
        items: vec![ConversationItem {
            id: "019ff3ff-2e7e-7550-8ed5-8678372c750f".into(),
            title: "Human readable conversation".into(),
            cwd: "/repo".into(),
            last_activity_at: "2026-08-13T14:32:00Z".into(),
            archived: true,
            agent_count: 12,
            max_depth: 3,
            profile_complete: true,
            title_source: "cwd/time".into(),
            state: "archived".into(),
            profile: "complete".into(),
            project: ProjectDisplay::Unknown,
            tokens: TokenDisplay::Unknown,
            model: None,
        }],
        next_cursor: None,
        approximate_total: Some(588),
    }));
    app.update(Event::SetViewport {
        width: 70,
        height: 4,
    });
    app.update(Event::ScrollRightPage);
    let backend = TestBackend::new(75, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let output = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(output.contains("Human readable conversation"));
    assert!(output.contains("◀"));
    assert!(output.contains("Rows 1–1 of ~588"));
}

#[test]
fn resized_root_table_status_uses_rendered_body_capacity() {
    let mut app = App::new(Preferences {
        color: false,
        ..Preferences::default()
    });
    app.update(Event::ConversationsLoaded(Page {
        items: (0..40)
            .map(|index| ConversationItem {
                id: index.to_string(),
                title: format!("Conversation {index}"),
                cwd: "/repo".into(),
                last_activity_at: "now".into(),
                archived: false,
                agent_count: 0,
                max_depth: 0,
                profile_complete: true,
                title_source: "fixture".into(),
                state: "active".into(),
                profile: "complete".into(),
                project: ProjectDisplay::Unknown,
                tokens: TokenDisplay::Unknown,
                model: None,
            })
            .collect(),
        next_cursor: None,
        approximate_total: Some(40),
    }));
    app.update(Event::Resize {
        width: 163,
        height: 39,
    });
    let backend = TestBackend::new(163, 39);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let output = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(output.contains("Rows 1–27 of ~40"), "status was: {output}");
}

#[test]
fn focused_column_is_visible_in_header_and_rows_after_horizontal_move() {
    let mut app = App::new(Preferences {
        color: false,
        ..Preferences::default()
    });
    app.update(Event::ConversationsLoaded(Page {
        items: vec![ConversationItem {
            id: "root".into(),
            title: "Root".into(),
            cwd: "/repo".into(),
            last_activity_at: "now".into(),
            archived: false,
            agent_count: 1,
            max_depth: 1,
            profile_complete: true,
            title_source: "fixture".into(),
            state: "active".into(),
            profile: "complete".into(),
            project: ProjectDisplay::Assigned {
                id: "p1".into(),
                name: "Project".into(),
            },
            tokens: TokenDisplay::Exact(1),
            model: None,
        }],
        next_cursor: None,
        approximate_total: Some(1),
    }));
    app.update(Event::SetViewport {
        width: 75,
        height: 4,
    });
    let before_backend = TestBackend::new(75, 18);
    let mut before_terminal = Terminal::new(before_backend).unwrap();
    before_terminal.draw(|frame| render(frame, &app)).unwrap();
    let before = before_terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    app.update(Event::ScrollRight);
    let after_backend = TestBackend::new(75, 18);
    let mut after_terminal = Terminal::new(after_backend).unwrap();
    after_terminal.draw(|frame| render(frame, &app)).unwrap();
    let after = after_terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert_ne!(before, after);
    assert!(after.contains('▸'), "focused marker missing: {after}");
}

#[test]
fn cjk_title_uses_terminal_cell_width_without_shifting_headers() {
    let mut app = App::new(Preferences {
        color: false,
        title_width: 24,
        ..Preferences::default()
    });
    app.update(Event::ConversationsLoaded(Page {
        items: vec![ConversationItem {
            id: "cjk".into(),
            title: "目前的交互是互動模式🙂".into(),
            cwd: "/repo".into(),
            last_activity_at: "2026-08-13T14:32:00Z".into(),
            archived: false,
            agent_count: 3,
            max_depth: 2,
            profile_complete: true,
            title_source: "user message".into(),
            state: "active".into(),
            profile: "complete".into(),
            project: ProjectDisplay::Unknown,
            tokens: TokenDisplay::Unknown,
            model: None,
        }],
        next_cursor: None,
        approximate_total: Some(1),
    }));
    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let row = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(row.contains('目'));
    assert!(row.contains('互'));
    assert!(row.contains("Updated↓"));
}

#[test]
fn agent_table_is_full_width_by_default_with_fixed_headers_and_tree_glyphs() {
    let mut app = App::new(Preferences {
        color: false,
        ..Preferences::default()
    });
    app.update(Event::ConversationsLoaded(Page {
        items: vec![ConversationItem {
            id: "root".into(),
            title: "Root title".into(),
            cwd: "/repo".into(),
            last_activity_at: "2026-08-13T14:32:00Z".into(),
            archived: false,
            agent_count: 2,
            max_depth: 2,
            profile_complete: true,
            title_source: "user message".into(),
            state: "active".into(),
            profile: "complete".into(),
            project: ProjectDisplay::Assigned {
                id: "payments".into(),
                name: "Payments".into(),
            },
            tokens: TokenDisplay::Exact(12_400),
            model: Some("gpt-5.6-sol".into()),
        }],
        next_cursor: None,
        approximate_total: Some(1),
    }));
    app.update(Event::Enter);
    app.update(Event::AgentsLoaded {
        conversation_id: "root".into(),
        agents: vec![AgentItem {
            id: "child-2".into(),
            parent_id: Some("root".into()),
            depth: 1,
            status: AgentStatus::Complete,
            task_name: "review".into(),
            title: "Review implementation".into(),
            role: Some("reviewer".into()),
            nickname: Some("Scout".into()),
            model: Some("gpt-5.6-sol".into()),
            effort: Some("high".into()),
            detail_loaded: false,
            tokens: TokenDisplay::LowerBound(12_400),
        }],
    });
    // The fixed schema is wider than a compact terminal; use a wide backend
    // here to assert every header while the narrow-layout test covers frozen
    // Title plus horizontal scrolling separately.
    let backend = TestBackend::new(200, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let output = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    for header in [
        "Title",
        "Agent Name",
        "Nickname",
        "Model",
        "Effort",
        "Role",
        "Status",
        "Tokens",
        "ID",
    ] {
        assert!(output.contains(header), "missing header {header}");
    }
    assert!(output.contains("Root title"));
    assert!(output.contains("└─ Review implementation"));
    assert!(output.contains("≥12.4K"));
    assert!(!output.contains("Selected Agent Detail"));
}
