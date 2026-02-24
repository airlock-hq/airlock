# Airlock — Technical Specification

## 1) Overview

Airlock is a local-first Git proxy that transforms messy AI-generated code into clean, reviewable pull requests.

**Core Design Principle:** Use standard Git mechanisms (bare repos, hooks) rather than custom protocols. Credentials work automatically because everything runs as the same OS user.

**Core Concept:** When you push code, Airlock creates a **Push Request** — your self-review checkpoint before code becomes a PR. Push Requests contain content (summaries, walkthroughs), comments (anchored to code), and patches (suggested fixes).

---

## 2) Architecture

### 2.1 Component Overview

```
┌──────────────────────────────────────────────────────────────────────────┐
│                              User's Machine                              │
├──────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐        ┌───────────────────────────────────────────┐   │
│  │  Working     │ push   │            Airlock System                 │   │
│  │  Repository  │───────▶│  ┌─────────────────────────────────────┐  │   │
│  │              │        │  │  Local Bare Repo (Gate)             │  │   │
│  │  origin ────────────────▶│  ~/.airlock/repos/<id>.git          │  │   │
│  │  upstream ───────────────│──────────────────────────────────┐  │  │   │
│  └──────────────┘        │  │  - pre-receive hook              │  │  │   │
│         │                │  │  - post-receive hook             │  │  │   │
│         │ fetch          │  │  - origin remote → GitHub        │  │  │   │
│         ▼                │  └──────────────┬───────────────────┘  │  │
│  ┌──────────────┐        │                 │                       │    │
│  │  Airlock CLI │◀───────│─────────────────┼───────────────────────│    │
│  │  (airlock)   │        │                 ▼                       │    │
│  └──────────────┘        │  ┌────────────────────────────────────┐ │    │
│         │                │  │  Airlock Daemon (airlockd)         │ │    │
│         │ IPC            │  │  - Manages bare repos              │ │    │
│         ▼                │  │  - Runs transformation pipeline    │ │    │
│  ┌──────────────┐        │  │  - Syncs with upstream             │ │    │
│  │  Desktop App │◀──────▶│  │  - Serves IPC API                  │ │    │
│  │  (Tauri)     │  IPC   │  └──────────────┬─────────────────────┘ │    │
│  └──────────────┘        │                 │                       │    │
│                          │                 │ git push upstream     │    │
│                          └─────────────────┼───────────────────────┘    │
│                                            ▼                            │
│                                   ┌──────────────┐                      │
│                                   │   Upstream   │                      │
│                                   │   (GitHub)   │                      │
│                                   └──────────────┘                      │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Components

| Component   | Language             | Purpose                                                    |
| ----------- | -------------------- | ---------------------------------------------------------- |
| `airlock`   | Rust                 | CLI for user commands (`init`, `status`, `artifact`, etc.) |
| `airlockd`  | Rust                 | Background daemon managing repos, pipelines, and sync      |
| Desktop App | Tauri (Rust + React) | GUI for reviewing Push Requests and approving changes      |
| Git Hooks   | Shell                | Scripts in bare repos that trigger daemon actions          |

### 2.3 Directory Structure

```
~/.airlock/
├── config.yml               # Global configuration
├── state.sqlite             # State database (repos, runs, stage_results)
├── repos/                   # Local bare repos (gates)
│   └── <repo-id>.git/
│       └── hooks/           # pre-receive, post-receive
├── worktrees/               # Run worktrees
│   └── <repo-id>/<run-id>/  # One worktree per run
├── artifacts/               # Generated artifacts per run
│   └── <repo-id>/<run-id>/
│       ├── logs/            # Per-stage log files
│       │   ├── describe/
│       │   │   ├── stdout.log
│       │   │   └── stderr.log
│       │   └── test/
│       │       ├── stdout.log
│       │       └── stderr.log
│       ├── content/         # Markdown content artifacts
│       │   ├── summary.md
│       │   ├── walkthrough.md
│       │   └── test-results.md
│       ├── comments/        # Code review comments (JSON)
│       │   └── ai-review.json
│       └── patches/         # Suggested code changes (JSON)
│           ├── lint-fixes.json
│           └── security-fix.json
├── locks/                   # Per-repo file locks
└── socket                   # Unix domain socket for IPC
```

---

## 3) Git Proxy Mechanism

### 3.1 Why a Local Bare Repo?

We considered several approaches for intercepting Git operations:

| Approach                      | Pros                                | Cons                                          |
| ----------------------------- | ----------------------------------- | --------------------------------------------- |
| **Git hooks in working repo** | Simple                              | Only intercepts commits, not pushes to remote |
| **Custom Git remote helper**  | Full control                        | Complex, credential handling is painful       |
| **Local bare repo (chosen)**  | Standard Git, credentials just work | Extra disk space (negligible for bare repos)  |

The bare repo approach wins because:

- `git push origin` works exactly as users expect
- SSH keys, GPG signing, credential helpers all work unchanged
- We can use standard Git hooks (`pre-receive`, `post-receive`)
- Fetches from upstream are transparent

### 3.2 Initialization Flow (`airlock init`)

When a user runs `airlock init` in their working repository:

1. Read current `origin` URL (e.g., `git@github.com:user/repo.git`)
2. Generate repo ID from hash of origin URL + working path
3. Create bare repo at `~/.airlock/repos/<id>.git`
4. Add `origin` remote to bare repo pointing to original origin (GitHub)
5. Rewire working repo: rename `origin` → `upstream`, add new `origin` → bare repo
6. Install `pre-receive` and `post-receive` hooks in bare repo
7. Trigger initial sync from origin
8. Record repo in SQLite state database

**Post-init remote layout:**

- Working repo: `origin` → local bare repo, `upstream` → GitHub (escape hatch)
- Bare repo (gate): `origin` → GitHub

### 3.3 Fetch Path

When the user runs `git fetch` or `git pull`:

1. Git fetches from local bare repo (instant, no network latency)
2. Hook notifies daemon of fetch request
3. Daemon checks if bare repo is stale (>5 seconds since last sync)
4. If stale, daemon fetches from upstream before completing the local fetch

**Rationale:** Users expect `git pull` to get fresh data. By syncing on fetch, we maintain that expectation while still routing through our gate.

### 3.4 Push Path (The Gate)

When the user runs `git push origin <branch>`:

1. Push lands in local bare repo instantly
2. `pre-receive` hook captures ref updates and notifies daemon
3. Hook exits 0 (soft gate — we accept the push locally)
4. `post-receive` hook triggers the transformation pipeline asynchronously
5. User continues working while pipeline runs
6. Desktop app shows notification when review is ready
7. User reviews changes and approves/rejects

**Why soft gate?** A hard gate (rejecting the push) would block the user's workflow. By accepting locally and holding for review, we let them keep working while we process the changes.

---

## 4) Daemon (`airlockd`)

### 4.1 Responsibilities

1. **Repo Management** — Create/delete bare repos, manage remotes
2. **Sync** — Keep bare repos in sync with upstream
3. **Pipeline Execution** — Run transformation stages on pushes
4. **State Management** — Track runs, stage results, artifacts in SQLite
5. **IPC Server** — Handle requests from CLI and desktop app
6. **Forwarding** — Push reviewed changes to upstream

### 4.2 IPC Design

Communication via local IPC using JSON-RPC 2.0:

- **Unix:** Domain socket at `~/.airlock/socket`
- **Windows:** Named pipe `airlock-daemon`

**Key operations:**

- `Init`, `Eject` — Repo enrollment
- `Sync`, `SyncAll` — Upstream synchronization
- `GetRuns`, `GetRunDetail` — Push Request queries
- `GetRunArtifacts` — Get content, comments, patches for a run
- `ApproveStage`, `RejectStage` — Stage approval actions
- `ApplyPatches` — Apply selected patches and re-run or ship
- `GetRunDiff` — Get diff for a run
- `Status`, `Health` — System status

**Why local IPC (Unix socket / named pipe)?** Fast, secure (OS-level access control), no network exposure, works offline.

### 4.3 Lifecycle Management

The daemon runs as a user-level service:

- **macOS:** launchd (`~/Library/LaunchAgents/dev.airlock.daemon.plist`)
- **Linux:** systemd user service (`~/.config/systemd/user/airlockd.service`)

Auto-starts on login, restarts on crash.

**Service Setup Strategy:**

1. **Package installers (preferred):** Homebrew, apt, and other package managers should install and enable the service as a post-install step. This ensures the daemon is always available after installation.

2. **Manual installation:** Users can run `airlock daemon install` to set up the service files, then `airlock daemon start` to enable it.

3. **Fallback auto-start:** If the daemon is not running when `airlock init` is executed, the CLI will attempt to start it automatically. This ensures a working setup even if the package installer didn't configure the service.

**Package Post-Install Scripts:**

```bash
# macOS (Homebrew post-install)
launchctl load -w ~/Library/LaunchAgents/dev.airlock.daemon.plist

