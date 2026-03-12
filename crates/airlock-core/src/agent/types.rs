//! Unified types for the agent adapter system.
//!
//! These types provide a provider-agnostic interface for agent interactions.
//! All adapters (Claude Code, Codex, etc.) map their native types to these.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::pin::Pin;

use futures::Stream;

use crate::error::Result;

/// A stream of agent events, the only output mode for all adapters.
pub type AgentEventStream = Pin<Box<dyn Stream<Item = Result<AgentEvent>> + Send>>;

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// Unified request sent to any agent adapter.
#[derive(Debug, Clone, Default)]
pub struct AgentRequest {
    /// The prompt text.
    pub prompt: String,
    /// Additional context (e.g., from stdin pipe).
    pub context: Option<String>,
    /// Working directory for the agent.
    pub cwd: Option<PathBuf>,
    /// JSON Schema for structured output validation.
    pub output_schema: Option<serde_json::Value>,
    /// Model override (adapter-specific format).
    pub model: Option<String>,
    /// Session ID to resume (`None` = new session).
    pub resume_session: Option<String>,
    /// Maximum turns/iterations the agent may take.
    pub max_turns: Option<u32>,
}

impl AgentRequest {
    /// Build the full prompt, combining context if present.
    ///
    /// If `context` is set and non-empty, wraps it in a code block
    /// and prefixes the prompt with "Task:".
    pub fn full_prompt(&self) -> String {
        match &self.context {
            Some(ctx) if !ctx.is_empty() => {
                format!(
                    "Context:\n```\n{}\n```\n\nTask: {}",
                    ctx.trim(),
                    self.prompt
                )
            }
            _ => self.prompt.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Result (assembled from a completed stream)
// ---------------------------------------------------------------------------

/// Final result assembled after an agent stream completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// The primary text or structured content.
    pub content: String,
    /// Session ID (for potential resume).
    pub session_id: Option<String>,
    /// Token usage metrics.
    pub usage: AgentUsage,
    /// Conversation history as a list of messages.
    pub messages: Vec<AgentMessage>,
}

/// Token / cost / timing metrics from an agent invocation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// API-only duration in milliseconds (provider-reported).
    pub duration_api_ms: Option<u64>,
    /// Number of agentic turns.
    pub num_turns: Option<u32>,
    /// Raw provider-specific usage blob for passthrough.
    pub raw: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Session messages (coarse-grained conversation history)
// ---------------------------------------------------------------------------

/// A single turn in the conversation — the unit of session history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    /// The user/system prompt that started this turn.
    User { content: String },
    /// A complete assistant response (one "turn" of output).
    Assistant {
        content: Vec<ContentBlock>,
        model: Option<String>,
    },
    /// A tool invocation and its result, paired together.
    ToolRoundtrip {
        tool_name: String,
        input: serde_json::Value,
        output: String,
        is_error: bool,
    },
}

// ---------------------------------------------------------------------------
// Streaming events (fine-grained, incremental)
// ---------------------------------------------------------------------------

/// An incremental event emitted during agent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Session started, metadata available.
    SessionStart {
        session_id: String,
        model: Option<String>,
    },
    /// Incremental text from the assistant.
    TextDelta { text: String },
    /// Complete assistant message (may contain thinking, tool calls, etc.).
    AssistantMessage { content: Vec<ContentBlock> },
    /// User message (tool results, etc.).
    UserMessage { content: Vec<ContentBlock> },
    /// Agent invoked a tool.
    ToolUse {
        tool_name: String,
        input: serde_json::Value,
    },
    /// Tool produced a result.
    ToolResult {
        tool_name: String,
        output: String,
        is_error: bool,
    },
    /// Final structured output (when `output_schema` was provided).
    StructuredOutput { data: serde_json::Value },
    /// Usage/cost update (may arrive mid-stream or only at end).
    Usage(AgentUsage),
    /// Agent finished.
    Complete {
        session_id: Option<String>,
        usage: AgentUsage,
    },
    /// An error occurred (non-fatal if stream continues, fatal if stream ends).
    Error { message: String, is_fatal: bool },
}

