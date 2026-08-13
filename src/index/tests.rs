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
