use codex_spawns::index::{
    ConversationFilter, ConversationRecord, IndexOptions, ProfileIndex, RefreshBatch,
};
use codex_spawns::{ProfileFact, TokenUsageSummary};
use std::time::{Duration, Instant};

/// A dependency-free release-gate smoke benchmark. Run with
/// `cargo bench --bench index_query`; the deliberately generous bound catches
/// accidental full scans while remaining stable on shared CI machines.
fn main() {
    let directory = tempfile::tempdir().unwrap();
    let mut index = ProfileIndex::open(IndexOptions {
        path: directory.path().join("index.sqlite"),
    })
    .unwrap();
    let conversations = (0..10_000)
        .map(|number| ConversationRecord {
            id: format!("conversation-{number:05}"),
            title: format!("Conversation {number}"),
            title_source: "fixture".into(),
            cwd: "/work".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            last_activity_at: format!("2026-01-01T00:{:02}:{:02}Z", number / 60 % 60, number % 60),
            archived: false,
            model: Some("gpt-5".into()),
            status: Some("complete".into()),
            agent_count: 2,
            max_depth: 1,
            profile_complete: true,
            project: ProfileFact::unknown(),
            tokens: TokenUsageSummary::default(),
        })
        .collect();
    index
        .refresh(
            RefreshBatch {
                conversations,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();

    let started = Instant::now();
    let page = index
        .browse(&ConversationFilter::default(), None, 25)
        .unwrap();
    assert_eq!(page.conversations.len(), 25);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_millis(100));
    println!("first page (10,000 conversations): {elapsed:?}");
}
