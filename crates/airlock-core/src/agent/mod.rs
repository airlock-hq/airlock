//! Agent module for Airlock.
//!
//! This module provides a multi-provider agent abstraction layer. Each agent CLI
//! (Claude Code, Codex, etc.) is wrapped in an adapter that implements the
//! [`AgentAdapter`] trait, producing a unified [`AgentEventStream`].
//!
//! Streaming is the only execution mode — every adapter returns a stream of
//! [`AgentEvent`] values. Callers that need a collected result use
//! [`StreamCollector`] to drain the stream into an [`AgentResult`].

mod claude_code;
mod codex;
mod idle_timeout;
pub mod stream;
pub mod subprocess;
pub mod types;

use async_trait::async_trait;

use crate::error::{AirlockError, Result};
pub use claude_code::{try_extract_json, ClaudeCodeAdapter};
pub use codex::CodexAdapter;
pub use idle_timeout::IdleTimeoutAdapter;
pub use stream::StreamCollector;
pub use types::{
    AgentEvent, AgentEventStream, AgentMessage, AgentRequest, AgentResult, AgentUsage, ContentBlock,
};

/// Trait implemented by every agent adapter.
///
/// Each adapter wraps a specific agent CLI (e.g., Claude Code, Codex) and
/// translates its native output into a unified stream of [`AgentEvent`] values.
///
/// Streaming is the only execution mode — there is no separate non-streaming
/// `run()` method. Callers that need a collected result drain the stream with
/// [`StreamCollector`].
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    /// Human-readable name (e.g., "Claude Code", "Codex").
    fn name(&self) -> &str;

    /// Check if this agent CLI is installed and available on PATH.
    fn is_available(&self) -> bool;

    /// Run a prompt and return a stream of events for real-time output.
    ///
    /// Streaming is the only execution mode — there is no non-streaming `run()`.
    /// Each adapter does best effort to produce a consistent stream of
    /// [`AgentEvent`] variants regardless of how the underlying CLI delivers
    /// output.
    async fn run(&self, request: &AgentRequest) -> Result<AgentEventStream>;
}

// ---------------------------------------------------------------------------
// Adapter registry
// ---------------------------------------------------------------------------

/// Create an adapter by name.
///
/// Supported names:
/// - `"claude-code"` or `"claude"` — Claude Code adapter
/// - `"codex"` — OpenAI Codex adapter
/// - `"auto"` — auto-detect the first available adapter on PATH
///
/// All adapters are wrapped with [`IdleTimeoutAdapter`] to kill hung
/// subprocesses that produce no output for an extended period.
pub fn create_adapter(name: &str) -> Result<Box<dyn AgentAdapter>> {
    let inner: Box<dyn AgentAdapter> = match name {
        "claude-code" | "claude" => Box::new(ClaudeCodeAdapter::new()),
        "codex" => Box::new(CodexAdapter::new()),
        "auto" => return detect_available_adapter(),
        _ => {
            return Err(AirlockError::Config(format!(
                "Unknown agent adapter: {name}"
            )))
        }
    };
    Ok(Box::new(IdleTimeoutAdapter::new(inner)))
}

/// Auto-detect the first available agent CLI on PATH.
///
/// Checks in priority order: `claude` → `codex`.
fn detect_available_adapter() -> Result<Box<dyn AgentAdapter>> {
    let adapters: Vec<Box<dyn AgentAdapter>> = vec![
        Box::new(ClaudeCodeAdapter::new()),
        Box::new(CodexAdapter::new()),
    ];
    for adapter in adapters {
        if adapter.is_available() {
            return Ok(Box::new(IdleTimeoutAdapter::new(adapter)));
        }
    }
    Err(AirlockError::Agent(
        "No agent CLI found on PATH. Install one of: claude, codex".into(),
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_adapter_claude_code() {
        let adapter = create_adapter("claude-code").unwrap();
        assert_eq!(adapter.name(), "Claude Code");
    }

    #[test]
    fn test_create_adapter_claude_alias() {
        let adapter = create_adapter("claude").unwrap();
        assert_eq!(adapter.name(), "Claude Code");
    }

    #[test]
    fn test_create_adapter_codex() {
        let adapter = create_adapter("codex").unwrap();
        assert_eq!(adapter.name(), "Codex");
    }

    #[test]
    fn test_create_adapter_auto() {
        // auto should either succeed or fail with "No agent CLI found"
        let result = create_adapter("auto");
        match result {
            Ok(adapter) => {
                // Should be one of the known adapters
                assert!(
                    adapter.name() == "Claude Code" || adapter.name() == "Codex",
                    "unexpected adapter: {}",
                    adapter.name()
                );
            }
            Err(e) => {
                assert!(e.to_string().contains("No agent CLI found"));
            }
        }
    }

    #[test]
    fn test_create_adapter_unknown() {
        let result = create_adapter("unknown-adapter");
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Unknown agent adapter"));
    }
}
