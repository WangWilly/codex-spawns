use codex_spawns::interactive::{
    AgentDetail, AgentItem, AgentStatus, App, Command, ConversationItem, Event, Filter, Focus,
    Page, Preferences, ProjectDisplay, ProjectFilter, RefreshProgress, Screen, Sort, SortDirection,
    TokenDisplay,
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
        title_source: "user message".into(),
        state: "active".into(),
        profile: "complete".into(),
        project: ProjectDisplay::Unknown,
        tokens: TokenDisplay::Unknown,
        model: None,
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
    assert_eq!(app.conversation_focused_column(), 1);
    assert_eq!(app.conversation_viewport().column, 21);
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
    assert_eq!(app.conversation_focused_column(), 1);
    assert_eq!(app.conversation_viewport().column, 21);
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
    app.update(Event::Enter);
    app.update(Event::Down);
    assert_eq!(app.selected_agent_index(), 1);
    assert_eq!(app.detail_viewport().row, 1);
    app.update(Event::BackTab);
    app.update(Event::Down);
    assert_eq!(app.selected_agent_index(), 1);
    assert_eq!(app.detail_viewport().row, 2);
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
        title: format!("title-{id}"),
        role: None,
        nickname: None,
        model: None,
        effort: None,
        detail_loaded: false,
        tokens: TokenDisplay::Unknown,
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
            filter: Filter::All,
            project: ProjectFilter::All,
        }]
    );
    assert_eq!(app.visible_conversations().len(), 1);
    assert_eq!(app.visible_conversations()[0].id, "one");

    assert_eq!(
        app.update(Event::Key('f')),
        vec![Command::Search {
            query: "profiler".into(),
            filter: Filter::ActiveOnly,
            project: ProjectFilter::All,
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
    assert_eq!(app.focus(), Focus::Tree);
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

#[test]
fn title_width_is_bounded_and_round_trips_as_a_low_sensitive_preference() {
    let mut app = App::new(Preferences::default());
    for _ in 0..20 {
        app.update(Event::Key('['));
    }
    assert_eq!(app.preferences().title_width, 24);
    for _ in 0..30 {
        app.update(Event::Key(']'));
    }
    assert_eq!(app.preferences().title_width, 100);
    let decoded = Preferences::from_toml_like(&app.preferences().to_toml_like());
    assert_eq!(decoded.title_width, 100);
}

#[test]
fn pending_refresh_does_not_intercept_enter_inside_a_conversation() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("old", "Old")],
        next_cursor: None,
        approximate_total: Some(1),
    }));
    app.update(Event::Enter);
    app.update(Event::AgentsLoaded {
        conversation_id: "old".into(),
        agents: vec![agent("child", Some("old"), 1, AgentStatus::Spawned)],
    });
    app.update(Event::RefreshReady(Page {
        items: vec![conversation("new", "New")],
        next_cursor: None,
        approximate_total: Some(1),
    }));
    assert_eq!(
        app.update(Event::Enter),
        vec![Command::LoadAgentDetail {
            agent_id: "child".into()
        }]
    );
    app.update(Event::Back);
    app.update(Event::Back);
    assert_eq!(app.selected_conversation().unwrap().id, "old");
    assert!(app.has_pending_snapshot());
}

#[test]
fn typed_profile_values_have_compact_unknown_and_lower_bound_forms() {
    assert_eq!(TokenDisplay::Exact(12_400).compact(), "12.4K");
    assert_eq!(TokenDisplay::LowerBound(12_400).compact(), "≥12.4K");
    assert_eq!(TokenDisplay::Unknown.compact(), "unknown");
    let project = ProjectDisplay::Assigned {
        id: "project-1".into(),
        name: "Payments".into(),
    };
    assert_eq!(project.id(), Some("project-1"));
    assert_eq!(project.name(), "Payments");
    assert_eq!(ProjectDisplay::NoProject.name(), "No Project");
}

#[test]
fn agent_table_is_parent_first_and_double_click_restores_browse_position() {
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
            agent("grandchild", Some("child"), 2, AgentStatus::Complete),
            agent("child", Some("root"), 1, AgentStatus::Spawned),
        ],
    });
    assert_eq!(
        app.visible_agents()
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "child", "grandchild"]
    );
    app.update(Event::SetViewport {
        width: 30,
        height: 2,
    });
    app.update(Event::Down);
    let before = (app.selected_agent_index(), app.tree_viewport());
    assert_eq!(
        app.update(Event::MouseDoubleClick { index: 1 }),
        vec![Command::LoadAgentDetail {
            agent_id: "child".into()
        }]
    );
    app.update(Event::Key('w'));
    app.update(Event::ScrollRight);
    app.update(Event::Back);
    assert_eq!(app.screen(), Screen::Conversation);
    assert_eq!((app.selected_agent_index(), app.tree_viewport()), before);
    assert!(app.detail_wrap());
}

