//! Tests for daemon lifecycle commands.
//!
//! These tests verify:
//! 1. Service file generation
//! 2. IPC request/response formatting
//! 3. Command logic (without actually starting daemon)
//!
//! This file serves as the test entry point and imports all test modules.

#[path = "daemon_commands_tests/service_manager.rs"]
mod service_manager;

#[path = "daemon_commands_tests/socket.rs"]
mod socket;

#[path = "daemon_commands_tests/daemon_path.rs"]
mod daemon_path;

#[path = "daemon_commands_tests/error_messages.rs"]
mod error_messages;

#[path = "daemon_commands_tests/init_help.rs"]
mod init_help;

#[path = "daemon_commands_tests/common.rs"]
mod common;

#[path = "daemon_commands_tests/daemon_lifecycle.rs"]
mod daemon_lifecycle;

#[path = "daemon_commands_tests/cli_ux.rs"]
mod cli_ux;

#[path = "daemon_commands_tests/ipc_types.rs"]
mod ipc_types;
