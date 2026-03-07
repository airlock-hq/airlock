# AGENTS.md

This file provides guidance to AI agents when working with code in this repository.

## Project Overview

All slop must die. Airlock is where every git push turns into a slop-free PR.

Airlock is a local-first CI system that transforms messy AI-generated code into clean, reviewable pull requests. It intercepts `git push` operations through a local bare repo "gate", runs a transformation pipeline, and lets users review changes before forwarding to upstream.

## Build Commands

```bash
# Using Makefile (recommended)
make dev              # Start desktop app in dev mode (hot reload)
make build            # Build frontend + all Rust crates
make test             # Run all tests
make check            # Run clippy, format checks, and frontend lint
make fmt              # Format all code (Rust + frontend)

# Run `make` or `make help` to see all available commands.


# Direct cargo commands (only if make commands don't fit your needs)
cargo build              # Build all Rust crates (frontend must be pre-built)
cargo test               # Run all tests
cargo fmt                # Format code
cargo clippy             # Run lints
cargo run --bin airlock  # Run CLI (launches GUI if no args)
cargo run --bin airlockd # Run daemon
```

Never run multiple make or cargo commands in parallel. It will cause lock contention. Run one command at a time, and
wait for it to finish before starting another.

Never run cargo tests in parallel (e.g. `--test-threads=N` where N > 1). Tests share database and filesystem
resources that cause deadlocks under parallel execution. Always use the default single-threaded test runner.

## Desktop App Development

The desktop app uses Tauri (Rust) + React (TypeScript). We use shadcn for UI components (look up context7 for docs when needed).

### Development Mode

When making frontend changes in the desktop app, you should run in development mode with mock data and hot reload:

```bash
cd crates/airlock-app && npm run dev
```

After making UI changes, you need to **always** use chrome-devtools to test the UI, otherwise you don't know if it works.

### Production Build

```bash
make build
# This builds frontend assets first, then compiles Rust
```

**If you see a white screen:** The frontend hasn't been built. Run `make build` or `make dev`.

### Color Guidelines

Use semantic colors instead of hard-coded Tailwind colors for theming support:

| Purpose                  | Use                                       | Avoid                          |
| ------------------------ | ----------------------------------------- | ------------------------------ |
| Success/passed states    | `text-success`, `bg-success`              | `text-green-*`, `bg-green-*`   |
| Warning/pending states   | `text-warning`, `bg-warning`              | `text-yellow-*`, `bg-yellow-*` |
| Error/failed states      | `text-danger`, `bg-danger`                | `text-red-*`, `bg-red-*`       |
| Signal/accent states     | `text-signal`, `border-signal`            | `text-blue-*`, `bg-blue-*`     |
| Terminal/log backgrounds | `bg-terminal`, `text-terminal-foreground` | `bg-zinc-*`, `text-zinc-*`     |

`destructive` is an alias for `danger`; both work. Semantic colors are defined in `packages/design-system/src/styles/tokens.css` and the Tailwind preset at `packages/design-system/src/tailwind/preset.js`.

## Pre-commit Checklist

Before committing, always run `make check` to catch CI failures early. This runs:

- `cargo fmt --check` - Rust formatting
- `cargo clippy` - Rust lints
- `npm run lint` - ESLint (catches unused imports, etc.)
- `npm run format:check` - Prettier formatting

If lint fails, run `make fmt` to auto-fix formatting, and manually fix any lint errors (e.g. remove unused imports).

## Rules

Before implementing a fix, understand the full architecture constraints — especially shared resources (e.g., worktree pool concurrency), plugin lifecycle ordering (e.g., window-state plugin restores after setup), and pipeline customizability (steps may not always run). Ask clarifying questions if unsure rather than assuming.

When referencing external library APIs (Clerk, OpenAI SDK, etc.), verify against official documentation rather than relying on training data. APIs change between major versions. If unsure, say so and look up the docs.

## Debugging

