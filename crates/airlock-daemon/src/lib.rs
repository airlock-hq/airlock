//! Airlock Daemon Library
//!
//! This crate provides the daemon implementation for Airlock.
//! The pipeline module is exposed publicly for integration testing.

// Artifact cleanup utilities
pub mod cleanup;

// IPC types for JSON-RPC communication
pub mod ipc;

// Expose pipeline module for integration testing
pub mod pipeline;

// Per-repo run serialization queue
pub mod run_queue;

// Step loader for reusable steps (actions)
pub mod stage_loader;

// Per-repo pool of reusable worktrees
pub mod worktree_pool;
