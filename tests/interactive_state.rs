use codex_spawns::interactive::{
    AgentDetail, AgentItem, AgentStatus, App, Command, ConversationItem, Event, Filter, Focus,
    Page, Preferences, RefreshProgress, Screen, Sort, SortDirection,
};

fn conversation(id: &str, title: &str) -> ConversationItem {
    ConversationItem {
        id: id.into(),
        title: title.into(),
        cwd: format!("/work/{id}"),
        last_activity_at: "2026-08-13T12:00:00Z".into(),
        archived: false,
        agent_count: 2,
        max_depth: 2,
        profile_complete: true,
    }
}

#[test]
fn selection_follows_vertical_viewport_and_supports_page_home_end() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: (0..12)
            .map(|n| conversation(&n.to_string(), &format!("Conversation {n}")))
            .collect(),
        next_cursor: None,
        approximate_total: Some(12),
    }));
    app.update(Event::SetViewport {
        width: 80,
        height: 4,
    });
    for _ in 0..5 {
        app.update(Event::Down);
    }
    assert_eq!(app.selected_conversation_index(), 5);
    assert_eq!(app.conversation_viewport().row, 2);
    app.update(Event::PageDown);
    assert_eq!(app.selected_conversation_index(), 9);
    assert_eq!(app.conversation_viewport().row, 6);
    app.update(Event::Home);
    assert_eq!(
        (
            app.selected_conversation_index(),
            app.conversation_viewport().row
        ),
        (0, 0)
    );
    app.update(Event::End);
    assert_eq!(
        (
            app.selected_conversation_index(),
            app.conversation_viewport().row
        ),
        (11, 8)
    );
}

#[test]
fn horizontal_scroll_sort_and_navigation_stack_preserve_browse_state() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("one", "One"), conversation("two", "Two")],
        next_cursor: None,
        approximate_total: Some(2),
    }));
    app.update(Event::SetViewport {
        width: 20,
        height: 1,
    });
    app.update(Event::Down);
    app.update(Event::ScrollRight);
    assert_eq!(app.conversation_viewport().column, 4);
    assert_eq!(
        app.update(Event::SelectSort(Sort::Title)),
        vec![Command::Sort {
            field: Sort::Title,
            direction: SortDirection::Ascending
        }]
    );
    assert_eq!(
        app.update(Event::SelectSort(Sort::Title)),
        vec![Command::Sort {
            field: Sort::Title,
            direction: SortDirection::Descending
        }]
    );
    app.update(Event::Down);
    app.update(Event::Enter);
    app.update(Event::Back);
    assert_eq!(app.screen(), Screen::Conversations);
    assert_eq!(app.selected_conversation_index(), 1);
    assert_eq!(app.conversation_viewport().column, 4);
}

#[test]
fn keyboard_sort_overlay_selects_a_field_and_toggles_its_direction() {
    let mut app = App::new(Preferences::default());
    app.update(Event::Key('s'));
    assert!(app.sort_overlay());
    app.update(Event::Down);
    assert_eq!(app.sort_selection(), Sort::Title);
    assert_eq!(
        app.update(Event::Enter),
        vec![Command::Sort {
            field: Sort::Title,
            direction: SortDirection::Ascending
        }]
    );
    assert!(!app.sort_overlay());
}

#[test]
fn detail_pane_scroll_is_independent_from_agent_tree_selection() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("root", "Root")],
        next_cursor: None,
        approximate_total: None,
    }));
    app.update(Event::Enter);
    app.update(Event::AgentsLoaded {
        conversation_id: "root".into(),
        agents: vec![
            agent("one", None, 0, AgentStatus::Complete),
            agent("two", Some("one"), 1, AgentStatus::Complete),
        ],
    });
    app.update(Event::Tab);
    app.update(Event::Down);
    assert_eq!(app.selected_agent_index(), 0);
    assert_eq!(app.detail_viewport().row, 1);
    app.update(Event::BackTab);
    app.update(Event::Down);
    assert_eq!(app.selected_agent_index(), 1);
    assert_eq!(app.detail_viewport().row, 1);
}

