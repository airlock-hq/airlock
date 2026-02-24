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

// Stage loader for reusable stages
pub mod stage_loader;
