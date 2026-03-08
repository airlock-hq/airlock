# Airlock — Product Specification

## 1) Product Overview

**Airlock** is a local-first Git proxy that transforms messy AI-generated code into clean, reviewable pull requests.

**Core Promise:** _Push messy. Review locally. Ship clean._

### What Airlock Does

When developers push code, Airlock automatically:

- **Cleans** — fixes lint errors, removes dead code, flags security issues
- **Tests** — runs existing tests, generates missing coverage
- **Documents** — adds docstrings, explains complex logic
- **Explains** — generates rich descriptions and interactive Guided Tours

All processing happens locally. Developers review **Push Requests** in a desktop GUI before anything reaches the team.

### The Push Request

A **Push Request** is the core concept in Airlock. When you `git push`, Airlock creates a Push Request — your self-review checkpoint before code becomes a PR for team review.

Think of it as: _"pushing something out of the airlock"_ — you review locally, approve, and release to the world.

Each Push Request contains:

- **Content** — summaries, walkthroughs, diagrams, test evidence (all markdown)
- **Comments** — code review findings anchored to specific lines
- **Patches** — suggested fixes you can apply before pushing

---

## 2) User Experience

### 2.1 Setup Flow

**Time to value: 2 minutes**

```bash
# Install Airlock
brew install --cask airlock  # or winget, apt, direct download

# Initialize in any repo
cd your-project
airlock init

# Push as normal
git push origin feature-branch
```

**What `airlock init` does:**

1. Detects your current `origin` remote
2. Creates a local bare repo as a "gate" (`~/.airlock/repos/<id>.git`)
3. Renames `origin` to `bypass-airlock` (preserved as escape hatch)
4. Sets `origin` to point to the local gate
5. Registers the repo with the Airlock daemon
6. Creates or overwrites `.airlock/workflows/main.yml` with the default pipeline, then applies your selected approval mode (`true`, `if_patches`, or `false`) to the push gate

**Post-init state:**

```
$ git remote -v
origin    ~/.airlock/repos/abc123.git (fetch)
origin    ~/.airlock/repos/abc123.git (push)
bypass-airlock  git@github.com:user/repo.git (fetch)
bypass-airlock  git@github.com:user/repo.git (push)
```

### 2.2 Core Workflow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Developer Workflow                             │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│   1. CODE        2. PUSH           3. REVIEW         4. SHIP            │
│   ─────────      ──────────        ──────────        ──────────         │
│   Write code     git push origin   Desktop app       Approve &          │
│   with AI        (goes to gate)    shows Push        forward to         │
│   assistant                        Request for       GitHub             │
│                                    self-review                          │
│                                                                          │
│   [Cursor]       [Instant]         [Airlock GUI]     [One click]        │
│   [Claude]                                                               │
│   [Copilot]                                                              │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.3 The Transformation

```
INPUT (messy AI code)              OUTPUT (clean, reviewable PR)
─────────────────────              ──────────────────────────────
❌ Lint errors                  →  ✅ All lints pass
❌ No tests                     →  ✅ Tests generated & passing
❌ No docstrings                →  ✅ Functions documented
❌ No PR description            →  ✅ Rich description + Guided Tour
❌ Unused imports               →  ✅ Clean, minimal code
❌ Hardcoded API key            →  ⚠️ Flagged for review
```

---

## 3) Desktop Application

### 3.1 Main Interface

The Airlock desktop app is where developers review their own work before it reaches anyone else.