#[test]
fn detail_and_source_actions_are_not_dropped() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("root", "Root")],
        next_cursor: None,
        approximate_total: None,
    }));
    assert_eq!(
        app.update(Event::Enter),
        vec![Command::LoadAgents {
            conversation_id: "root".into()
        }]
    );
    app.update(Event::AgentsLoaded {
        conversation_id: "root".into(),
        agents: vec![agent("child", Some("root"), 1, AgentStatus::Spawned)],
    });
    assert_eq!(
        app.update(Event::Enter),
        vec![Command::LoadAgentDetail {
            agent_id: "child".into()
        }]
    );
    app.update(Event::AgentDetailLoaded(AgentDetail {
        agent_id: "child".into(),
        lines: vec![
            ("event count".into(), "4".into()),
            ("provenance".into(), "rollout:3".into()),
        ],
    }));
    assert_eq!(
        app.selected_detail().unwrap().lines[0],
        ("event count".into(), "4".into())
    );
    assert_eq!(
        app.update(Event::Key('e')),
        vec![Command::OpenEvidence {
            agent_id: "child".into()
        }]
    );
    assert_eq!(
        app.update(Event::Key('m')),
        vec![Command::OpenMessage {
            agent_id: "child".into()
        }]
    );
}

fn agent(id: &str, parent: Option<&str>, depth: u32, status: AgentStatus) -> AgentItem {
    AgentItem {
        id: id.into(),
        parent_id: parent.map(str::to_owned),
        depth,
        status,
        task_name: format!("task-{id}"),
        role: None,
        nickname: None,
        model: None,
        effort: None,
        detail_loaded: false,
    }
}

#[test]
fn list_navigation_search_filter_and_cursor_request_are_commands() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![
            conversation("one", "Index profiler"),
            conversation("two", "Old work"),
        ],
        next_cursor: Some("cursor-2".into()),
        approximate_total: Some(8),
    }));
    assert_eq!(app.selected_conversation().unwrap().id, "one");

    assert_eq!(app.update(Event::Key('j')), vec![]);
    assert_eq!(app.selected_conversation().unwrap().id, "two");
    assert_eq!(app.update(Event::Key('/')), vec![]);
    for ch in "profiler".chars() {
        app.update(Event::Key(ch));
    }
    assert_eq!(
        app.update(Event::Enter),
        vec![Command::Search {
            query: "profiler".into(),
            filter: Filter::All
        }]
    );
    assert_eq!(app.visible_conversations().len(), 1);
    assert_eq!(app.visible_conversations()[0].id, "one");

    assert_eq!(
        app.update(Event::Key('f')),
        vec![Command::Search {
            query: "profiler".into(),
            filter: Filter::ActiveOnly
        }]
    );
    assert_eq!(app.filter(), Filter::ActiveOnly);
    app.update(Event::ClearSearch);
    app.select_last();
    assert_eq!(
        app.update(Event::Key('j')),
        vec![Command::LoadMore {
            cursor: "cursor-2".into()
        }]
    );
}

#[test]
fn entering_conversation_shows_breadcrumb_tree_and_lazy_detail() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("root", "Current title")],
        next_cursor: None,
        approximate_total: None,
    }));
    assert_eq!(
        app.update(Event::Enter),
        vec![Command::LoadAgents {
            conversation_id: "root".into()
        }]
    );
    app.update(Event::AgentsLoaded {
        conversation_id: "root".into(),
        agents: vec![
            agent("root", None, 0, AgentStatus::Complete),
            agent("child", Some("root"), 1, AgentStatus::Requested),
            agent("grandchild", Some("child"), 2, AgentStatus::Orphan),
        ],
    });
    assert_eq!(app.breadcrumb(), "Conversations / Current title / root");
    assert_eq!(
        app.visible_agents()
            .iter()
            .map(|a| a.depth)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    app.update(Event::Key('j'));
    assert_eq!(
        app.update(Event::Enter),
        vec![Command::LoadAgentDetail {
            agent_id: "child".into()
        }]
    );
    assert_eq!(app.screen(), Screen::AgentDetail);
    assert_eq!(app.update(Event::Back), vec![]);
    assert_eq!(app.screen(), Screen::Conversation);
    assert_eq!(app.update(Event::Back), vec![]);
    assert_eq!(app.screen(), Screen::Conversations);
}

