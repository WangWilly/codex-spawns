# codex-spawns

`codex-spawns` 是一個 Rust 單一執行檔，提供 Interactive TUI 與相容的 command mode，用來分析 Codex root conversations 與完整的 spawned-agent tree。

它會合併三種證據來源：

- 父 session 的 `spawn_agent` / `collaboration.spawn_agent` function call
- 子 session 的 `session_meta.source.subagent.thread_spawn` metadata
- 可選的 `state_*.sqlite` 中 `thread_spawn_edges` table（只讀）

因此即使父 rollout 沒有保存完整的 function-call output，或只剩子 rollout，仍能產生可檢視的紀錄。大型 JSONL 會逐行讀取，不會整個載入記憶體。

## 建置

在這個專案目錄執行：

```bash
cargo build --release
./target/release/codex-spawns --help
```

release profile 使用 LTO、strip 與 size optimization；SQLite bundled 於 binary，不依賴系統 `libsqlite3`。

## Interactive Mode

在 TTY 中無參數執行會立即顯示本機 Profile Index 的舊 snapshot，再以背景 thread 增量 refresh：

```bash
codex-spawns
codex-spawns interactive
```

首頁以 root conversation 為單位，固定顯示標題、ID、cwd、最近活動、agent 數量與最大深度。`Enter` 開啟完整 agent tree；detail 僅在選取時讀取。列表使用 cursor pagination，每批預設 25 筆。背景 refresh 完成後會標示新 snapshot 可用，按 `Enter` 套用，避免瀏覽中的項目跳動。

主要按鍵：`j/k` 或方向鍵移動、`Enter` 開啟或套用更新、`Esc` 返回、`/` 搜尋、`f` 篩選、`r` refresh、`R` 兩次確認 rebuild、`Tab` 切換 pane、`?` 說明、`q` 離開。

## 基本用法

預設掃描 `$CODEX_HOME/sessions` 或 `~/.codex/sessions`，也會掃描存在的 `archived_sessions` 與 `state_*.sqlite`：

```bash
codex-spawns list
codex-spawns sessions
codex-spawns doctor
codex-spawns index status
codex-spawns index refresh
codex-spawns index rebuild
codex-spawns index prune --before 1723420800
```

指定不同的 Codex home 或 rollout 根目錄：

```bash
codex-spawns --codex-home ~/.codex list
codex-spawns --sessions-dir ~/other-codex/sessions list
codex-spawns --sessions-dir ~/other-codex/sessions --sessions-dir ~/.codex/sessions list
codex-spawns --file /path/to/rollout.jsonl list
```

依工作環境、父 session、子 session、模型或角色篩選：

```bash
codex-spawns list --cwd ~/src/my-repo
codex-spawns list --parent 019e...
codex-spawns list --child 019f...
codex-spawns list --model gpt-5.5 --role explorer
codex-spawns list --since 2026-08-01T00:00:00Z --status spawned
```

查看特定紀錄。列表第一欄是可直接傳給 `show` 的 1-based index；也可以使用完整或前綴的 spawn/child thread ID：

```bash
codex-spawns show 1 --include-message --evidence
codex-spawns show 019f... --format json --include-message
```

## 輸出與腳本化

預設是人類可讀的 table。另支援 JSON、JSONL、CSV：

```bash
codex-spawns list --format json > subagents.json
codex-spawns list --format jsonl > subagents.jsonl
codex-spawns list --format csv > subagents.csv
```

為避免把 prompt 或工作內容意外印到終端，列表預設只顯示 `message_excerpt`；需要完整 task message 時才加上 `--include-message`。rollout 可能包含 prompt、程式碼、路徑與其他私人內容。

## 重要欄位

- `parent_thread_id` / `child_thread_id`: 父子 session 關係
- `requested_model` / `requested_effort`: 父 session 的 spawn 參數
- `effective_model` / `effective_effort`: 子 rollout 的 `turn_context` 實際記錄
- `agent_role` / `agent_nickname` / `agent_path`: 子 session metadata
- `fork_turns`: V2 fork 模式；舊版 `fork_context` 會轉成 `all` 或 `none`
- `source`: `rollout`、`child-metadata`、`state-db` 或合併來源
- `state_status`: SQLite edge 的狀態（若有）

`effective_model` 應優先於父 session 當時選取的模型；工具會保留 requested/effective 兩組欄位，方便檢查模型路由是否如預期。

## 開發

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo bench --bench index_query
```

Python 版本暫時保留為相容性 reference implementation：

```bash
python3 -m unittest discover -s tests -v
```

Profile Index 位於 `$CODEX_HOME/cache/codex-spawns/index.sqlite`。它只保存 display metadata 與 excerpt；完整 task message、raw evidence 與 transcript 不建立全文索引。rollout JSONL 與 Codex state database 永遠唯讀；`rebuild`／`prune` 只修改可重建的 Profile Index。