```
┌─────────────────────────────────────────────────────────────────┐
│  AIRLOCK                                         user/repo      │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Inbox                                                          │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ ⏳ feature/add-auth                                      │   │
│  │    Processing: test • 3 stages complete                  │   │
│  │    [View]                                                │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ ⏸️ feature/preferences                                   │   │
│  │    Ready for review • 2 patches suggested                │   │
│  │    [Review & Approve]                                    │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ ✅ fix/timezone-bug                                      │   │
│  │    Shipped • PR #423 created                             │   │
│  │    [View PR]                                             │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  [Settings]                                                     │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 Push Request View

The Push Request view is where you review and approve changes before they ship.

````
┌─────────────────────────────────────────────────────────────────┐
│  Push Request: feature/add-auth                        [Approve]│
│  3 commits • 5 files changed • Ready for review                 │
├─────────────────────────────────────────────────────────────────┤
│  [Overview] [Critique] [Patches] [Activity]                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ## Summary                                (from describe stage)│
│  Added JWT authentication middleware with refresh token support.│
│  Users can now log in via OAuth and maintain sessions across... │
│                                                                 │
│  ```mermaid                                                     │
│  sequenceDiagram                                                │
│    Client->>API: Request + JWT                                  │
│    API->>Middleware: Validate token                             │
│  ```                                                            │
│                                                                 │
│  ## Code Review                          (from ai-review stage) │
│  ⚠️ 2 warnings found                                            │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ⚠️ src/auth.rs:42 - Token expiry not checked             │  │
│  │ ⚠️ src/auth.rs:78 - Consider rate limiting login         │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ## Test Results                              (from test stage) │
│  ✅ 47 tests passed • 82% coverage                              │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
````

### 3.3 Key Tabs

**Overview** — Unified artifact feed for review:

- Content artifacts (markdown) rendered inline
- Critique comment artifacts with per-comment selection
- Patch artifacts with per-patch selection

**Critique** — Diff viewer with inline comments:

- Standard diff view
- Comments from review stages anchored to specific lines
- Tab badge shows the number of critique comments
- Comments can be toggled for copy/share selection

**Patches** — Suggested fixes from stages:

- Each patch shows title, explanation, and diff preview
- Select which patches to apply
- Apply selected patches as a commit in the run worktree
- Selection is shared with the approval action when the run is awaiting approval

**Activity** — Pipeline execution history:

- Stage-by-stage progress
- Logs and timing information
- Useful for debugging pipeline issues

### 3.4 Notifications

- Desktop notification when Push Request is ready for review
- Badge on dock/taskbar for Push Requests awaiting approval
- Optional: system tray with quick status

---

## 4) Guided Tours

### 4.1 What is a Guided Tour?

A Guided Tour is an interactive, step-by-step walkthrough of a code change. Unlike static PR descriptions, tours guide reviewers through changes in the author's intended order.

Tours are **content artifacts** (markdown) produced by pipeline stages. The GUI renders them with interactive navigation.

### 4.2 Tour Features

**Interactive Navigation:**

- Step through changes in logical order
- Each step shows relevant code, highlighted
- Keyboard shortcuts (←/→, j/k)

**Rich Context:**

- Side-by-side before/after views
- Code blocks with file paths and line numbers
- Expandable "deep dives" for complex sections

**Visual Polish:**

- Smooth transitions between steps
- Syntax highlighting with semantic awareness
- Clean, focused UI

### 4.3 Tour Format

Tours are markdown with a specific structure:

```markdown
---
title: Authentication Walkthrough
---

## Step 1: Add middleware

First, we add the JWT validation middleware.

\`\`\`rust:src/middleware/auth.rs:15-45
// Code is automatically pulled from the diff
\`\`\`

This middleware intercepts all requests and validates the JWT token...

## Step 2: Update routes

Next, we protect the API routes...
```

### 4.4 Tour Sharing (Pro)

Free users can create and view tours locally. Pro users can:

- Share tours via hosted link (`tour.airlock.dev/abc123`)
- Embed interactive tour in GitHub PR description
- View analytics (who viewed, time spent, where they stopped)

---

## 5) Pipeline Stages

When code is pushed, Airlock runs a **stage-based pipeline**. Stages are user-defined shell commands executed sequentially, producing artifacts that form the Push Request.

### 5.1 Stage Overview

Stages are defined in `.airlock/config.yml` and execute in order. Each stage runs in a dedicated worktree and produces **artifacts** that contribute to the Push Request.

**Stage Properties:**

| Property            | Type              | Default     | Description                                                                                            |
| ------------------- | ----------------- | ----------- | ------------------------------------------------------------------------------------------------------ |
| `name`              | string            | —           | Stage identifier                                                                                       |
| `run`               | string            | —           | Shell command to execute                                                                               |
| `shell`             | string            | login shell | Shell to use (sh, bash, zsh). Omit to use `$SHELL -l` for full user environment (API keys, PATH, etc.) |
| `continue_on_error` | boolean           | `false`     | Continue pipeline if stage fails                                                                       |
| `require_approval`  | boolean or string | `false`     | Pause pipeline for user approval (`true`, `false`, or `if_patches`)                                    |

### 5.2 Artifact Types

Stages produce three types of artifacts via CLI helpers:

**Content** (markdown) — Summaries, walkthroughs, diagrams, evidence:

```bash
airlock artifact content --title "Summary" <<EOF
## What Changed
Added JWT authentication middleware...
EOF
```

**Comments** (structured) — Code review findings anchored to file:line:

```bash
airlock artifact comment \
  --file src/auth.rs \
  --line 42 \
  --message "Token expiry not validated"
```

**Patches** (structured) — Suggested code changes with explanation:

```bash
# Make changes to worktree, then capture as patch
eslint --fix .
airlock artifact patch \
  --title "ESLint auto-fixes" \
  --explanation "Applied ESLint auto-fix rules"
```

### 5.3 The Freeze Mechanism

The pipeline has two phases separated by a **freeze point**:

```
┌─────────────────────────────────────────────────────────────────┐
│  PRE-FREEZE (mutable)          │  POST-FREEZE (analysis)       │
├────────────────────────────────┼────────────────────────────────┤
│  • Patches are auto-applied    │  • Code is locked              │
│  • Stages can modify code      │  • Patches queued for review   │
│  • Lint fixes, formatting      │  • Summaries, reviews, tests   │
└────────────────────────────────┴────────────────────────────────┘
```

Stages always produce **explicit patches** via `airlock artifact patch`. The freeze point determines what happens:

- **Pre-freeze:** Patches are applied to the worktree (next stage sees the changes)
- **Post-freeze:** Patches are stored for user review (worktree stays frozen)

This design means stages don't need to know which phase they're in — they just produce artifacts, and the pipeline structure determines behavior.

### 5.4 Reusable Steps

Steps can be loaded from public Git repositories using the `uses:` syntax:

```yaml
- name: describe
  uses: airlock-hq/airlock/defaults/describe@v1
```

Properties from `step.yml` can be overridden inline:

```yaml
- name: test
  uses: airlock-hq/airlock/defaults/test@v1
  continue-on-error: true # Override default from step.yml
  require-approval: true # Add approval step after tests
```

**Version resolution:**

- `@v1` — Latest release with major version 1
- `@v1.2.3` — Exact version
- `@main` — Branch name
- `@abc123def` — Commit SHA

**Default steps** are published in this repo under `defaults/`:

| Step        | Path                 | Description                                                     |
| ----------- | -------------------- | --------------------------------------------------------------- |
| `describe`  | `defaults/describe`  | Generate PR description via AI agent                            |
| `lint`      | `defaults/lint`      | Run linters/formatters, auto-fix issues                         |
| `test`      | `defaults/test`      | Run tests, capture results                                      |
| `critique`  | `defaults/critique`  | Critique diff for bugs, risks, and simplification opportunities |
| `push`      | `defaults/push`      | Push changes to upstream remote                                 |
| `create-pr` | `defaults/create-pr` | Create pull/merge request                                       |

**Step structure:**

```
defaults/describe/
└── step.yml        # Step definition (same schema as inline)
```

**step.yml** uses the same schema as inline steps:

```yaml
run: | # Command to execute
  set -euo pipefail
  echo "hello"
shell: bash # Optional: sh, bash, zsh
description: Generate PR description via AI agent
```

Steps use `airlock artifact` and `airlock exec agent` CLI helpers to produce artifacts.

### 5.5 Core Operations

Some operations require Airlock internals and remain as `airlock exec`:

| Command                  | Description                                       |
| ------------------------ | ------------------------------------------------- |
| `airlock exec freeze`    | Commit pending patches, lock the worktree         |
| `airlock exec push`      | Push changes to upstream                          |
| `airlock exec create-pr` | Create pull request on GitHub                     |
| `airlock exec await`     | Request human approval before continuing pipeline |

### 5.6 Default Workflow

The default workflow uses parallel jobs connected by a DAG. After rebase, critique and test run concurrently. A review gate pauses for human approval if tests fail or critical issues are found. Steps that produce patches use `apply-patch: true` to auto-commit them.

```yaml
name: Main Pipeline

on:
  push:
    branches: ['**']

jobs:
  rebase:
    name: Rebase
    steps:
      - name: rebase
        uses: airlock-hq/airlock/defaults/rebase@main

  critique:
    name: Critique
    needs: rebase
    steps:
      - name: critique
        uses: airlock-hq/airlock/defaults/critique@main

  test:
    name: Test
    needs: rebase
    steps:
      - name: test
        uses: airlock-hq/airlock/defaults/test@main

  gate:
    name: Review Gate
    needs: [critique, test]
    steps:
      - name: review
        run: |
          verdict=$(cat "$AIRLOCK_ARTIFACTS/test_result.json" | airlock exec json verdict)
          severity=$(cat "$AIRLOCK_ARTIFACTS/critique_result.json" | airlock exec json max_severity)
          if [ "$verdict" != "pass" ] || [ "$severity" = "error" ]; then
            echo "Tests failed or critical issues found. Awaiting human review."
            airlock exec await
          fi

  describe:
    name: Describe
    needs: gate
    steps:
      - name: describe
        uses: airlock-hq/airlock/defaults/describe@main

  document:
    name: Document
    needs: gate
    steps:
      - name: document
        uses: airlock-hq/airlock/defaults/document@main
        apply-patch: true

  deploy:
    name: Lint & Push
    needs: [describe, document]
    steps:
      - name: lint
        uses: airlock-hq/airlock/defaults/lint@main
        apply-patch: true
      - name: push
        uses: airlock-hq/airlock/defaults/push@main
      - name: create-pr
        uses: airlock-hq/airlock/defaults/create-pr@main
```