#[test]
fn narrow_layout_moves_tree_to_detail_while_wide_layout_changes_focus() {
    let mut app = App::new(Preferences::default());
    app.update(Event::Resize {
        width: 70,
        height: 24,
    });
    assert!(app.is_narrow());
    app.update(Event::Resize {
        width: 120,
        height: 30,
    });
    assert!(!app.is_narrow());
    assert_eq!(app.focus(), Focus::Tree);
    app.update(Event::Tab);
    assert_eq!(app.focus(), Focus::Detail);
    app.update(Event::BackTab);
    assert_eq!(app.focus(), Focus::Tree);
}

#[test]
fn refresh_keeps_snapshot_until_user_applies_completed_update() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("old", "Old")],
        next_cursor: None,
        approximate_total: None,
    }));
    assert_eq!(app.update(Event::Key('r')), vec![Command::Refresh]);
    app.update(Event::RefreshProgress(RefreshProgress {
        scanned: 10,
        total: Some(20),
    }));
    assert_eq!(app.conversations()[0].id, "old");
    app.update(Event::RefreshReady(Page {
        items: vec![conversation("new", "New")],
        next_cursor: None,
        approximate_total: None,
    }));
    assert!(app.has_pending_snapshot());
    assert_eq!(app.conversations()[0].id, "old");
    app.update(Event::Enter);
    assert_eq!(app.conversations()[0].id, "new");
}

#[test]
fn load_more_is_not_requested_twice_while_the_page_is_in_flight() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("one", "One")],
        next_cursor: Some("next".into()),
        approximate_total: Some(2),
    }));
    assert_eq!(
        app.update(Event::Down),
        vec![Command::LoadMore {
            cursor: "next".into()
        }]
    );
    assert_eq!(app.update(Event::Down), vec![]);
    app.update(Event::MoreConversationsLoaded(Page {
        items: vec![conversation("two", "Two")],
        next_cursor: None,
        approximate_total: Some(2),
    }));
    assert_eq!(app.conversations().len(), 2);
}

#[test]
fn preferences_exclude_sensitive_navigation_state() {
    let mut app = App::new(Preferences::default());
    app.update(Event::Key('/'));
    app.update(Event::Key('x'));
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("one", "One")],
        next_cursor: None,
        approximate_total: None,
    }));
    app.update(Event::Enter);
    let encoded = app.preferences().to_toml_like();
    assert!(encoded.contains("page_size"));
    assert!(!encoded.contains("search"));
    assert!(!encoded.contains("one"));
}

#[test]
fn arrow_mouse_and_destructive_confirmation_are_explicit_events() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("one", "One"), conversation("two", "Two")],
        next_cursor: None,
        approximate_total: None,
    }));
    app.update(Event::Down);
    assert_eq!(app.selected_conversation().unwrap().id, "two");
    app.update(Event::Up);
    assert_eq!(app.selected_conversation().unwrap().id, "one");
    app.update(Event::MouseSelect { index: 1 });
    assert_eq!(app.selected_conversation().unwrap().id, "two");
    assert_eq!(app.update(Event::Key('R')), vec![]);
    assert!(app.rebuild_confirmation());
    assert_eq!(app.update(Event::Key('R')), vec![Command::Rebuild]);
}