/// A block of content within an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        output: String,
        is_error: bool,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // AgentRequest::full_prompt tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_prompt_no_context() {
        let request = AgentRequest {
            prompt: "Hello".into(),
            ..Default::default()
        };
        assert_eq!(request.full_prompt(), "Hello");
    }

    #[test]
    fn test_full_prompt_empty_context() {
        let request = AgentRequest {
            prompt: "Hello".into(),
            context: Some("".into()),
            ..Default::default()
        };
        assert_eq!(request.full_prompt(), "Hello");
    }

    #[test]
    fn test_full_prompt_with_context() {
        let request = AgentRequest {
            prompt: "Summarize".into(),
            context: Some("some diff content".into()),
            ..Default::default()
        };
        let prompt = request.full_prompt();
        assert!(prompt.contains("Context:"));
        assert!(prompt.contains("some diff content"));
        assert!(prompt.contains("Task: Summarize"));
    }

    // -----------------------------------------------------------------------

    #[test]
    fn test_agent_event_roundtrip_all_variants() {
        let events: Vec<AgentEvent> = vec![
            AgentEvent::TextDelta {
                text: "hello".into(),
            },
            AgentEvent::SessionStart {
                session_id: "sess-123".into(),
                model: Some("sonnet".into()),
            },
            AgentEvent::UserMessage {
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".into(),
                    output: "file contents".into(),
                    is_error: false,
                }],
            },
            AgentEvent::ToolUse {
                tool_name: "read_file".into(),
                input: serde_json::json!({"path": "/tmp/foo.txt"}),
            },
            AgentEvent::ToolResult {
                tool_name: "read_file".into(),
                output: "content".into(),
                is_error: false,
            },
            AgentEvent::StructuredOutput {
                data: serde_json::json!({"title": "PR"}),
            },
            AgentEvent::Error {
                message: "rate limit".into(),
                is_fatal: false,
            },
            AgentEvent::Complete {
                session_id: Some("sess-abc".into()),
                usage: AgentUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    duration_ms: 1500,
                    duration_api_ms: Some(1200),
                    num_turns: Some(3),
                    raw: None,
                },
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let val: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert!(val.get("type").is_some(), "Missing type tag in: {}", json);
            let roundtrip: AgentEvent = serde_json::from_str(&json).unwrap();
            let roundtrip_json = serde_json::to_string(&roundtrip).unwrap();
            assert_eq!(json, roundtrip_json, "Roundtrip mismatch for event");
        }
    }

    #[test]
    fn test_agent_message_and_content_block_roundtrip() {
        // Test all AgentMessage variants
        let messages: Vec<AgentMessage> = vec![
            AgentMessage::User {
                content: "hello".into(),
            },
            AgentMessage::Assistant {
                content: vec![ContentBlock::Text {
                    text: "response".into(),
                }],
                model: Some("opus".into()),
            },
            AgentMessage::ToolRoundtrip {
                tool_name: "bash".into(),
                input: serde_json::json!({"command": "ls"}),
                output: "file.txt\n".into(),
                is_error: false,
            },
        ];

        for msg in &messages {
            let json = serde_json::to_string(msg).unwrap();
            let roundtrip: AgentMessage = serde_json::from_str(&json).unwrap();
            let roundtrip_json = serde_json::to_string(&roundtrip).unwrap();
            assert_eq!(json, roundtrip_json, "Message roundtrip mismatch");
        }

        // Test all ContentBlock variants
        let blocks: Vec<ContentBlock> = vec![
            ContentBlock::Text {
                text: "hello".into(),
            },
            ContentBlock::Thinking {
                thinking: "hmm".into(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read".into(),
                input: serde_json::json!({}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                output: "done".into(),
                is_error: false,
            },
        ];

        for block in &blocks {
            let json = serde_json::to_string(block).unwrap();
            let val: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert!(val.get("type").is_some());
            let roundtrip: ContentBlock = serde_json::from_str(&json).unwrap();
            let roundtrip_json = serde_json::to_string(&roundtrip).unwrap();
            assert_eq!(json, roundtrip_json, "ContentBlock roundtrip mismatch");
        }
    }

    #[test]
    fn test_agent_result_roundtrip() {
        let result = AgentResult {
            content: "done".into(),
            session_id: Some("s-1".into()),
            usage: AgentUsage {
                input_tokens: Some(10),
                output_tokens: Some(20),
                duration_ms: 500,
                duration_api_ms: None,
                num_turns: Some(1),
                raw: Some(serde_json::json!({"provider": "test"})),
            },
            messages: vec![
                AgentMessage::User {
                    content: "hi".into(),
                },
                AgentMessage::Assistant {
                    content: vec![ContentBlock::Text {
                        text: "done".into(),
                    }],
                    model: None,
                },
            ],
        };

        let json = serde_json::to_string_pretty(&result).unwrap();
        let roundtrip: AgentResult = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.content, "done");
        assert_eq!(roundtrip.session_id, Some("s-1".into()));
        assert_eq!(roundtrip.usage.input_tokens, Some(10));
        assert_eq!(roundtrip.usage.raw.unwrap()["provider"], "test");
        assert_eq!(roundtrip.messages.len(), 2);
    }
}