### 5.7 Stage Execution Environment

Each stage runs in a dedicated worktree with these environment variables:

| Variable                | Description                                    |
| ----------------------- | ---------------------------------------------- |
| `$AIRLOCK_RUN_ID`       | Unique run identifier                          |
| `$AIRLOCK_BRANCH`       | Branch being pushed                            |
| `$AIRLOCK_BASE_SHA`     | Base commit SHA                                |
| `$AIRLOCK_HEAD_SHA`     | Head commit SHA (updated after freeze)         |
| `$AIRLOCK_WORKTREE`     | Path to run worktree (also CWD)                |
| `$AIRLOCK_ARTIFACTS`    | Directory for run artifacts (shared by stages) |
| `$AIRLOCK_REPO_ROOT`    | Path to the original working repository        |
| `$AIRLOCK_UPSTREAM_URL` | URL of the upstream remote                     |

**Stage Contract:**

- Exit code 0 = success, non-zero = failure
- Produce artifacts via `airlock artifact` CLI helpers
- Run agent tasks via `airlock exec agent` CLI helper
- Uncommitted worktree changes without a patch are ignored

### 5.8 Status Lifecycle

**Stage Status:** `Pending` → `Running` → `Passed` / `Failed` / `AwaitingApproval`

**Push Request State** (derived from stages):

- **Processing**: Pipeline still running
- **Ready for Review**: Pipeline complete, awaiting user approval
- **Shipped**: Approved and pushed to upstream

### 5.9 Custom Workflow Examples

```yaml
name: Custom CI

on:
  push:
    branches: ['**']

jobs:
  default:
    name: Full Pipeline
    steps:
      # === Pre-freeze: safe auto-fixes ===
      - name: format
        uses: airlock-hq/airlock/defaults/lint@main

      - name: freeze
        run: airlock exec freeze

      # === Post-freeze: analysis ===
      - name: security
        run: npm audit --production
        continue-on-error: true

      - name: ai-review
        uses: someuser/airlock-steps-review@v1 # Third-party step

      - name: describe
        uses: airlock-hq/airlock/defaults/describe@main

      - name: review
        run: 'true'
        require-approval: true

      - name: push
        uses: airlock-hq/airlock/defaults/push@main

      - name: create-pr
        uses: airlock-hq/airlock/defaults/create-pr@main
```

---

## 6) CLI Commands

The CLI is minimal — most interaction happens in the desktop app.

```
airlock - Local Git proxy for AI-assisted development

COMMANDS:
    init        Initialize Airlock in the current repository
    eject       Eject from Airlock (restore original git config)
    status      Quick status check (pending Push Requests, last sync)
    runs        List recent Push Requests for the current repository
    show        Show details for a specific Push Request
    cancel      Cancel a stuck or running pipeline
    exec        Execute a core operation (freeze, push, create-pr, agent)
    artifact    Produce artifacts from within a stage (content, comment, patch)
    doctor      Diagnose common issues
    daemon      Daemon management (start, stop, restart, status, install, uninstall)

ARTIFACT COMMANDS (for use within stages):
    airlock artifact content --title "Title" < content.md
    airlock artifact content --title "Title" --file summary.md
    airlock artifact comment --file path --line N --message "..."
    airlock artifact comment --file findings.json
    airlock artifact patch --title "Title" --explanation "Why"
    airlock artifact patch --title "Title" --explanation "Why" --diff-file fix.diff

AGENT COMMANDS (for use within stages):
    airlock exec agent "Generate a PR description for this diff"
    git diff $AIRLOCK_BASE_SHA $AIRLOCK_HEAD_SHA | airlock exec agent "Summarize"

EXAMPLES:
    airlock init                    # Set up Airlock in current repo
    airlock status                  # Quick check from terminal
    airlock runs                    # List recent Push Requests
    airlock show abc123             # Show details (supports prefix)
    airlock cancel abc123           # Cancel a stuck pipeline
    airlock doctor                  # Troubleshoot issues
    git push bypass-airlock main    # Bypass Airlock (escape hatch)
```

