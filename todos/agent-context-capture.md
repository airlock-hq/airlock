# Agent Context Capture for Airlock

## Context

Most Airlock users write code with AI agents (Claude Code, Codex) then push through Airlock. Today, Airlock sees only the code diff — it has no idea what the user asked the agent to do or why. Capturing this intent and session context would make pipeline stages smarter (e.g., `describe` knows the original request) and give reviewers essential context for understanding AI-generated code.

## Architecture Overview

```
Agent session (Claude Code or Codex)          Airlock
────────────────────────────────────          ───────
  Hook fires (agent-specific)  ───────────> airlock hooks <agent> <event>
                                               │
                                               ▼
                                            SessionCaptureAdapter
                                            (normalize to unified events)
                                               │
                                               ▼
                                            Database (agent_sessions table)
                                            - session state, prompts, files
                                            - per-turn checkpoints (git-anchored)

  git push origin main  ──────────────────> post-receive (existing)
                                               │
                                               ▼
                                            Push-time linking (checkpoint-based):
                                            1. List commits in push range
                                            2. Match checkpoint head_sha to commit parents
                                            3. Verify file overlap per checkpoint
                                            4. Link sessions to run (run_sessions)
                                            5. Write agent_context.json artifact
                                               │
                                       ┌───────┴────────┐
                                       ▼                ▼
                                Pipeline stages      Desktop app
                                (AIRLOCK_AGENT_      "AI Context" tab
                                 CONTEXT env var)    in RunDetail
```

### Session-to-Push Linking (Checkpoint-Based)

We anchor session work to specific points in git history. This eliminates noise from old sessions that happened to touch the same files.

**Core concept: Per-turn checkpoints**

At each `TurnEnd` hook, we record a checkpoint in the DB:

```
checkpoint = {
    session_id,
    head_sha,              -- HEAD when the agent finished this turn
    files_modified,        -- files the agent modified during THIS turn
    prompt,                -- what the user asked
    transcript_entries,    -- filtered conversation for this turn
}
```

The `head_sha` anchors the checkpoint to the git timeline. When the user later commits, their commit's parent will match a checkpoint's `head_sha` — proving that session's work was incorporated into that commit.

**Push-time linking algorithm:**

1. For each commit C in `base_sha..head_sha`:
   - Get C's parent SHA (`C~1`)
   - Get C's changed files: `git diff C~1..C --name-only`
   - Query: `SELECT * FROM session_checkpoints WHERE head_sha = C~1 AND repo_id = ?`
   - For each matching checkpoint: compute `overlap = |checkpoint.files ∩ C.changed_files|`
   - If overlap > 0 → this checkpoint (and its session) contributed to commit C
