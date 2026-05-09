# Follow-up: Batch Query Optimization for Remaining N+1 Callsites

> [!WARNING]
> This TODO is historical.
> Airlock is deprecated, and active development has moved to [kunchenguid/no-mistakes](https://github.com/kunchenguid/no-mistakes).

After eliminating the N+1 pattern in the daemon's run handlers (`handle_get_all_runs`, `handle_get_runs`, `handle_get_run_counts`), these production callsites still use the per-run `get_job_results_for_run` / `compute_run_status` in a loop.

## Worth optimizing

### 1. CLI `airlock runs` command

**File:** `crates/airlock-cli/src/commands/runs.rs:86-88`

Loops through all runs (up to 100) calling `compute_run_status` per run. Same N+1 pattern as the daemon handlers we already fixed. Use `get_job_results_for_runs` to batch-fetch, then derive status from the map.

### 2. CLI `airlock status` command

**File:** `crates/airlock-cli/src/commands/status.rs:99-108` and `:173-176`

Two separate loops: one over `active_runs` and one over `recent_runs` (up to 10). Both call `compute_run_status` per run. Could combine both run sets and batch-fetch in a single query.

### 3. `supersede_older_runs` in push handler

**File:** `crates/airlock-daemon/src/handlers/push.rs:219-220`

Loops through superseded runs fetching job results to find paused jobs for worktree release. Usually small (1-3 runs), but trivially batchable with the existing method.

## Not worth optimizing (single-run or rare paths)

- **`server.rs:165`** — Orphan cleanup at daemon startup. Runs once, low priority.
- **`pipeline.rs:1556`** (`emit_run_final_status`) — Single run per call, no loop.
- **`steps.rs` handlers** — All operate on a single run per request.
- **`push.rs:840, 973`** — Cleanup handler, called infrequently.