#[test]
fn root_and_agent_title_width_preferences_round_trip_independently() {
    let preferences = Preferences {
        root_title_width: 28,
        agent_title_width: 64,
        ..Preferences::default()
    };
    let decoded = Preferences::from_toml_like(&preferences.to_toml_like());
    assert_eq!(decoded.root_title_width, 28);
    assert_eq!(decoded.agent_title_width, 64);
}

#[test]
fn horizontal_cursor_reveals_one_complete_column_at_table_edges() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("one", "One")],
        next_cursor: None,
        approximate_total: None,
    }));
    app.update(Event::SetViewport {
        width: 75,
        height: 4,
    });
    assert_eq!(app.conversation_focused_column(), 0);
    app.update(Event::ScrollRight);
    assert_eq!(
        (
            app.conversation_focused_column(),
            app.conversation_viewport().column
        ),
        (1, 21)
    );
    app.update(Event::ScrollRight);
    assert_eq!(
        (
            app.conversation_focused_column(),
            app.conversation_viewport().column
        ),
        (2, 34)
    );
    app.update(Event::ScrollLeft);
    assert_eq!(
        (
            app.conversation_focused_column(),
            app.conversation_viewport().column
        ),
        (1, 21)
    );
}

#[test]
fn project_filter_cycles_known_projects_and_search_matches_project_name() {
    let mut alpha = conversation("alpha", "Alpha");
    alpha.project = ProjectDisplay::Assigned {
        id: "p-alpha".into(),
        name: "Alpha Project".into(),
    };
    let mut beta = conversation("beta", "Beta");
    beta.project = ProjectDisplay::Assigned {
        id: "p-beta".into(),
        name: "Beta Project".into(),
    };
    let mut no_project = conversation("none", "No Project");
    no_project.project = ProjectDisplay::NoProject;
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![beta, no_project, alpha],
        next_cursor: None,
        approximate_total: Some(3),
    }));
    assert_eq!(
        app.project_filter_options(),
        vec![
            ProjectFilter::All,
            ProjectFilter::Assigned("p-alpha".into()),
            ProjectFilter::Assigned("p-beta".into()),
            ProjectFilter::NoProject,
            ProjectFilter::Unknown,
        ]
    );
    assert_eq!(
        app.update(Event::Key('p')),
        vec![Command::Search {
            query: String::new(),
            filter: Filter::All,
            project: ProjectFilter::Assigned("p-alpha".into()),
        }]
    );
    assert_eq!(app.visible_conversations().len(), 1);
    app.update(Event::Key('p'));
    assert_eq!(app.visible_conversations()[0].id, "beta");
    app.update(Event::ClearSearch);
    app.update(Event::Key('/'));
    app.update(Event::Key('b'));
    app.update(Event::Key('e'));
    app.update(Event::Key('t'));
    app.update(Event::Key('a'));
    app.update(Event::Key(' '));
    app.update(Event::Key('p'));
    assert_eq!(app.visible_conversations().len(), 1);
    assert_eq!(app.visible_conversations()[0].id, "beta");
}

#[test]
fn agent_title_width_changes_only_inside_agent_table_and_tab_does_not_focus_hidden_pane() {
    let mut app = App::new(Preferences::default());
    app.update(Event::ConversationsLoaded(Page {
        items: vec![conversation("root", "Root")],
        next_cursor: None,
        approximate_total: None,
    }));
    app.update(Event::Enter);
    app.update(Event::AgentsLoaded {
        conversation_id: "root".into(),
        agents: vec![agent("child", Some("root"), 1, AgentStatus::Spawned)],
    });
    let root_width = app.preferences().root_title_width;
    app.update(Event::Key('['));
    assert_eq!(app.preferences().root_title_width, root_width);
    let agent_width = app.preferences().agent_title_width;
    app.update(Event::Key(']'));
    assert_eq!(app.preferences().agent_title_width, agent_width + 4);
    app.update(Event::Tab);
    assert_eq!(app.focus(), Focus::Tree);
}