**GUI handles:**

- Reviewing and approving Push Requests
- Viewing diffs with inline comments
- Reviewing and applying patches
- Configuration

---

## 7) Configuration

### 7.1 Per-Repo Configuration

Create `.airlock/config.yml` in repo root:

```yaml
# Agent settings
agent:
  adapter: claude-code # claude-code, codex, gemini, aider

# Pipeline stages
pipeline:
  stages:
    # Pre-freeze: auto-apply fixes
    - name: format
      run: npm run format && airlock artifact patch --title "Formatting" --explanation "Auto-formatted code"
    - name: lint-fix
      run: npm run lint:fix && airlock artifact patch --title "Lint fixes" --explanation "Applied lint auto-fixes"

    # Freeze point
    - name: freeze
      run: airlock exec freeze

    # Post-freeze: analysis
    - name: describe
      run: airlock exec describe
    - name: test
      run: npm test
      continue_on_error: true
    - name: review
      run: 'true'
      require_approval: true
    - name: push
      run: airlock exec push
    - name: create-pr
      run: airlock exec create-pr

# Branch-specific pipeline overrides
branches:
  main:
    pipeline:
      stages:
        - name: freeze
          run: airlock exec freeze
        - name: describe
          run: airlock exec describe
        - name: test
          run: npm test
        - name: review
          run: 'true'
          require_approval: true
        - name: push
          run: airlock exec push
        - name: create-pr
          run: airlock exec create-pr
  feature/*:
    pipeline:
      stages:
        - name: freeze
          run: airlock exec freeze
        - name: test
          run: npm test
          continue_on_error: true
        - name: push
          run: airlock exec push
```

**Minimal config (uses all defaults):**

```yaml
# Empty file or just agent config - uses default pipeline
agent:
  adapter: claude-code
```

### 7.2 Global Configuration

Located at `~/.airlock/config.yml`:

```yaml
# Default agent (can be overridden per-repo)
agent:
  adapter: claude-code

# Sync behavior
sync:
  on_fetch: true

# Storage limits
storage:
  max_artifact_age_days: 30
```

---

## 8) Product Roadmap

### Phase 1 — MVP (Weeks 1-4)

**Goal:** Prove the core loop works.

| Feature                                      | Priority |
| -------------------------------------------- | -------- |
| `airlock init` / `eject`                     | P0       |
| Git proxy (bare repo + hooks)                | P0       |
| Daemon with sync-on-fetch                    | P0       |
| Pipeline with freeze mechanism               | P0       |
| Artifact system (content, comments, patches) | P0       |
| `airlock artifact` CLI helpers               | P0       |
| `airlock exec agent` CLI helper              | P0       |
| Reusable stages (`use:` syntax)              | P0       |
| CLI: status, runs, show                      | P0       |
| Desktop app shell with repo list             | P1       |
| SQLite state management                      | P0       |

### Phase 2 — Core Features (Weeks 4-8)

**Goal:** Full Push Request experience.

| Feature                                   | Priority |
| ----------------------------------------- | -------- |
| Stage executor with custom stages         | P0       |
| Agent adapters (Claude Code, Codex, etc.) | P0       |
| Desktop app: Push Request review UI       | P0       |
| Desktop app: patch review and apply       | P0       |
| Official stages: describe, test, format   | P1       |
| Repo-level config                         | P1       |

### Phase 3 — Polish & Pro (Weeks 8-12)

**Goal:** Production quality + monetization.

| Feature                                 | Priority |
| --------------------------------------- | -------- |
| More official stages (AI review, tours) | P1       |
| Tour sharing (hosted)                   | P1       |
| Tour analytics                          | P2       |
| Airlock Cloud (hosted agents)           | P2       |

### Phase 4 — Team (Weeks 12+)

| Feature                  | Priority |
| ------------------------ | -------- |
| Team dashboard           | P2       |
| Shared configurations    | P2       |
| Tour library             | P3       |
| GitLab/Bitbucket support | P2       |

---

## 9) Design Principles

1. **Local-first** — Everything works offline. No account required for core features.

2. **Zero friction** — One command to set up. Push works the same as before.

3. **Escape hatch always available** — `git push bypass-airlock` bypasses Airlock.

4. **Credentials untouched** — Airlock never copies or stores your SSH keys or tokens.

5. **Transparent** — Users can see exactly what Airlock changed and why.

6. **Configurable** — Sensible defaults, but everything can be customized.

7. **Fast feedback** — Pipeline results in seconds, not minutes.

---

_Last updated: February 2026_
