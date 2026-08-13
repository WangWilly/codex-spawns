use super::*;
use tempfile::TempDir;

fn open() -> (TempDir, ProfileIndex) {
    let dir = tempfile::tempdir().unwrap();
    let index = ProfileIndex::open(IndexOptions {
        path: dir.path().join("index.sqlite"),
    })
    .unwrap();
    (dir, index)
}

fn conversation(id: &str, activity: &str) -> ConversationRecord {
    ConversationRecord {
        id: id.into(),
        title: format!("Conversation {id}"),
        title_source: "derived".into(),
        cwd: "/work".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        last_activity_at: activity.into(),
        archived: false,
        model: Some("gpt-5".into()),
        status: Some("complete".into()),
        agent_count: 2,
        max_depth: 1,
        profile_complete: true,
    }
}

fn ids(page: BrowsePage) -> Vec<String> {
    page.conversations
        .into_iter()
        .map(|record| record.id)
        .collect()
}

#[test]
fn browse_orders_the_entire_catalog_by_each_public_sort_field() {
    let (_dir, mut index) = open();
    let mut a = conversation("a", "2026-01-02");
    a.title = "Zulu".into();
    a.agent_count = 1;
    a.max_depth = 3;
    let mut b = conversation("b", "2026-01-01");
    b.title = "Alpha".into();
    b.agent_count = 4;
    b.max_depth = 1;
    b.archived = true;
    b.profile_complete = false;
    index
        .refresh(
            RefreshBatch {
                conversations: vec![a, b],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let cases = [
        (SortField::Updated, vec!["b", "a"]),
        (SortField::Title, vec!["b", "a"]),
        (SortField::Agents, vec!["a", "b"]),
        (SortField::Depth, vec!["b", "a"]),
        (SortField::State, vec!["a", "b"]),
        (SortField::Profile, vec!["a", "b"]),
    ];
    for (field, expected) in cases {
        let page = index
            .browse_ordered(
                &ConversationFilter::default(),
                None,
                25,
                BrowseOrder {
                    field,
                    direction: SortDirection::Asc,
                },
            )
            .unwrap();
        assert_eq!(ids(page), expected, "field {field:?}");
        let page = index
            .browse_ordered(
                &ConversationFilter::default(),
                None,
                25,
                BrowseOrder {
                    field,
                    direction: SortDirection::Desc,
                },
            )
            .unwrap();
        assert_eq!(
            ids(page),
            expected.into_iter().rev().collect::<Vec<_>>(),
            "field {field:?} descending"
        );
    }
}

#[test]
fn updated_unknown_is_bottom_in_both_directions_and_ids_break_ties() {
    let (_dir, mut index) = open();
    index
        .refresh(
            RefreshBatch {
                conversations: vec![
                    conversation("b", "2026-01-01"),
                    conversation("a", "2026-01-01"),
                    conversation("unknown", ""),
                ],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    for direction in [SortDirection::Asc, SortDirection::Desc] {
        let page = index
            .browse_ordered(
                &ConversationFilter::default(),
                None,
                25,
                BrowseOrder {
                    field: SortField::Updated,
                    direction,
                },
            )
            .unwrap();
        assert_eq!(page.conversations.last().unwrap().id, "unknown");
        assert_eq!(&ids(page)[..2], &["a", "b"]);
    }
}

#[test]
fn cursor_is_bound_to_order_and_snapshot() {
    let (_dir, mut index) = open();
    index
        .refresh(
            RefreshBatch {
                conversations: vec![
                    conversation("a", "1"),
                    conversation("b", "2"),
                    conversation("c", "3"),
                ],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let order = BrowseOrder {
        field: SortField::Title,
        direction: SortDirection::Asc,
    };
    let first = index
        .browse_ordered(&ConversationFilter::default(), None, 2, order)
        .unwrap();
    index
        .refresh(
            RefreshBatch {
                conversations: vec![conversation("new", "4")],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    assert_eq!(
        ids(index
            .browse_ordered(
                &ConversationFilter::default(),
                first.next_cursor.as_ref(),
                2,
                order
            )
            .unwrap()),
        vec!["c"]
    );
    assert!(matches!(
        index.browse_ordered(
            &ConversationFilter::default(),
            first.next_cursor.as_ref(),
            2,
            BrowseOrder::default()
        ),
        Err(IndexError::InvalidCursor)
    ));
}

#[test]
fn richer_state_and_profile_semantics_are_stored_without_conflation() {
    let (_dir, mut index) = open();
    index
        .refresh(
            RefreshBatch {
                conversations: vec![conversation("root", "1")],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    index
        .update_semantics(&[ConversationSemantics {
            id: "root".into(),
            state: ConversationState::Missing,
            profile: ProfileQuality::Conflicting,
        }])
        .unwrap();
    let page = index
        .browse(&ConversationFilter::default(), None, 25)
        .unwrap();
    assert_eq!(
        page.semantics["root"],
        (ConversationState::Missing, ProfileQuality::Conflicting)
    );
}

#[test]
fn projection_completion_is_transactional() {
    let (_dir, mut index) = open();
    assert!(index.projection_status().unwrap().needs_reprojection());
    index
        .complete_reprojection(
            REQUIRED_PROJECTION_VERSION,
            RefreshBatch {
                conversations: vec![conversation("root", "1")],
                ..Default::default()
            },
            &[],
            |_| {},
        )
        .unwrap();
    assert_eq!(
        index.projection_status().unwrap().current,
        REQUIRED_PROJECTION_VERSION
    );
    let result = index.complete_reprojection(
        REQUIRED_PROJECTION_VERSION + 1,
        RefreshBatch {
            conversations: vec![conversation("rolled-back-projection", "2")],
            reject_reason: Some("stop".into()),
            ..Default::default()
        },
        &[],
        |_| {},
    );
    assert!(result.is_err());
    assert_eq!(
        index.projection_status().unwrap().current,
        REQUIRED_PROJECTION_VERSION
    );
    assert!(index.profile("rolled-back-projection").unwrap().is_none());
}

#[test]
fn refresh_and_cursor_pagination_keep_a_stable_browse_snapshot() {
    let (_dir, mut index) = open();
    index
        .refresh(
            RefreshBatch {
                conversations: vec![
                    conversation("a", "2026-01-01"),
                    conversation("b", "2026-01-02"),
                    conversation("c", "2026-01-03"),
                ],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let first = index
        .browse(&ConversationFilter::default(), None, 2)
        .unwrap();
    assert_eq!(
        first
            .conversations
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "b"]
    );
    index
        .refresh(
            RefreshBatch {
                conversations: vec![conversation("new-but-old", "2025-12-01")],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let second = index
        .browse(
            &ConversationFilter::default(),
            first.next_cursor.as_ref(),
            2,
        )
        .unwrap();
    assert_eq!(
        second
            .conversations
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a"]
    );
}

#[test]
fn refresh_updating_an_existing_row_does_not_mutate_an_open_snapshot() {
    let (_dir, mut index) = open();
    index
        .refresh(
            RefreshBatch {
                conversations: vec![
                    conversation("a", "1"),
                    conversation("b", "2"),
                    conversation("c", "3"),
                ],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let first = index
        .browse(&ConversationFilter::default(), None, 1)
        .unwrap();
    assert_eq!(ids(first.clone()), vec!["c"]);

    index
        .refresh(
            RefreshBatch {
                conversations: vec![conversation("b", "9")],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let second = index
        .browse(
            &ConversationFilter::default(),
            first.next_cursor.as_ref(),
            2,
        )
        .unwrap();
    assert_eq!(ids(second), vec!["b", "a"]);
}

#[test]
fn browse_reports_snapshot_total_and_rejects_cursor_reused_with_another_filter() {
    let (_dir, mut index) = open();
    index
        .refresh(
            RefreshBatch {
                conversations: vec![conversation("a", "1"), conversation("b", "2")],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let first = index
        .browse(&ConversationFilter::default(), None, 1)
        .unwrap();
    assert_eq!(first.approximate_total, 2);
    let error = index.browse(
        &ConversationFilter {
            query: Some("a".into()),
            ..Default::default()
        },
        first.next_cursor.as_ref(),
        1,
    );
    assert!(matches!(error, Err(IndexError::InvalidCursor)));
}

#[test]
fn filters_metadata_without_indexing_messages() {
    let (_dir, mut index) = open();
    let mut archived = conversation("archived", "2026-01-02");
    archived.archived = true;
    archived.cwd = "/other".into();
    index
        .refresh(
            RefreshBatch {
                conversations: vec![conversation("active", "2026-01-01"), archived],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let page = index
        .browse(
            &ConversationFilter {
                archived: Some(true),
                query: Some("Conversation archived".into()),
                ..Default::default()
            },
            None,
            25,
        )
        .unwrap();
    assert_eq!(page.conversations.len(), 1);
    assert_eq!(page.conversations[0].id, "archived");
}

#[test]
fn search_matches_agent_metadata_outside_conversation_columns() {
    let (_dir, mut index) = open();
    let agent = AgentRecord {
        id: "agent-special".into(),
        root_id: "active".into(),
        parent_id: Some("active".into()),
        agent_path: Some("/root/scout".into()),
        task_name: Some("inspect-parser".into()),
        task_excerpt: Some("sensitive text is not searched".into()),
        role: Some("explorer".into()),
        nickname: Some("Scout".into()),
        model: Some("gpt-special".into()),
        effort: Some("high".into()),
        status: "spawned".into(),
        depth: 1,
        evidence_complete: true,
    };
    index
        .refresh(
            RefreshBatch {
                conversations: vec![conversation("active", "2026-01-01")],
                agents: vec![agent],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    for query in [
        "agent-special",
        "inspect-parser",
        "explorer",
        "Scout",
        "gpt-special",
        "spawned",
    ] {
        let page = index
            .browse(
                &ConversationFilter {
                    query: Some(query.into()),
                    ..Default::default()
                },
                None,
                25,
            )
            .unwrap();
        assert_eq!(
            page.conversations
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["active"],
            "query {query}"
        );
    }
    assert!(index
        .browse(
            &ConversationFilter {
                query: Some("sensitive text".into()),
                ..Default::default()
            },
            None,
            25
        )
        .unwrap()
        .conversations
        .is_empty());
}

#[test]
fn agent_metadata_search_membership_is_stable_for_an_open_snapshot() {
    let (_dir, mut index) = open();
    let make_agent = |id: &str, root_id: &str, task_name: &str| AgentRecord {
        id: id.into(),
        root_id: root_id.into(),
        parent_id: Some(root_id.into()),
        agent_path: Some(format!("/root/{id}")),
        task_name: Some(task_name.into()),
        task_excerpt: None,
        role: Some("explorer".into()),
        nickname: None,
        model: Some("gpt-test".into()),
        effort: Some("high".into()),
        status: "complete".into(),
        depth: 1,
        evidence_complete: true,
    };
    index
        .refresh(
            RefreshBatch {
                conversations: vec![
                    conversation("a", "1"),
                    conversation("b", "2"),
                    conversation("c", "3"),
                ],
                agents: vec![
                    make_agent("agent-a", "a", "needle"),
                    make_agent("agent-b", "b", "needle"),
                    make_agent("agent-c", "c", "needle"),
                ],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let filter = ConversationFilter {
        query: Some("needle".into()),
        ..Default::default()
    };
    let first = index.browse(&filter, None, 1).unwrap();
    assert_eq!(ids(first.clone()), vec!["c"]);
    assert_eq!(first.approximate_total, 3);

    index
        .refresh(
            RefreshBatch {
                agents: vec![
                    make_agent("agent-b", "b", "renamed"),
                    make_agent("agent-new", "c", "needle"),
                ],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();

    let second = index
        .browse(&filter, first.next_cursor.as_ref(), 2)
        .unwrap();
    assert_eq!(ids(second.clone()), vec!["b", "a"]);
    assert_eq!(second.approximate_total, 3);

    let fresh = index.browse(&filter, None, 25).unwrap();
    assert_eq!(ids(fresh.clone()), vec!["c", "a"]);
    assert_eq!(fresh.approximate_total, 2);
}

#[test]
fn source_identity_supports_append_move_and_replacement() {
    let (_dir, mut index) = open();
    let source = SourceRecord {
        logical_id: "thread-1".into(),
        canonical_path: "/active/a.jsonl".into(),
        size: 100,
        modified_ns: 1,
        fingerprint: "prefix".into(),
        safe_offset: 90,
        archived: false,
    };
    index
        .refresh(
            RefreshBatch {
                sources: vec![source.clone()],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let mut appended = source.clone();
    appended.size = 120;
    appended.modified_ns = 2;
    assert_eq!(
        index.source_change(&appended).unwrap(),
        SourceChange::Appended { from_offset: 90 }
    );
    let mut moved = source.clone();
    moved.canonical_path = "/archived/a.jsonl".into();
    moved.archived = true;
    assert_eq!(
        index.source_change(&moved).unwrap(),
        SourceChange::Moved {
            from: "/active/a.jsonl".into()
        }
    );
    let mut replaced = source;
    replaced.size = 10;
    replaced.fingerprint = "other".into();
    assert_eq!(
        index.source_change(&replaced).unwrap(),
        SourceChange::Replaced
    );
}

#[test]
fn complete_discovery_marks_missing_and_prune_is_explicit() {
    let (_dir, mut index) = open();
    let source = SourceRecord {
        logical_id: "gone".into(),
        canonical_path: "/gone".into(),
        size: 1,
        modified_ns: 1,
        fingerprint: "x".into(),
        safe_offset: 1,
        archived: false,
    };
    index
        .refresh(
            RefreshBatch {
                sources: vec![source],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let mut events = vec![];
    index
        .refresh(
            RefreshBatch {
                discovered_all_sources: true,
                ..Default::default()
            },
            |e| events.push(e),
        )
        .unwrap();
    assert!(events.contains(&RefreshEvent::SourceMissing {
        logical_id: "gone".into()
    }));
    assert_eq!(index.prune_missing(i64::MAX).unwrap(), 1);
}

#[test]
fn failed_refresh_rolls_back_the_entire_batch() {
    let (_dir, mut index) = open();
    let result = index.refresh(
        RefreshBatch {
            conversations: vec![conversation("rolled-back", "2026-01-01")],
            reject_reason: Some("fixture failure".into()),
            ..Default::default()
        },
        |_| {},
    );
    assert!(matches!(result, Err(IndexError::RefreshRejected(_))));
    assert!(index
        .browse(&ConversationFilter::default(), None, 25)
        .unwrap()
        .conversations
        .is_empty());
}

#[test]
fn profile_returns_the_complete_agent_tree_with_excerpts_only() {
    let (_dir, mut index) = open();
    let agent = AgentRecord {
        id: "agent-1".into(),
        root_id: "root".into(),
        parent_id: Some("root".into()),
        agent_path: Some("/root/research".into()),
        task_name: Some("research".into()),
        task_excerpt: Some("Inspect the parser".into()),
        role: Some("explorer".into()),
        nickname: None,
        model: Some("gpt-5".into()),
        effort: Some("high".into()),
        status: "spawned".into(),
        depth: 1,
        evidence_complete: true,
    };
    index
        .refresh(
            RefreshBatch {
                conversations: vec![conversation("root", "2026-01-01")],
                agents: vec![agent.clone()],
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
    let profile = index.profile("root").unwrap().unwrap();
    assert_eq!(profile.conversation.title, "Conversation root");
    assert_eq!(profile.agents, vec![agent]);
    assert!(index.profile("missing").unwrap().is_none());
}

#[cfg(unix)]
#[test]
fn index_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, _index) = open();
    assert_eq!(
        std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(dir.path().join("index.sqlite"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
