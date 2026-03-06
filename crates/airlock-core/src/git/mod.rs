//! Git operations for Airlock.
//!
//! This module provides utilities for managing Git repositories, including:
//! - Creating bare repositories (gates)
//! - Managing remotes (add, rename, get URL)
//! - Installing hooks
//! - Fetching from and pushing to remotes
//! - Parsing ref updates
//! - Computing diffs with null SHA handling

mod cmd;
mod diff;
mod fetch;
pub mod hooks;
mod push;
mod refs;
mod remote;
mod repo;
mod show;

// Re-export all public items for backward compatibility
pub use diff::{
    compute_diff, compute_diff_with_commits, find_effective_base_sha, find_merge_base,
    find_root_commit, get_commit_patch, list_commits, CommitDiffResult, CommitInfo, DiffResult,
    DEFAULT_BRANCHES, EMPTY_TREE_SHA,
};
pub use fetch::{
    create_local_tracking_branches, ensure_tracking_for_existing_branches, fetch, fetch_all,
    fetch_with_refspecs, list_local_branches, mirror_from_remote, smart_sync_from_remote,
    BranchSyncStatus, ConflictResolver, SyncReport,
};
pub use hooks::{
    configure_upload_pack, install_hooks, install_upload_pack_wrapper, post_receive_hook,
    pre_receive_hook, remove_hooks, UPLOAD_PACK_WRAPPER,
};
pub use push::{
    build_refspec, push, push_all_branches, push_branch, push_force_with_lease, push_ref_updates,
};
pub use refs::{
    classify_ref, cleanup_push_markers, delete_ref, get_ref_update_type, is_ancestor_of,
    is_null_sha, is_pipeline_ref, list_push_markers, parse_ref_updates, push_marker_ref,
    resolve_ref, rev_parse_head, run_ref, update_ref, RefClass, RefUpdateType,
};
pub use remote::{
    add_remote, get_remote_url, list_remotes, remote_exists, remove_remote, rename_remote,
    repoint_tracking_branches, set_remote_url,
};
pub use repo::{
    configure_gate_ssh, create_bare_repo, discover_repo, get_current_branch, get_git_config,
    get_repo_id_from_path, get_workdir, is_git_repo, open_repo,
};
pub use show::show_file;

#[cfg(test)]
mod tests;