# Linux (apt post-install)
systemctl --user daemon-reload
systemctl --user enable --now airlockd.service
```

### 4.4 Windows Support

**Status:** Partial support (IPC and paths ready, hooks and lifecycle deferred)

| Component        | Windows Status | Notes                                      |
| ---------------- | -------------- | ------------------------------------------ |
| IPC              | Ready          | Named pipes via `interprocess` crate       |
| File paths       | Ready          | Cross-platform path handling               |
| Git hooks        | Deferred       | Requires PowerShell/batch scripts          |
| Daemon lifecycle | Deferred       | Requires Windows Service or Task Scheduler |
| File permissions | Deferred       | Unix socket 0700 → Windows ACLs            |

**Why deferred?** Git hooks and daemon lifecycle require significant Windows-specific implementation (shell script translation, service registration). These are planned for a future release.

**Workaround:** Windows users can run Airlock in WSL2, where the Unix implementation works fully.

---

## 5) Transformation Pipeline

### 5.1 Pipeline Engine

The pipeline executes user-defined stages sequentially, producing a **Push Request**. Each run gets a single worktree where all stages execute.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Stage-Based Pipeline                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  1. CREATE WORKTREE                                                  │   │
│  │     git worktree add ~/.airlock/worktrees/<repo>/<run> <head_sha>   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  2. PRE-FREEZE STAGES (patches auto-applied)                         │   │
│  │     ┌────────────┐   ┌────────────┐                                  │   │
│  │     │   format   │ → │  lint-fix  │ → ...                            │   │
│  │     └────────────┘   └────────────┘                                  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  3. FREEZE                                                           │   │
│  │     Commit applied patches, lock worktree                            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  4. POST-FREEZE STAGES (patches queued for review)                   │   │
│  │     ┌────────────┐   ┌────────────┐   ┌────────────┐   ┌──────────┐ │   │
│  │     │  describe  │ → │    test    │ → │   review   │ → │   push   │ │   │
│  │     └────────────┘   └────────────┘   └────────────┘   └──────────┘ │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                              │                                              │
│                              ▼                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  5. CLEANUP                                                          │   │
│  │     Remove worktree (unless keep_worktrees=true)                     │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.2 Artifact Types

Stages produce three types of artifacts that form the Push Request:

**Content** (markdown) — Summaries, walkthroughs, diagrams, evidence:

```
$AIRLOCK_ARTIFACTS/content/*.md
```

Each content file can have YAML frontmatter:

```markdown
---
title: Summary
---

## What Changed

Added JWT authentication middleware...
```

**Comments** (structured JSON) — Code review findings anchored to file:line:

```
$AIRLOCK_ARTIFACTS/comments/*.json
```

Schema:

```json
{
  "comments": [
    {
      "file": "src/auth.rs",
      "line": 42,
      "message": "Token expiry not validated",
      "severity": "warning"
    }
  ]
}
```

**Patches** (structured JSON) — Suggested code changes with explanation:

```
$AIRLOCK_ARTIFACTS/patches/*.json
```

Schema:

```json
{
  "title": "Fix null pointer dereference",
  "explanation": "The user variable could be null if authentication fails. Added null check.",
  "diff": "--- a/src/auth.rs\n+++ b/src/auth.rs\n@@ -42,6 +42,9 @@\n..."
}
```

### 5.3 The Freeze Mechanism

The pipeline has two phases separated by a **freeze point**:

| Phase       | Patch Behavior                   | Worktree State |
| ----------- | -------------------------------- | -------------- |
| Pre-freeze  | Patches auto-applied to worktree | Mutable        |
| Post-freeze | Patches queued for user review   | Locked         |

**Key design principle:** Stages always produce **explicit patches** via `airlock artifact patch`. Uncommitted worktree changes without a corresponding patch are ignored. This means:

- Stages don't need to know which phase they're in
- Same stage can work in either phase
- Pipeline structure determines behavior

**What `airlock exec freeze` does:**

1. Collect all patches produced by pre-freeze stages
2. Apply them to the worktree (if not already applied)
3. Commit with message: "Airlock: auto-fixes from format, lint-fix, ..."
4. Update `$AIRLOCK_HEAD_SHA` to the new commit
5. Mark the worktree as frozen
6. Post-freeze stages that produce patches → patches stored for review

### 5.4 Stage Definition

Each stage is defined with these properties:

```yaml
- name: string # Stage identifier (required)
  run: string # Shell command to execute (required)
  shell: string # Shell to use: sh, bash, zsh (default: user's login shell via $SHELL -l)
  continue_on_error: bool # Continue if stage fails (default: false)
  require_approval: bool # Pause for user approval (default: false)
```

**Stage Contract:**

- Command runs with CWD set to worktree
- Exit code 0 = success, non-zero = failure
- Produce artifacts via `airlock artifact` CLI helpers
- Uncommitted worktree changes without a patch are ignored

### 5.5 Stage Execution Environment

Each stage receives these environment variables:

| Variable               | Description                                        |
| ---------------------- | -------------------------------------------------- |
| `AIRLOCK_RUN_ID`       | Unique run identifier (UUID)                       |
| `AIRLOCK_BRANCH`       | Branch being pushed (e.g., `feature/add-auth`)     |
| `AIRLOCK_BASE_SHA`     | Base commit SHA (before push)                      |
| `AIRLOCK_HEAD_SHA`     | Head commit SHA (updated after freeze)             |
| `AIRLOCK_WORKTREE`     | Absolute path to run worktree (also CWD)           |
| `AIRLOCK_ARTIFACTS`    | Directory for run artifacts (shared by all stages) |
| `AIRLOCK_REPO_ROOT`    | Path to the original working repository            |
| `AIRLOCK_UPSTREAM_URL` | URL of the upstream remote                         |
| `AIRLOCK_FROZEN`       | "true" if worktree is frozen, "false" otherwise    |

### 5.6 CLI Artifact Helpers

Stages produce artifacts via CLI helpers:

**Content:**

```bash
# From stdin
airlock artifact content --title "Summary" <<EOF
## What Changed
Added authentication middleware...
EOF

# From file
airlock artifact content --title "Test Results" --file results.md
```

**Comments:**

```bash
# Single comment
airlock artifact comment \
  --file src/auth.rs \
  --line 42 \
  --message "Token expiry not validated" \
  --severity warning

# From JSON file
airlock artifact comment --file findings.json
```

**Patches:**

```bash
# Capture uncommitted worktree changes (default if no --diff-file)
eslint --fix .
airlock artifact patch \
  --title "ESLint auto-fixes" \
  --explanation "Applied ESLint auto-fix rules"

# From explicit diff file
airlock artifact patch \
  --title "Security fix" \
  --explanation "Added input validation" \
  --diff-file fix.diff
```

When `airlock artifact patch` captures worktree changes:

1. Diff uncommitted changes
2. Create patch artifact
3. Revert worktree to clean state
4. (Pre-freeze) Airlock then re-applies the patch

### 5.7 Reusable Steps

Steps can be loaded from public Git repositories using the `uses:` syntax:

```yaml
- name: describe
  uses: airlock-hq/airlock/defaults/describe@v1
```

**Property overrides:** Inline properties override values from `step.yml`:

```yaml
- name: test
  uses: airlock-hq/airlock/defaults/test@v1
  continue-on-error: true # Override default from step.yml
  require-approval: true # Add approval step after tests
```

Resolution order: inline properties > step.yml defaults

**Version resolution:**

| Syntax       | Meaning                             |
| ------------ | ----------------------------------- |
| `@v1`        | Latest release with major version 1 |
| `@v1.2.3`    | Exact version                       |
| `@main`      | Branch name                         |
| `@abc123def` | Commit SHA                          |

**Step fetching:**

1. Parse `uses:` field (e.g., `airlock-hq/airlock/defaults/describe@v1`)
2. **Bundled defaults fast-path:** If the reference matches a first-party default (`airlock-hq/airlock/defaults/*@main`), use the YAML embedded in the binary via `include_str!()`. This skips all cache and network I/O, ensuring defaults are always up-to-date with the installed binary version.
3. Check cache: `~/.airlock/actions/<owner>/<repo>/<path>@<version>/`
4. **TTL check for mutable refs:** If the cached ref is mutable (branch name like `@main`, or semver-major like `@v1`), check if the cache is older than 1 hour. Stale caches are removed and re-fetched. Immutable refs (`@v1.2.3` exact semver, `@abc123def` commit SHA) are cached indefinitely. If stale cache removal fails, the stale copy is used as a graceful fallback.
5. If not cached, fetch from GitHub (sparse checkout or raw content)
6. Run the step's `run` command with standard environment variables

**Default steps** are bundled into the binary and also published in this repo under `defaults/`:

```
defaults/
├── describe/
│   └── step.yml        # Step definition
├── lint/
│   └── step.yml
├── test/
│   └── step.yml
├── push/
│   └── step.yml
└── create-pr/
    └── step.yml
```

**step.yml schema** (same as inline step definition):

```yaml
# Required
run: | # Command to execute (inline script)
  set -euo pipefail
  echo "hello"

# Optional (same as inline steps)
shell: bash # Shell to use: sh, bash, zsh (default: user's login shell via $SHELL -l)
continue-on-error: false # Continue pipeline if step fails
require-approval: false # Pause pipeline for user approval

# Optional metadata
description: Generate PR description via AI agent
```

This means a `uses:` step and an inline `run:` step have the same properties. The only difference is where the definition lives.

### 5.8 Core Operations

Some operations require Airlock internals and remain as `airlock exec`:

| Command                  | Description                                |
| ------------------------ | ------------------------------------------ |
| `airlock exec freeze`    | Commit patches, lock worktree (see 5.3)    |
| `airlock exec push`      | Push current branch to upstream            |
| `airlock exec create-pr` | Create pull request on GitHub via `gh` CLI |

### 5.9 Agent CLI Helper

Stages can run agent tasks via CLI:

```bash
# Simple prompt
airlock exec agent "Generate a PR description for this diff"

# With stdin context
git diff $AIRLOCK_BASE_SHA $AIRLOCK_HEAD_SHA | airlock exec agent "Summarize this diff"
```

This uses the agent adapter configured in `~/.airlock/config.yml` or `.airlock/config.yml`. Stages are agent-agnostic — they use `airlock exec agent` and the user chooses which agent CLI to use.

### 5.10 Default Workflow

The `airlock init` command creates a `.airlock/workflows/main.yml` file with this default workflow:

```yaml
name: Main Pipeline

on:
  push:
    branches: ['**']

jobs:
  default:
    name: Lint, Test & Deploy
    steps:
      - name: lint
        uses: airlock-hq/airlock/defaults/lint@main
      - name: freeze
        run: airlock exec freeze
      - name: describe
        uses: airlock-hq/airlock/defaults/describe@main
      - name: test
        uses: airlock-hq/airlock/defaults/test@main
        continue-on-error: true
      - name: review
        run: 'true'
        require-approval: true
      - name: push
        uses: airlock-hq/airlock/defaults/push@main
      - name: create-pr
        uses: airlock-hq/airlock/defaults/create-pr@main
```

**Important:** Workflow files in `.airlock/workflows/` are required for pipeline execution. If no workflows exist or cannot be parsed, the pipeline will fail with an error directing the user to run `airlock init`.

### 5.11 Status Lifecycles

**Stage Status:**

```
Pending → Running → Passed
              ↓        ↓
           Failed   AwaitingApproval → Passed
                                    ↓
                                  Failed (rejected)
```

| StageStatus        | Description                                       |
| ------------------ | ------------------------------------------------- |
| `Pending`          | Stage has not started yet                         |
| `Running`          | Stage is currently executing                      |
| `Passed`           | Stage completed successfully                      |
| `Failed`           | Stage failed (or was rejected during approval)    |
| `Skipped`          | Stage was skipped                                 |
| `AwaitingApproval` | Stage has `require_approval=true` and awaits user |

**Push Request State (derived from stages):**

| Derived State    | Condition                                        |
| ---------------- | ------------------------------------------------ |
| Processing       | Any stage is `Pending` or `Running`              |
| Ready for Review | Any stage is `AwaitingApproval`                  |
| Shipped          | All stages `Passed` or `Skipped`, push completed |
| Failed           | Any stage is `Failed`                            |

Helper methods on `Run`:

- `run.is_processing()` — pipeline still executing
- `run.is_ready_for_review()` — waiting for user approval
- `run.is_shipped()` — completed and pushed to upstream
- `run.is_failed()` — completed with at least one failed stage

**State Transitions:**

1. Push received → first stage `Running`
2. Stage completes → Stage `Passed`, next stage `Running`
3. Stage with `require_approval=true` completes → Stage `AwaitingApproval`
4. User approves → Stage `Passed`, next stage `Running`
5. User approves with patches → patches applied, new commit, re-run pipeline
6. User rejects → Stage `Failed`
7. All stages done → Push Request `Shipped` or `Failed`

### 5.12 Pipeline Concurrency

**Rapid Push Handling:**

- Multiple pushes in quick succession are coalesced (debounced)
- New pushes supersede pending runs for the same branch
- Avoid creating duplicate Push Requests for the same ref updates

**Execution Concurrency:**

- Limit concurrent pipeline executions globally (e.g., max 2-4)
- Per-repo serialization to avoid conflicts
- Support cancellation of in-progress pipelines when superseded

**Resource Management:**

- Timeouts for each stage (configurable, default 5 minutes)
- Resource limits for spawned processes
- Agent calls have their own timeout handling

---

## 6) Desktop App

### 6.1 Technology Choice: Tauri

We chose Tauri over Electron because:

- Much smaller binary size (~10MB vs ~150MB)
- Uses native webview (no bundled Chromium)
- Rust backend integrates naturally with our crates
- Better memory usage

### 6.2 Architecture

- **Frontend:** React + Tailwind + Shadcn UI
- **Backend:** Rust, communicates with daemon via IPC
- **Views:** Repo list, Push Request list, Push Request detail, settings

The app is a thin client — all state lives in the daemon's SQLite database.

### 6.3 Push Request View

The main view where users review and approve changes:

**Overview Tab:**

- All content artifacts rendered as markdown sections
- Comments summarized with severity counts
- Quick stats (files changed, tests passed)

**Changes Tab:**

- Standard diff view
- Inline comments from review stages anchored to specific lines
- Click comment to jump to location

**Patches Tab:**

- List of suggested patches from post-freeze stages
- Each shows title, explanation, diff preview
- Checkboxes to select which patches to apply
- Actions: "Apply & Re-run" or "Apply & Ship"

**Activity Tab:**

- Stage execution timeline
- Logs and timing information
- Useful for debugging pipeline issues

### 6.4 Patch Handling

When user reviews patches:

1. **Approve as-is** — Push the frozen code without suggested patches
2. **Apply & Re-run** — Select patches, apply as new commit, trigger fresh pipeline run
3. **Apply & Ship** — Apply patches and push immediately (shows staleness warning)

Option 2 is recommended because analysis artifacts will be regenerated for the new code state.

---

## 7) State Management

### 7.1 Data Model

**Entities:**

- **Repo** — Enrolled repository (working path, upstream URL, gate path, sync timestamp)
- **Run** — Single pipeline execution (branch, base/head SHA, current_stage, timestamps, error). Status is derived from stages.
- **StageResult** — Result of a single stage execution (name, status, duration, artifacts, error)
- **SyncLog** — Record of upstream syncs

**Schema:**

```sql
CREATE TABLE repos (
    id TEXT PRIMARY KEY,
    working_path TEXT NOT NULL,
    upstream_url TEXT NOT NULL,
    gate_path TEXT NOT NULL,
    last_sync_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    repo_id TEXT NOT NULL REFERENCES repos(id),
    branch TEXT NOT NULL,
    base_sha TEXT NOT NULL,
    head_sha TEXT NOT NULL,
    current_stage TEXT,    -- name of currently executing stage (for display)
    error TEXT,            -- pipeline-level error (e.g., worktree creation failed)
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
-- Note: Run status is derived from stage_results, not stored

CREATE TABLE stage_results (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(id),
    name TEXT NOT NULL,
    status TEXT NOT NULL,  -- pending, running, passed, failed, skipped, awaiting_approval
    stage_order INTEGER,   -- order in pipeline (0-indexed)
    exit_code INTEGER,
    duration_ms INTEGER,
    error TEXT,
    started_at INTEGER,
    completed_at INTEGER
);
-- Note: Artifacts are stored at run level (~/.airlock/artifacts/<repo-id>/<run-id>/)
```

**Storage split:**

- SQLite for relational data (repos, runs, stage_results, sync log)
- Filesystem for large artifacts (descriptions, tours, logs) to avoid bloating the database

### 7.2 Artifact Structure

Artifacts are organized by type, enabling the GUI to render Push Requests consistently:

```
~/.airlock/artifacts/<repo-id>/<run-id>/
├── logs/                    # Per-stage log files
│   ├── describe/
│   │   ├── stdout.log
│   │   └── stderr.log
│   ├── test/
│   │   ├── stdout.log
│   │   └── stderr.log
│   └── <stage-name>/
│       ├── stdout.log
│       └── stderr.log
├── content/                 # Markdown content (summaries, tours, evidence)
│   ├── summary.md           # From describe stage
│   ├── walkthrough.md       # From tour stage
│   └── test-results.md      # From test stage
├── comments/                # Code review comments (JSON)
│   ├── ai-review.json       # From AI review stage
│   └── security-scan.json   # From security stage
├── patches/                 # Suggested code changes (JSON)
│   ├── lint-fixes.json      # From lint stage (post-freeze)
│   └── security-fix.json    # From AI review stage
└── pr_result.json           # From create-pr stage
```

**Content files** use markdown with optional YAML frontmatter for title.

**Comment files** contain arrays of findings with file, line, message, severity.

**Patch files** contain title, explanation, and diff string.

---

## 8) Agent Adapters

### 8.1 Design Philosophy

Stages should be **agent-agnostic**. They use `airlock exec agent` to run AI tasks, and users choose which agent CLI to use. This means:

- Stages work with any supported agent
- Users leverage their existing subscriptions/setups
- No vendor lock-in

### 8.2 Supported Agent CLIs

Airlock adapts to these agent CLIs:

| Agent CLI    | Command    | Auth                     |
| ------------ | ---------- | ------------------------ |
| Claude Code  | `claude`   | Subscription (logged in) |
| OpenAI Codex | `codex`    | API key or subscription  |
| Gemini CLI   | `gemini`   | Google account           |
| Open Code    | `opencode` | Various API keys         |

### 8.3 How `airlock exec agent` Works

When a stage calls `airlock exec agent "prompt"`:

1. Read configured agent adapter from `~/.airlock/config.yml`
2. Spawn the agent CLI with appropriate flags
3. Pass prompt (and stdin if provided) to the agent
4. Capture and return output

```bash
# Stage code (agent-agnostic)
description=$(git diff $AIRLOCK_BASE_SHA $AIRLOCK_HEAD_SHA | airlock exec agent "Generate a PR description")
airlock artifact content --title "Summary" <<< "$description"
```

### 8.4 Configuration

```yaml
# ~/.airlock/config.yml
agent:
  # Which agent CLI to use
  adapter: claude-code # claude-code, codex, gemini, aider

  # Adapter-specific options (optional)
  options:
    model: sonnet # For adapters that support model selection
```

**Adapter selection:**

1. Use `adapter` from config (default: `claude-code`)
2. Fall back to first available agent CLI on PATH

### 8.5 Adapter Implementation

Each adapter translates `airlock exec agent` to the appropriate CLI invocation:

**Claude Code adapter:**

```bash
echo "$prompt" | claude --print
```

**Codex adapter:**

```bash
echo "$prompt" | codex --quiet
```

### 8.6 Security

- Stages only see the agent's output, not credentials
- Agent CLIs handle their own authentication
- Sensitive files (`.env*`, `*.key`, `**/secrets/**`) should not be passed to agents

---

## 9) Security Model

### 9.1 Trust Boundaries

Everything runs as the same OS user on the local machine. There's a single trust zone.

### 9.2 Key Security Properties

1. **Credential passthrough** — Daemon uses existing git credential helpers. No keys are ever copied or stored by Airlock.

2. **Local-only IPC** — Unix domain socket (Unix) or named pipe (Windows). No network exposure. Protected by file permissions (Unix: 0700) or ACLs (Windows).

3. **Secret redaction** — Logs and artifacts are scanned for secrets before storage.

4. **Sandboxed commands** — Test/lint commands run with timeouts and resource limits.

5. **Agent data minimization** — Stages control what data is sent to agents, sensitive files excluded.

---

## 10) CLI Commands

Minimal CLI — most interaction happens in the desktop app.

| Command            | Purpose                                                   |
| ------------------ | --------------------------------------------------------- |
| `airlock`          | No arguments → launch desktop GUI                         |
| `airlock init`     | Initialize Airlock in current repository                  |
| `airlock eject`    | Eject from Airlock (restore original git config)          |
| `airlock status`   | Quick status check (pending Push Requests, last sync)     |
| `airlock runs`     | List recent Push Requests for the current repository      |
| `airlock show`     | Show details for a specific Push Request (prefix match)   |
| `airlock cancel`   | Cancel a stuck or running pipeline                        |
| `airlock exec`     | Execute core operation (freeze, push, create-pr, agent)   |
| `airlock artifact` | Produce artifacts from within a stage (see below)         |
| `airlock doctor`   | Diagnose common issues                                    |
| `airlock daemon`   | Daemon management (start, stop, restart, status, install) |

**Artifact subcommands** (for use within stages):

| Command                    | Purpose                                       |
| -------------------------- | --------------------------------------------- |
| `airlock artifact content` | Write markdown content artifact               |
| `airlock artifact comment` | Write code review comment artifact            |
| `airlock artifact patch`   | Write patch artifact (captures worktree diff) |

**Exec subcommands:**

| Command                  | Purpose                                      |
| ------------------------ | -------------------------------------------- |
| `airlock exec freeze`    | Commit patches, lock worktree                |
| `airlock exec push`      | Push to upstream                             |
| `airlock exec create-pr` | Create pull request on GitHub                |
| `airlock exec agent`     | Run agent task with prompt (stdin supported) |

**Bypass:** Users can always `git push upstream main` to skip Airlock entirely.

### 10.1 CLI as GUI Launcher

When `airlock` is invoked without arguments, it spawns the desktop app (`airlock-app`) as a detached process and exits immediately. This provides a unified entry point while keeping the binaries separate.

**Benefits:**

- CLI stays lightweight (~2-3MB) for headless/CI environments
- GUI can be packaged as a proper native app bundle (`.app` on macOS)
- Users only need to remember one command
- CLI-only installation remains possible for servers

**GUI Location Resolution:**

The CLI locates the GUI binary using the following precedence:

1. `AIRLOCK_APP_PATH` environment variable (explicit override)
2. Same directory as the CLI binary
3. Platform-specific install paths:
   - macOS: `/Applications/Airlock.app/Contents/MacOS/airlock-app`
   - Linux: `/usr/bin/airlock-app` or `~/.local/bin/airlock-app`

**Graceful Degradation:**

If the GUI binary is not found, the CLI prints a helpful message:

```
Desktop app not found. Install it from https://airlock.dev/download
or run 'airlock --help' for CLI commands.
```

---

## 11) Configuration

### 11.1 Global Config (`~/.airlock/config.yml`)

- Daemon settings (socket path, log level)
- Default agent adapter
- Sync settings (on-fetch, background interval)
- Storage limits (artifact age, count)

### 11.2 Repo Config (`.airlock/config.yml` in repo root)

- Agent adapter override for this repo
- Pipeline stages definition
- Branch → pipeline mapping

---

## 12) MVP Scope

### Phase 1: Core Mechanism

| Feature                                      | Priority |
| -------------------------------------------- | -------- |
| `airlock init` / `eject`                     | P0       |
| Bare repo creation + hooks                   | P0       |
| Daemon with sync-on-fetch                    | P0       |
| Push interception (soft gate)                | P0       |
| Basic pipeline with freeze                   | P0       |
| Artifact system (content, comments, patches) | P0       |
| CLI status commands                          | P0       |
| `airlock artifact` CLI helpers               | P0       |
| `airlock exec agent` CLI helper              | P0       |
| Reusable stages (`use:` syntax)              | P0       |
| SQLite state management                      | P0       |
| Tauri app shell                              | P1       |

### Phase 2: AI Features

| Feature                                   | Priority |
| ----------------------------------------- | -------- |
| Stage executor with custom stages         | P0       |
| Agent adapters (Claude Code, Codex, etc.) | P0       |
| Desktop app: Push Request review UI       | P0       |
| Desktop app: patch review and apply       | P0       |
| Official stages: describe, test, format   | P1       |
| Repo-level config                         | P1       |

### Future

- More official stages (AI code review, tour generation)
- Tour sharing (hosted)
- Team features
- GitLab/Bitbucket support

---

## 13) Crate Structure

```
airlock/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── airlock-cli/              # CLI binary
│   ├── airlock-daemon/           # Daemon binary
│   │   └── src/pipeline/         # Stage executor
│   ├── airlock-core/             # Shared library
│   │   └── src/agents/           # Agent adapters
│   └── airlock-app/              # Tauri desktop app
│       └── ui/                   # React frontend
├── defaults/                     # Default reusable steps
│   ├── describe/
│   │   └── step.yml
│   ├── lint/
│   │   └── step.yml
│   ├── test/
│   │   └── step.yml
│   ├── push/
│   │   └── step.yml
│   └── create-pr/
│       └── step.yml
└── hooks/                        # Git hook scripts
```

---

## 14) Key Dependencies

**Rust:**

- `tokio` — Async runtime
- `git2` — libgit2 bindings for Git operations
- `rusqlite` — SQLite access
- `interprocess` — Unix domain sockets
- `clap` — CLI parsing
- `tauri` — Desktop app framework

**Agent CLIs** (user-installed, not bundled):

- Claude Code CLI (`claude`)
- OpenAI Codex CLI (`codex`)
- Gemini CLI (`gemini`)
- Aider (`aider`)

**Frontend:**

- React 18, React Router
- Tailwind CSS, Shadcn UI
- `@tauri-apps/api`
- `prism-react-renderer` for code highlighting

---

# Implementation Plan

## TODO Task 1: Define unified types (`types.rs`)

### Description

Create `crates/airlock-core/src/agent/types.rs` with all the shared types that both adapters and consumers depend on:

- `AgentRequest` — unified request struct (prompt, context, cwd, output_schema, model, resume_session, max_turns)
- `AgentResult` — final collected result (content, session_id, usage, messages)
- `AgentUsage` — token/timing metrics (input_tokens, output_tokens, duration_ms, duration_api_ms, num_turns, raw)
- `AgentMessage` — session history enum (User, Assistant, ToolRoundtrip)
- `ContentBlock` — content block enum (Text, Thinking, ToolUse, ToolResult)
- `AgentEvent` — streaming event enum (SessionStart, TextDelta, AssistantMessage, ToolUse, ToolResult, StructuredOutput, Usage, Complete, Error)
- `AgentEventStream` — type alias for `Pin<Box<dyn Stream<Item = Result<AgentEvent>> + Send>>`

All types must derive `Debug, Clone, Serialize, Deserialize` where appropriate. `AgentEventStream` is just a type alias.

### Acceptance criteria

- All types compile and are well-documented
- Types are serializable to JSON (test round-trip serialization for each type)
- Unit tests verify construction of each type variant and serde round-tripping

### Notes

## TODO Task 2: Define `AgentAdapter` trait and registry (`mod.rs`)

### Description

Refactor `crates/airlock-core/src/agent/mod.rs` to:

1. Define the `AgentAdapter` async trait with methods: `name() -> &str`, `is_available() -> bool`, `run(&self, request: &AgentRequest) -> Result<AgentEventStream>`
2. Implement `create_adapter(name: &str) -> Result<Box<dyn AgentAdapter>>` registry function that matches on "claude-code"/"claude", "codex", and "auto"
3. Implement `detect_available_adapter()` that checks CLI availability in priority order (claude → codex)
4. Re-export all public types from `types.rs` and the trait

Remove the old public API items (`run_agent`, `run_agent_with_context`, `is_available`, `AgentConfig`, `AgentResponse`) from the module exports. These will be replaced once the adapters are implemented.

### Acceptance criteria

- `AgentAdapter` trait compiles and is object-safe (`Box<dyn AgentAdapter>`)
- `create_adapter("claude-code")`, `create_adapter("codex")`, `create_adapter("auto")` return the correct adapter types
- `create_adapter("unknown")` returns an error
- Unit tests for registry function

### Notes

## TODO Task 3: Implement `StreamCollector` (`stream.rs`)

### Description

Create `crates/airlock-core/src/agent/stream.rs` with a `StreamCollector` utility that consumes an `AgentEventStream` and produces an `AgentResult`:

1. Concatenate `TextDelta` events into the final `content` string
2. Assemble `AgentMessage` entries from `AssistantMessage`, `ToolUse`/`ToolResult` event pairs
3. Capture `Usage` and `Complete` events for the final `AgentUsage` and `session_id`
4. Handle `StructuredOutput` events — if present, use the structured data as the content (JSON-serialized)
5. Handle `Error` events — fatal errors should cause the collector to return an error

Provide a public async function: `pub async fn collect_stream(stream: AgentEventStream) -> Result<AgentResult>`

### Acceptance criteria

- Collector correctly assembles `AgentResult` from a synthetic stream of `AgentEvent` values
- `TextDelta` events are concatenated in order
- `ToolUse` + `ToolResult` pairs are matched into `AgentMessage::ToolRoundtrip`
- `StructuredOutput` overrides content when present
- Fatal `Error` events cause the function to return `Err`
- Unit tests with synthetic streams covering all event types

### Notes

## TODO Task 4: Implement subprocess utilities (`subprocess.rs`)

### Description

Create `crates/airlock-core/src/agent/subprocess.rs` with shared utilities for spawning agent CLI processes and reading JSONL output:

1. `SpawnConfig` struct — command, args, cwd, env vars
2. `spawn_agent(config: SpawnConfig) -> Result<(Child, BufReader<ChildStdout>)>` — spawns a `tokio::process::Command` with stdout piped
3. `jsonl_stream<T, F>(reader: BufReader<ChildStdout>, map_fn: F) -> AgentEventStream` — reads JSONL lines, parses each as `serde_json::Value`, applies the mapping function `F(Value) -> Option<AgentEvent>`, and yields through an async stream
4. Handle process exit — when the subprocess exits, emit a final `Complete` event if one hasn't been emitted, or an `Error` event if the exit code is non-zero

This module is used by the Codex adapter. The Claude Code adapter uses the SDK's native streaming instead.

### Acceptance criteria

- `spawn_agent` correctly spawns a child process with piped stdout
- `jsonl_stream` correctly reads JSONL lines and maps them to events
- Non-zero exit codes produce `Error` events
- Unit tests with mock/synthetic data (no real CLI needed)

### Notes

## TODO Task 5: Refactor Claude Code adapter (`claude_code.rs`)

### Description

Refactor `crates/airlock-core/src/agent/claude_code.rs` to implement the `AgentAdapter` trait:

1. Create `ClaudeCodeAdapter` struct (stateless, `new()` constructor)
2. Implement `AgentAdapter::name()` → `"Claude Code"`
3. Implement `AgentAdapter::is_available()` → check if `claude` CLI is on PATH (reuse existing logic)
4. Implement `AgentAdapter::run()`:
   - Convert `AgentRequest` fields to `ClaudeAgentOptions` (cwd, output_format, model, resume session)
   - Call `query_stream()` from the SDK instead of `query()` (upgrade to streaming)
   - Map each SDK `Message` type to `AgentEvent`:
     - `Message::System` → `SessionStart`
     - `Message::StreamEvent` with text → `TextDelta`
     - `Message::Assistant` with tool_use → `ToolUse`
     - `Message::Assistant` with tool_result → `ToolResult`
     - `Message::Assistant` with text → `AssistantMessage`
     - `Message::Result` → `Complete` (extract usage from `ResultMessage`)
   - Return the stream as `AgentEventStream`
5. Extract token usage from `ResultMessage` fields: `usage`, `duration_ms`, `duration_api_ms`, `num_turns`, `total_cost_usd`
6. Preserve the JSON extraction helpers (`try_extract_json`, `extract_response_content`) — these are still useful for the `StructuredOutput` mapping

Remove the old public functions (`run_agent`, `run_agent_with_context`, `is_available`, `AgentConfig`, `AgentResponse`). All existing unit tests should be migrated to test the new adapter.

### Acceptance criteria

- `ClaudeCodeAdapter` implements `AgentAdapter` trait
- `is_available()` correctly detects `claude` CLI
- `run()` returns an `AgentEventStream` that yields correct `AgentEvent` variants
- Token usage is extracted from `ResultMessage`
- All existing unit tests pass (migrated to new types)
- Integration tests (ignored) updated for new API

### Notes

## TODO Task 6: Implement Codex adapter (`codex.rs`)

### Description

Create `crates/airlock-core/src/agent/codex.rs` implementing the `AgentAdapter` trait for OpenAI Codex CLI:

1. Create `CodexAdapter` struct (stateless, `new()` constructor)
2. Implement `AgentAdapter::name()` → `"Codex"`
3. Implement `AgentAdapter::is_available()` → check if `codex` CLI is on PATH
4. Implement `AgentAdapter::run()`:
   - Build command: `codex exec --json -C <cwd> --dangerously-bypass-approvals-and-sandbox "<prompt>"`
   - Add `-m <model>` if model is specified
   - Add `--output-schema <path>` if output_schema is provided (write schema to temp file)
   - Use `subprocess.rs` utilities to spawn and read JSONL
   - Map Codex JSONL events to `AgentEvent`:
     - `item.content_text.delta` → `TextDelta`
     - `item.tool_use` / `item.function_call` → `ToolUse`
     - `item.tool_result` / `item.function_call_output` → `ToolResult`
     - `turn.completed` → `AssistantMessage`
     - End of stream → `Complete` (wall-clock duration, turn count, no token data)
5. Handle context from stdin: if `AgentRequest.context` is set, prepend it to the prompt or pipe via stdin
6. Measure wall-clock `duration_ms` since Codex doesn't report it

### Acceptance criteria

- `CodexAdapter` implements `AgentAdapter` trait
- `is_available()` correctly detects `codex` CLI
- `run()` constructs the correct command line arguments
- JSONL events are mapped to correct `AgentEvent` variants
- Structured output schema is written to temp file and passed via `--output-schema`
- Unit tests for command construction and event mapping (using synthetic JSONL data, no real CLI needed)

### Notes

## TODO Task 7: Add agent config to `GlobalConfig`

### Description

Extend `crates/airlock-core/src/config/global.rs`:

1. Add `AgentGlobalConfig` struct with `adapter: String` (default: "claude-code") and `options: AgentOptions` (model, max_turns)
2. Add `agent: AgentGlobalConfig` field to `GlobalConfig` with `#[serde(default)]`
3. Implement `Default` for `AgentGlobalConfig` that sets adapter to "claude-code"

Also add `AIRLOCK_AGENT_ADAPTER` to `StageEnvironment::to_env_vars()` in `crates/airlock-daemon/src/pipeline/executor.rs` so pipeline stages can use the configured adapter.

### Acceptance criteria

- `GlobalConfig` deserializes correctly with and without the `agent` section
- Default adapter is "claude-code" when not specified
- `AIRLOCK_AGENT_ADAPTER` env var is set in stage environment
- Existing config files continue to work (backward compatible)
- Unit tests for config deserialization with various inputs

### Notes

## TODO Task 8: Migrate CLI handler (`exec agent` command)

### Description

Rewrite `crates/airlock-cli/src/commands/exec/agent.rs` and update `crates/airlock-cli/src/main.rs`:

1. Add `--adapter` flag to `ExecAction::Agent` variant
2. Update the `agent()` handler to:
   - Load `GlobalConfig` to get default adapter
   - Check `--adapter` CLI flag override, then `AIRLOCK_AGENT_ADAPTER` env var, then config default
   - Create adapter via `create_adapter(name)`
   - Build `AgentRequest` from prompt, stdin context, schema, config options
   - Call `adapter.run()` to get `AgentEventStream`
   - Consume the stream: write each `AgentEvent` as JSONL to **stderr** (real-time streaming)
   - Simultaneously feed events to `StreamCollector` to assemble `AgentResult`
   - After stream completes, write final output to **stdout** (structured JSON if schema was provided, else text content)
   - If `AIRLOCK_ARTIFACTS` env var is set, write session history JSONL to `$AIRLOCK_ARTIFACTS/agent_session.jsonl`
3. Update `crates/airlock-core/src/lib.rs` — replace old re-exports with new public API: `AgentAdapter`, `AgentRequest`, `AgentResult`, `AgentEvent`, `AgentEventStream`, `AgentUsage`, `AgentMessage`, `ContentBlock`, `create_adapter`, `collect_stream`

### Acceptance criteria

- `airlock exec agent "prompt"` works with the default adapter (claude-code)
- `airlock exec agent "prompt" --adapter codex` uses the Codex adapter
- `AIRLOCK_AGENT_ADAPTER=codex airlock exec agent "prompt"` uses the Codex adapter
- Streaming events appear on stderr as JSONL
- Final output appears on stdout only
- `--output-schema` produces structured JSON on stdout
- Stdin piping still works for context
- Session history is written to artifacts dir when in pipeline context
- Existing CLI tests updated and passing

### Notes

## TODO Task 9: Update `mod.rs` re-exports and clean up old API

### Description

Final cleanup pass across the codebase:

1. Verify `crates/airlock-core/src/agent/mod.rs` exports the complete new public API
2. Verify `crates/airlock-core/src/lib.rs` re-exports are updated
3. Remove any remaining references to the old API (`run_agent`, `run_agent_with_context`, `AgentConfig`, `AgentResponse`, `is_agent_available`)
4. Search the entire codebase for any stale references
5. Ensure `cargo build` and `cargo test` pass with no warnings
6. Run `cargo clippy` and `make check` — fix any issues

### Acceptance criteria

- No references to old API anywhere in the codebase
- `cargo build` succeeds with no warnings
- `cargo test` passes all tests (unit + integration where applicable)
- `cargo clippy` passes
- `make check` passes (includes frontend lint)

### Notes

## TODO Task 10: Write integration tests

### Description

Create comprehensive tests in `crates/airlock-core/src/agent/tests.rs`:

1. **Unit tests for each adapter's event mapping** — feed synthetic JSONL data through the mapping logic and verify correct `AgentEvent` output
2. **Unit tests for `StreamCollector`** — synthetic event streams producing expected `AgentResult`
3. **Unit tests for adapter registry** — `create_adapter` with valid/invalid names
4. **Integration tests (marked `#[ignore]`)** for each adapter — require the actual CLI to be installed:
   - Claude Code: simple prompt, structured output, context piping
   - Codex: simple prompt, structured output, context piping
5. **CLI integration tests** — verify the stdout/stderr split behavior:
   - `airlock exec agent "prompt"` produces text on stdout, JSONL on stderr
   - `airlock exec agent "prompt" --output-schema schema.json` produces JSON on stdout
   - `airlock exec agent "prompt" --adapter codex` uses the right adapter

### Acceptance criteria

- All unit tests pass in CI (no external CLIs needed)
- Integration tests pass when the respective CLI is installed (marked `#[ignore]` for CI)
- Tests cover: adapter selection, event mapping, stream collection, CLI output behavior, config loading
- At least one test per `AgentEvent` variant and `AgentMessage` variant

### Notes

---

_Last updated: February 2026_
