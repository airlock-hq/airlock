# Airlock

[![CI](https://github.com/airlock-hq/airlock/actions/workflows/ci.yml/badge.svg)](https://github.com/airlock-hq/airlock/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/airlock-hq/airlock?filter=airlock-*&label=release)](https://github.com/airlock-hq/airlock/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform: macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)]()
[![Website](https://img.shields.io/badge/web-airlockhq.com-black.svg)](https://airlockhq.com)
[![X](https://img.shields.io/badge/follow-@Airlock__HQ-black.svg?logo=x)](https://x.com/Airlock_HQ)

All slop must die. Airlock is where every git push turns into a slop-free PR.

Airlock is a local Git proxy that intercepts `git push`, runs your code through a customizable validation pipeline, and lets you approve before anything is pushed to remote. Think of it as an airlock between your local repo and the outside world.

```mermaid
flowchart LR
  subgraph before[" Before "]
    direction TB
    A["Local Repo"] -- "git push" --> B["Remote Branch"]
    B -- "dirty pull request" --> C["CI + Code Review"]
    C -- "fail / request changes" --> A
  end
  subgraph after[" After "]
    direction TB
    D["Local Repo"] -- "git push" --> E["Airlock"]
    E -- "lint, test, review, fix" --> F["Remote Branch"]
    F -- "clean pull request" --> G["Merge"]
  end
  before ~~~ after
```

## Install

```bash
brew install --cask airlock-hq/airlock/airlock
```

macOS only for now. More platforms coming soon.

Documentation: [https://airlockhq.com/docs](https://airlockhq.com/docs).

## Quick Start

```bash
cd your-project
airlock init      # sets up local git gate
git push origin feature-branch   # triggers the pipeline
```

That's it. Airlock intercepts the push, runs your pipeline, and opens a **Push Request** in the desktop app for self-review. When you approve, it forwards to GitHub and creates a PR.

![Airlock Push Request — Overview with architecture diagram](assets/screenshot1-overview-diagram.png)

During `airlock init`, you'll choose how strict the push gate should be:

- `Always` (`require-approval: true`)
- `Only when there are patches` (`require-approval: if_patches`)
- `Never` (`require-approval: false`)

`airlock init` creates or overwrites `.airlock/workflows/main.yml` with the default workflow, then applies your selected approval mode.

To bypass Airlock at any time: `git push bypass-airlock main`

## What It Does

Your agents are writing a ton of code, fast. How do you review and merge the code at the same pace, with confidence?

Airlock handles the basic review, validation and clean up, so you can focus on more important decisions.

| Before            | After                         |
| ----------------- | ----------------------------- |
| Lint errors       | All lints pass                |
| No tests          | Tests generated & passing     |
| No docs           | Functions documented          |
| No PR description | Rich summary with walkthrough |
| Hardcoded secrets | Flagged for review            |

![Rebase with merge conflict auto-resolved](assets/screenshot2-overview-rebase.png)
_The rebase step detected a merge conflict in `src/routes/api.ts` and resolved it automatically._

![Test results — all 247 tests passing](assets/screenshot3-overview-tests.png)
_Full test suite ran after changes — 247 tests passing, including 12 new tests the agent wrote for the auth module._

![Critique comments](assets/screenshot4-overview-critique.png)
_The critique step found a real bug (milliseconds vs seconds in token expiry) and flagged it before the code ever left your machine._

## How It Works

1. `airlock init` reroutes your `origin` remote to a local bare repo (a "gate")
2. When you `git push`, a daemon picks up the push and runs your pipeline
3. Pipeline jobs run in a temporary worktree — lint, test, describe, critique, review
4. Results appear as a **Push Request** you can review in the Airlock desktop app
5. You review and approve the change to exit the airlock, get pushed upstream and become a clean PR

### Pipeline

Defined in `.airlock/workflows/main.yml` using a familiar YAML workflow syntax with parallel jobs:

```yaml
jobs:
  rebase:
    steps:
      - name: rebase
        uses: airlock-hq/airlock/defaults/rebase@main
  critique:
    needs: rebase
    steps:
      - name: critique
        uses: airlock-hq/airlock/defaults/critique@main
  test:
    needs: rebase
    steps:
      - name: test
        uses: airlock-hq/airlock/defaults/test@main
  gate:
    needs: [critique, test]
    steps:
      - name: review
        run: |
          # Pause for human approval if tests fail or critical issues found
          airlock exec await
  deploy:
    needs: gate
    steps:
      - name: lint
        uses: airlock-hq/airlock/defaults/lint@main
        apply-patch: true
      - name: push
        uses: airlock-hq/airlock/defaults/push@main
      - name: create-pr
        uses: airlock-hq/airlock/defaults/create-pr@main
```

Jobs declare dependencies via `needs:` and run in parallel when possible. Steps with `apply-patch: true` auto-commit any patches they produce.

Steps can be inline shell commands or reusable definitions loaded from Git repos via `uses:`.

The desktop app runs from the system tray: closing the window hides it, and new pushes trigger an OS notification.

## Architecture

```
crates/
├── airlock-cli/      # CLI binary
├── airlock-daemon/   # Background daemon (watches for pushes)
├── airlock-core/     # Core library (git ops, config, pipeline)
├── airlock-app/      # Desktop app (Tauri + React)
└── airlock-fixtures/ # Test fixtures
defaults/             # Built-in reusable pipeline steps
packages/
└── design-system/    # Shared UI components
```

Built with Rust + TypeScript. Desktop app uses Tauri.

## Development

```bash
make dev      # Start desktop app with hot reload
make build    # Build everything
make test     # Run all tests
make check    # Clippy + format + lint checks
```

Run `make help` for all available commands.

## Status

Alpha. Expect rough edges. We're building in the open.

## License

[MIT](LICENSE)