2. Also check `head_sha IN ancestors(C~1)` for sessions that worked across multiple intermediate commits (e.g., session makes changes, user commits something else, then commits the session's work later)
3. Aggregate: run is linked to all matched sessions with overlap details

**Why this is better than raw file overlap:**

- **Temporally anchored**: Session A editing `main.rs` last week won't match today's push because its checkpoint `head_sha` is far back in history.
- **Non-invasive**: No git commit message modification.
- **Multiple sessions per commit**: Naturally handled — each session has its own checkpoints with different `head_sha` values.
- **Sessions with no commits**: Don't create checkpoints (no TurnEnd with file changes), so they don't link.
- **Partial commits**: If user commits only some files, only the checkpoints with overlapping files are linked.

**Fallback for edge cases:** If no checkpoint `head_sha` matches any commit parent directly, fall back to broader matching: sessions active during the push timeframe + file overlap. This handles rebases, squashes, and other git history rewrites.

## Implementation Plan

### Phase 1: Adapter Trait and Core Types

**New file: `crates/airlock-core/src/agent/capture.rs`**

The `SessionCaptureAdapter` trait abstracts agent-specific hook/transcript behavior. Each agent implements this trait. This is analogous to the existing `AgentAdapter` trait (which handles running agents from pipeline stages) but for observing external sessions.

```rust
/// Trait for capturing context from external AI agent sessions.
/// Each agent CLI (Claude Code, Codex) implements this to normalize
/// its hook events and transcript format into unified types.
pub trait SessionCaptureAdapter: Send + Sync {
    /// Agent identifier (e.g., "claude-code", "codex").
    fn name(&self) -> &str;

    /// Check if this agent CLI is present on the system.
    fn is_available(&self) -> bool;

    /// Install hooks into the agent's project-level config.
    /// Returns the number of hooks installed.
    fn install_hooks(&self, repo_path: &Path) -> Result<u32>;

    /// Remove hooks from the agent's config.
    fn remove_hooks(&self, repo_path: &Path) -> Result<()>;

    /// Check if hooks are already installed.
    fn are_hooks_installed(&self, repo_path: &Path) -> bool;

    /// Parse a hook event from stdin JSON into a normalized CaptureEvent.
    fn parse_hook_event(&self, event_name: &str, stdin: &[u8]) -> Result<CaptureEvent>;

    /// Parse the agent's transcript file and extract session data.
    /// Supports incremental parsing from a byte offset.
    fn parse_transcript(&self, path: &Path, from_offset: u64) -> Result<(TranscriptData, u64)>;

    /// Find the transcript file path for a given session.
    fn find_transcript(&self, session_id: &str, repo_path: &Path) -> Result<Option<PathBuf>>;
}
```

**Normalized event types** (in `capture.rs`):

```rust
/// Normalized lifecycle events from any agent.
pub enum CaptureEvent {
    SessionStart { session_id: String, transcript_path: Option<String> },
    TurnEnd { session_id: String, transcript_path: Option<String> },
    FileChanged { session_id: String, file_path: String, tool: String },
    SessionEnd { session_id: String },
}

/// Data extracted from a transcript (agent-agnostic).
pub struct TranscriptData {
    pub prompts: Vec<CapturedPrompt>,
    pub files_modified: Vec<String>,
    pub tool_summary: Vec<ToolSummary>,
    pub transcript_entries: Vec<TranscriptEntry>,  // filtered: user + assistant text only
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_turns: Option<u32>,
}

pub struct CapturedPrompt { pub text: String, pub timestamp: i64 }
pub struct ToolSummary { pub tool: String, pub count: u32, pub files: Vec<String> }
pub struct TranscriptEntry { pub role: String, pub content: String, pub timestamp: i64 }

/// Full agent context associated with a run (assembled at push time).
pub struct AgentContext {
    pub sessions: Vec<CapturedSessionSummary>,
    pub primary_intent: Option<String>,
    pub captured_at: i64,
}

pub struct CapturedSessionSummary {
    pub session_id: String,
    pub agent: String,
    pub model: Option<String>,
    pub prompts: Vec<CapturedPrompt>,
    pub files_modified: Vec<String>,
    pub tool_summary: Vec<ToolSummary>,
    pub transcript_entries: Vec<TranscriptEntry>,
    pub total_turns: Option<u32>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}
```

**Adapter registry** (in `capture.rs`):

```rust
pub fn create_capture_adapter(name: &str) -> Result<Box<dyn SessionCaptureAdapter>>;
pub fn all_capture_adapters() -> Vec<Box<dyn SessionCaptureAdapter>>;  // for init
```

### Phase 2: Claude Code Capture Adapter

**New file: `crates/airlock-core/src/agent/capture_claude.rs`**

Implements `SessionCaptureAdapter` for Claude Code.

**Hook installation** — merges into `<repo>/.claude/settings.json`:

| Event          | Matcher                     | Command                                   | Purpose                                   |
| -------------- | --------------------------- | ----------------------------------------- | ----------------------------------------- |
| `SessionStart` | —                           | `airlock hooks claude-code session-start` | Register session                          |
| `Stop`         | —                           | `airlock hooks claude-code turn-end`      | Parse transcript delta, create checkpoint |
| `PostToolUse`  | `Write\|Edit\|NotebookEdit` | `airlock hooks claude-code file-changed`  | Track files incrementally                 |
| `SessionEnd`   | —                           | `airlock hooks claude-code session-end`   | Mark ended                                |

Hook entries include an `"_airlock": true` marker field for clean identification/removal.

**Transcript parsing** — Claude Code JSONL at `~/.claude/projects/<sanitized-path>/<session>.jsonl`:

- Each line: `{"type": "user"|"assistant", "uuid": "...", "message": {...}}`
- Extract user prompts (text from content blocks)
- Extract file paths from `tool_use` blocks with name Write/Edit/NotebookEdit
- Extract assistant text (filter out tool inputs/outputs, thinking blocks)
- Track token usage from message metadata
- Incremental: read from byte offset, return new offset

### Phase 3: Codex Capture Adapter

**New file: `crates/airlock-core/src/agent/capture_codex.rs`**

Implements `SessionCaptureAdapter` for Codex.

**Hook installation** — Codex has a notification hook in `<repo>/.codex/config.toml`:

```toml
[hooks]
session-complete = "airlock hooks codex session-complete"
```

Codex's hook system is more limited than Claude Code's. For MVP, we use:

- The notification hook (fires when Codex finishes a task) → `session-complete`
- On `session-complete`, scan `~/.codex/sessions/YYYY/MM/DD/` for the most recently modified `rollout-*.jsonl` file for this repo

Since Codex runs in a more batch-oriented mode (`codex exec`), the primary capture point is when the session completes. We don't need incremental file-changed hooks because Codex sessions are typically shorter/single-task.

**Transcript parsing** — Codex JSONL at `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`:

- Events: `thread.started`, `item.started`, `item.completed`, `item.content_text.delta`, `turn.completed`
- Extract user prompt from the initial prompt (passed as argument)
- Extract file paths from `file_change` and `command_execution` items
- Extract assistant text from `agent_message` items
- Track token usage from `turn.completed` usage fields

### Phase 4: Database Schema (v9 migration)

**Modify: `crates/airlock-core/src/db/schema.rs`**

The database is the **primary store** for all session data. This makes querying, analysis, and UI serving fast.

```sql
-- Tracks agent sessions observed via hooks
CREATE TABLE agent_sessions (
    id TEXT PRIMARY KEY,                    -- Airlock UUID
    external_session_id TEXT NOT NULL,      -- Agent's native session ID
    repo_id TEXT NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    agent TEXT NOT NULL,                    -- "claude-code" | "codex"
    model TEXT,
    status TEXT NOT NULL DEFAULT 'active',  -- "active" | "ended"
    transcript_path TEXT,                   -- Path to agent's transcript file
    transcript_offset INTEGER DEFAULT 0,   -- Byte offset for incremental parsing
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    input_tokens INTEGER,
    output_tokens INTEGER,
    total_turns INTEGER,
    created_at INTEGER NOT NULL
);

-- User prompts captured from agent sessions
CREATE TABLE session_prompts (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    prompt_text TEXT NOT NULL,
    prompt_order INTEGER NOT NULL,          -- Ordering within the session
    captured_at INTEGER NOT NULL
);

-- Per-turn checkpoints — the core linking mechanism.
-- Created at each TurnEnd hook. Anchors session work to a git commit.
CREATE TABLE session_checkpoints (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    head_sha TEXT NOT NULL,                 -- HEAD when this turn ended (git anchor)
    files_modified TEXT NOT NULL,            -- JSON array of files modified in this turn
    prompt_text TEXT,                        -- User's prompt for this turn
    transcript_entries TEXT,                 -- JSON array of filtered {role, content} entries
    input_tokens INTEGER,
    output_tokens INTEGER,
    checkpoint_order INTEGER NOT NULL,       -- Ordering within the session
    created_at INTEGER NOT NULL
);

-- Files modified during agent sessions (incremental, from FileChanged hooks).
-- Used for real-time tracking between TurnEnd checkpoints.
CREATE TABLE session_files (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    tool TEXT NOT NULL,                     -- "Write" | "Edit" | "file_change" | etc.
    captured_at INTEGER NOT NULL,
    UNIQUE(session_id, file_path, tool)     -- Deduplicate
);

-- Links runs (pushes) to the agent sessions that contributed.
-- Populated at push time by matching checkpoints against commits.
CREATE TABLE run_sessions (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    matched_checkpoints TEXT,               -- JSON array of checkpoint IDs that matched
    overlapping_files TEXT,                 -- JSON array of files in both session and commit diffs
    commit_shas TEXT,                       -- JSON array of commits where overlap was found
    PRIMARY KEY (run_id, session_id)
);
```

Indexes:

```sql
CREATE INDEX idx_agent_sessions_repo ON agent_sessions(repo_id);
CREATE INDEX idx_agent_sessions_status ON agent_sessions(status);
CREATE INDEX idx_session_prompts_session ON session_prompts(session_id);
CREATE INDEX idx_session_checkpoints_session ON session_checkpoints(session_id);
CREATE INDEX idx_session_checkpoints_head ON session_checkpoints(head_sha);
CREATE INDEX idx_session_files_session ON session_files(session_id);
CREATE INDEX idx_run_sessions_run ON run_sessions(run_id);
CREATE INDEX idx_run_sessions_session ON run_sessions(session_id);
```

**New file: `crates/airlock-core/src/db/agent_session.rs`** — DB operations:

- `insert_session()`, `update_session_status()`, `update_session_transcript_offset()`
- `insert_prompt()`, `insert_file()`
- `insert_checkpoint()` — creates a new per-turn checkpoint row
- `get_active_sessions(repo_id)`, `get_recent_sessions(repo_id, since)`
- `get_checkpoints_by_head_sha(head_sha)` — key query for push-time linking
- `get_checkpoints_for_session(session_id)`
- `link_session_to_run()`, `get_sessions_for_run(run_id)`
- `get_prompts_for_session()`, `get_files_for_session()`

### Phase 5: Hook Command Handlers

**New file: `crates/airlock-cli/src/commands/hooks/mod.rs`** — dispatcher
**New file: `crates/airlock-cli/src/commands/hooks/handler.rs`** — agent-agnostic handler logic

CLI command: `airlock hooks <agent> <event>`

All hook handlers must:

- Complete in <100ms (they block the agent)
- Never write to stdout (breaks agent protocol)
- Never exit non-zero (breaks the agent)
- Wrap all operations in catch-all error handling (log errors, exit 0)

**Handler flow** (agent-agnostic, uses `SessionCaptureAdapter`):

1. Read stdin JSON
2. `adapter.parse_hook_event(event_name, stdin)` → `CaptureEvent`
3. Dispatch based on event type:

**`CaptureEvent::SessionStart`**:

- Look up repo_id from cwd via database
- Insert row into `agent_sessions` table (status = "active")

**`CaptureEvent::FileChanged`**:

- Insert/upsert row into `session_files` table

**`CaptureEvent::TurnEnd`**:

- Get session from DB, read transcript_offset
- Get current HEAD: `git rev-parse HEAD` (the git anchor for this checkpoint)
- `adapter.parse_transcript(path, offset)` → `TranscriptData`
- Insert new prompts into `session_prompts`
- Insert new file changes into `session_files`
- **Create checkpoint**: insert into `session_checkpoints` with `head_sha`, files modified this turn, prompt, filtered transcript entries
- Update `agent_sessions.transcript_offset`, token counts, total_turns

**`CaptureEvent::SessionEnd`**:

- Update `agent_sessions.status = "ended"`, set `ended_at`

Register in `crates/airlock-cli/src/commands/mod.rs` and `crates/airlock-cli/src/main.rs`.

### Phase 6: Hook Installation Integration

**Modify: `crates/airlock-core/src/init.rs`** — in `init_repo()`, after existing setup:

```rust
// Install agent capture hooks for all available agents
for adapter in all_capture_adapters() {
    if adapter.is_available() {
        match adapter.install_hooks(working_path) {
            Ok(n) => info!("Installed {} {} hooks", n, adapter.name()),
            Err(e) => warn!("Could not install {} hooks: {}", adapter.name(), e),
        }
    }
}
```

**Modify: `crates/airlock-cli/src/commands/eject.rs`** — in eject:

```rust
for adapter in all_capture_adapters() {
    let _ = adapter.remove_hooks(working_path);  // best-effort
}
```

Never fail init or eject due to hook installation/removal errors.

### Phase 7: Push-Time Capture

**New file: `crates/airlock-daemon/src/handlers/agent_context.rs`**

When daemon processes a push (called from `handle_push_received`):

1. **List commits in push range** — `git rev-list base_sha..head_sha` → list of commit SHAs.
2. **For each commit C**:
   a. Get C's parent: `C~1`
   b. Get C's changed files: `git diff C~1..C --name-only`
   c. Query DB: `SELECT * FROM session_checkpoints WHERE head_sha = C_parent` (checkpoints anchored to this commit's parent = session was working on top of it)
   d. For each matching checkpoint: compute file overlap with C's changed files
   e. If overlap > 0 → checkpoint contributed to this commit
3. **Broader ancestry check** — for commits where no checkpoint matches the exact parent, check if any checkpoint's `head_sha` is an ancestor of `C~1` within the push range. This handles cases where the user made non-agent commits between the session work and the push.
4. **Freshen active sessions** — if a matched session is still active, do one final incremental transcript parse.
5. **Insert `run_sessions` links** in DB — with matched checkpoint IDs, overlapping files, and commit SHAs.
6. **Build `AgentContext`** from linked session data (prompts, files, transcript from checkpoints) and write `agent_context.json` to `~/.airlock/artifacts/<repo_id>/<run_id>/`
7. **Fallback** — if no checkpoints match any commits (e.g., hooks were just installed), fall back to: active sessions for this repo + file overlap with the full push diff. This gracefully degrades for the initial setup period.

**Modify: `crates/airlock-daemon/src/handlers/push.rs`** — call agent context capture between run creation and pipeline execution.

**Modify: `crates/airlock-daemon/src/pipeline/executor.rs`** — add `AIRLOCK_AGENT_CONTEXT` env var pointing to `agent_context.json` file path. Pipeline stages can read this for intent-aware operations.

### Phase 8: IPC and Frontend

**Modify daemon IPC** to include agent context in run detail responses:

- Extend `GetRunDetailResult` with `agent_sessions: Vec<RunAgentSession>`
- Query from `run_sessions` + `agent_sessions` + `session_prompts` tables

```rust
pub struct RunAgentSession {
    pub session_id: String,
    pub agent: String,
    pub model: Option<String>,
    pub prompts: Vec<PromptEntry>,
    pub files_modified: Vec<FileEntry>,
    pub transcript: Vec<TranscriptEntryDTO>,
    pub total_turns: Option<u32>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}
```

**New frontend component: `crates/airlock-app/src/components/push-request/AIContextTab.tsx`**

"AI Context" tab in RunDetail showing:

- **Primary intent** banner at top (most recent prompt from highest-relevance session)
- **Session cards** for each contributing session:
  - Agent name + model badge (e.g., "Claude Code / Opus")
  - Time range (started → ended)
  - User prompts (expandable list)
  - Files modified (cross-referenced with push diff — highlight files in both)
  - Filtered transcript viewer (collapsible)
- **Empty state**: "No AI agent context detected. Agent context is captured automatically when you use Claude Code or Codex."

**Modify RunDetail page** — add tab alongside existing tabs (Overview, Changes, Patches, Activity).

**Overview tab enhancement**: when agent context exists, show subtle banner:
`"AI-Assisted | Claude Code / Opus | 'Add error handling...'"` linking to the AI Context tab.

## Files Summary

### New Files

| File                                                              | Purpose                                                           |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `crates/airlock-core/src/agent/capture.rs`                        | `SessionCaptureAdapter` trait, normalized types, adapter registry |
| `crates/airlock-core/src/agent/capture_claude.rs`                 | Claude Code capture adapter (hooks + JSONL parser)                |
| `crates/airlock-core/src/agent/capture_codex.rs`                  | Codex capture adapter (hooks + JSONL parser)                      |
| `crates/airlock-core/src/db/agent_session.rs`                     | DB operations for session tables                                  |
| `crates/airlock-cli/src/commands/hooks/mod.rs`                    | `airlock hooks` command dispatcher                                |
| `crates/airlock-cli/src/commands/hooks/handler.rs`                | Agent-agnostic hook event handler                                 |
| `crates/airlock-daemon/src/handlers/agent_context.rs`             | Push-time session capture and linking                             |
| `crates/airlock-app/src/components/push-request/AIContextTab.tsx` | UI tab component                                                  |

### Files to Modify

| File                                             | Change                                                                                                    |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| `crates/airlock-core/src/agent/mod.rs`           | Register `capture`, `capture_claude`, `capture_codex` submodules                                          |
| `crates/airlock-core/src/init.rs`                | Call `install_hooks()` for all available capture adapters                                                 |
| `crates/airlock-cli/src/commands/eject.rs`       | Call `remove_hooks()` for all capture adapters                                                            |
| `crates/airlock-cli/src/commands/mod.rs`         | Register `hooks` command module                                                                           |
| `crates/airlock-cli/src/main.rs`                 | Add `Hooks` variant to CLI commands enum                                                                  |
| `crates/airlock-core/src/db/schema.rs`           | v9 migration: `agent_sessions`, `session_prompts`, `session_checkpoints`, `session_files`, `run_sessions` |
| `crates/airlock-core/src/db/mod.rs`              | Register migration, add `agent_session` module                                                            |
| `crates/airlock-daemon/src/handlers/push.rs`     | Call agent context capture between run creation and pipeline                                              |
| `crates/airlock-daemon/src/pipeline/executor.rs` | Add `AIRLOCK_AGENT_CONTEXT` env var                                                                       |
| RunDetail page (frontend)                        | Add "AI Context" tab                                                                                      |

## Verification Plan

1. **Unit tests**: Both transcript parsers with sample JSONL fixtures, hook install/remove for both agents, CaptureEvent parsing, DB operations, checkpoint-based linking
2. **Integration test**: Full round-trip — install hooks, simulate hook events via `airlock hooks <agent> <event>` CLI, verify DB records (sessions, checkpoints), simulate push, verify run_sessions linking and agent_context.json output
3. **Manual E2E with Claude Code**: `airlock init` in test repo → use Claude Code to make changes → push → verify AI Context tab shows prompts and files
4. **Manual E2E with Codex**: Same flow with Codex
5. **Edge cases**: No active sessions (empty context), multiple concurrent sessions, session still active at push time, hook command failures (must not break agent), very large transcripts (incremental parsing), rebased history (fallback linking)