When debugging production issues, systematically enumerate and test hypotheses rather than chasing the first plausible theory. Check environment variables, build configs (eas.json, .env), and server-side overrides before assuming code-level bugs.

## Testing Requirements

After implementing new features, always write:

1. **Unit tests** - Test individual functions and logic in isolation
2. **Integration/E2E tests** - Test the full flow with real git repos and database

### Test Performance

Use the smallest possible timeouts and delays in tests. If a test needs a timeout to trigger, use 1ms — not
100ms, not 1s. Every unnecessary millisecond adds up across the test suite.

### E2E Test Principles

E2E tests must reflect **end-user-visible behavior**. Do things the way a real user would:

- **Call the real code paths.** If the user runs `airlock init`, the test must call the same `run_with_paths` function — not a manual helper that simulates post-init state. Never assume which internal function a command uses; trace the actual execution path first.
- **Assert on user-visible outcomes.** Check what the user would see: Does `git branch -vv` show the right tracking? Does `git push origin main` go to the right place? Does `git remote -v` show the expected URLs? Don't just assert on internal state like database records or ref existence.
- **Test full round trips.** If init changes something, test that eject reverses it correctly by running init then eject, not by manually constructing post-init state.
- **Before writing any test, verify which code path the CLI actually uses.** The daemon handler and CLI command may have separate implementations. A test that only covers one is incomplete.

Write real integration/E2E tests, not simulation tests. Tests should exercise actual behavior (real git repos, real subprocess calls) and use minimal timeouts (1ms not 100ms). Avoid running tests in parallel when they contend on shared resources like file locks.

Use test-driven development when building new features or fixing bugs.

## Pipeline Step Scripts (defaults/)

Step scripts in `defaults/*/step.yml` run on user machines. **Never use `jq`** — it's not universally installed. Use `airlock exec json` instead:

```bash
# Extract a field
TITLE=$(echo "$RESPONSE" | airlock exec json title)

# Extract nested field
VALUE=$(echo "$RESPONSE" | airlock exec json a.b.c)

# Build JSON from scratch (--set values auto-detect types: numbers, booleans, strings)
echo '{}' | airlock exec json \
    --set "title=$TITLE" \
    --set "count=42" \
    --set "passed=true" \
    > output.json
```

## Code Organization

### File Size Guidelines

Keep files under **500 lines** as a soft limit. If a file approaches or exceeds this:

1. **Split into submodules** - Create a directory with the same name as the file and add a `mod.rs` with re-exports for backward compatibility
2. **Separate tests** - Move `#[cfg(test)]` modules to a dedicated `tests.rs` file in the same directory
3. **Group by responsibility** - Each submodule should have a single, clear purpose

Example structure when splitting `foo.rs`:

```
foo/
├── mod.rs          # Re-exports, main struct/enum definitions
├── helpers.rs      # Utility functions
├── operations.rs   # Core logic
└── tests.rs        # All tests (#[cfg(test)])
```

### Rust Module Patterns

- Use `mod.rs` files with `pub use` re-exports to maintain backward compatibility when refactoring
- For test files in `tests/`, use `#[path = "subdir/file.rs"]` attributes to organize into subdirectories
- Keep `impl` blocks close to their struct definitions, but they can span multiple files if needed

## Data Files and Debugging

Airlock stores its data in `~/.airlock/`:

```
~/.airlock/
├── state.sqlite    # Main database (repos, runs, stage_results, sync_log)
├── repos/          # Gate bare repos for each enrolled repository
├── worktrees/      # Pool worktrees for pipeline execution (worktrees/{repo_id}/pool-{N})
├── artifacts/      # Stage output artifacts (logs, etc.)
├── locks/          # File locks for concurrent access
└── socket          # Unix socket for daemon IPC
```

## Specs

Detailed specifications are in `specs/`:

- `airlock_business_plan.md` - Business plan and market positioning
- `airlock_technical_spec.md` - Architecture and design decisions
- `airlock_product_spec.md` - User experience and features
- `mvp_implementation_plan.md` - Implementation progress tracking
