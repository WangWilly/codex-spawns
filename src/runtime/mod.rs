use crate::cli::{Common, IndexAction};
use codex_spawns::index::{ConversationFilter, IndexOptions, ProfileIndex};
use std::path::PathBuf;

fn index_path(common: &Common) -> PathBuf {
    common
        .codex_home
        .clone()
        .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".codex")
        })
        .join("cache/codex-spawns/index.sqlite")
}

pub fn run_tui(common: &Common) -> Result<(), String> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return Err("interactive mode requires a TTY".into());
    }
    let index = ProfileIndex::open(IndexOptions {
        path: index_path(common),
    })
    .map_err(|e| e.to_string())?;
    let page = index
        .browse(&ConversationFilter::default(), None, 25)
        .map_err(|e| e.to_string())?;
    println!(
        "Interactive profile index ready ({} conversations).",
        page.conversations.len()
    );
    Ok(())
}

pub fn run_index(action: IndexAction, common: &Common) -> Result<(), String> {
    let path = index_path(common);
    match action {
        IndexAction::Status => {
            let index = ProfileIndex::open(IndexOptions { path: path.clone() })
                .map_err(|e| e.to_string())?;
            let page = index
                .browse(&ConversationFilter::default(), None, 1)
                .map_err(|e| e.to_string())?;
            println!(
                "index: {}\nstatus: ready\nhas_conversations: {}",
                path.display(),
                !page.conversations.is_empty()
            );
        }
        IndexAction::Refresh | IndexAction::Rebuild => println!(
            "index refresh is available through interactive mode: {}",
            path.display()
        ),
        IndexAction::Prune { before } => {
            let mut index = ProfileIndex::open(IndexOptions { path }).map_err(|e| e.to_string())?;
            println!(
                "pruned: {}",
                index.prune_missing(before).map_err(|e| e.to_string())?
            );
        }
    }
    Ok(())
}
