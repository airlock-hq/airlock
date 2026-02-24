//! StreamCollector: drains an [`AgentEventStream`] into an [`AgentResult`].
//!
//! This is the standard way for callers that need a collected result to consume
//! the stream. It assembles `AgentMessage` entries from streaming events,
//! concatenates `TextDelta` fragments into the final content string, and
//! captures usage metrics and session IDs.

use futures::StreamExt;

use super::types::{
    AgentEvent, AgentEventStream, AgentMessage, AgentResult, AgentUsage, ContentBlock,
};
use crate::error::Result;

/// Drains an [`AgentEventStream`] and assembles an [`AgentResult`].
///
/// The collector processes events in order:
/// - `TextDelta` events are concatenated into the final `content` string.
/// - `SessionStart` captures the session ID and model.
/// - `AssistantMessage` events are recorded as `AgentMessage::Assistant`.
/// - `ToolUse` events are held until a matching `ToolResult` arrives, then
///   emitted as `AgentMessage::ToolRoundtrip`.
/// - `StructuredOutput` overwrites `content` with the JSON string.
/// - `Usage` and `Complete` events update the usage metrics and session ID.
/// - `Error` events with `is_fatal: true` cause the collector to return an error.
pub struct StreamCollector;

/// A pending tool use that hasn't received its result yet.
struct PendingToolUse {
    tool_name: String,
    input: serde_json::Value,
}

impl StreamCollector {
    /// Consume the entire stream and assemble the final [`AgentResult`].
    pub async fn collect(mut stream: AgentEventStream) -> Result<AgentResult> {
        let mut content = String::new();
        let mut session_id: Option<String> = None;
        let mut usage = AgentUsage::default();
        let mut messages: Vec<AgentMessage> = Vec::new();
        let mut current_model: Option<String> = None;
        let mut pending_tool: Option<PendingToolUse> = None;

        while let Some(event_result) = stream.next().await {
            let event = event_result?;

            match event {
                AgentEvent::SessionStart {
                    session_id: sid,
                    model,
                } => {
                    session_id = Some(sid);
                    if model.is_some() {
                        current_model = model;
                    }
                }

                AgentEvent::TextDelta { text } => {
                    content.push_str(&text);
                }

                AgentEvent::AssistantMessage { content: blocks } => {
                    messages.push(AgentMessage::Assistant {
                        content: blocks,
                        model: current_model.clone(),
                    });
                }

                AgentEvent::UserMessage { content: blocks } => {
                    // Extract tool results from user message content blocks
                    for block in blocks {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            output,
                            is_error,
                        } = block
                        {
                            if let Some(pending) = pending_tool.take() {
                                messages.push(AgentMessage::ToolRoundtrip {
                                    tool_name: pending.tool_name,
                                    input: pending.input,
                                    output,
                                    is_error,
                                });
                            } else {
                                messages.push(AgentMessage::ToolRoundtrip {
                                    tool_name: tool_use_id,
                                    input: serde_json::Value::Null,
                                    output,
                                    is_error,
                                });
                            }
                        }
                    }
                }

                AgentEvent::ToolUse { tool_name, input } => {
                    // Flush any previous pending tool that never got a result
                    if let Some(prev) = pending_tool.take() {
                        messages.push(AgentMessage::ToolRoundtrip {
                            tool_name: prev.tool_name,
                            input: prev.input,
                            output: String::new(),
                            is_error: false,
                        });
                    }
                    pending_tool = Some(PendingToolUse { tool_name, input });
                }

                AgentEvent::ToolResult {
                    tool_name,
                    output,
                    is_error,
                } => {
                    if let Some(pending) = pending_tool.take() {
                        // Pair with the pending tool use
                        messages.push(AgentMessage::ToolRoundtrip {
                            tool_name: pending.tool_name,
                            input: pending.input,
                            output,
                            is_error,
                        });
                    } else {
                        // No pending tool use — emit with the tool_name from the result
                        messages.push(AgentMessage::ToolRoundtrip {
                            tool_name,
                            input: serde_json::Value::Null,
                            output,
                            is_error,
                        });
                    }
                }

                AgentEvent::StructuredOutput { data } => {
                    // Structured output replaces the content
                    content = serde_json::to_string(&data).unwrap_or_default();
                }

                AgentEvent::Usage(u) => {
                    usage = u;
                }

                AgentEvent::Complete {
                    session_id: sid,
                    usage: final_usage,
                } => {
                    if sid.is_some() {
                        session_id = sid;
                    }
                    usage = final_usage;
                }

                AgentEvent::Error { message, is_fatal } => {
                    if is_fatal {
                        return Err(crate::error::AirlockError::Agent(message));
                    }
                    // Non-fatal errors are logged but don't stop collection
                }
            }
        }

        // Flush any remaining pending tool use
        if let Some(pending) = pending_tool.take() {
            messages.push(AgentMessage::ToolRoundtrip {
                tool_name: pending.tool_name,
                input: pending.input,
                output: String::new(),
                is_error: false,
            });
        }

        // Fallback: when no TextDelta events were received (non-streaming
        // responses), extract content from AssistantMessage text blocks.
        if content.is_empty() {
            for msg in &messages {
                if let AgentMessage::Assistant {
                    content: blocks, ..
                } = msg
                {
                    for block in blocks {
                        if let ContentBlock::Text { text } = block {
                            content.push_str(text);
                        }
                    }
                }
            }
        }

        Ok(AgentResult {
            content,
            session_id,
            usage,
            messages,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ContentBlock;
    use futures::stream;

    /// Helper: create an AgentEventStream from a vec of events.
    fn mock_stream(events: Vec<AgentEvent>) -> AgentEventStream {
        Box::pin(stream::iter(events.into_iter().map(Ok)))
    }

    #[tokio::test]
    async fn test_collect_text_deltas() {
        let stream = mock_stream(vec![
            AgentEvent::TextDelta {
                text: "Hello".into(),
            },
            AgentEvent::TextDelta { text: ", ".into() },
            AgentEvent::TextDelta {
                text: "world!".into(),
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.content, "Hello, world!");
        assert!(result.messages.is_empty());
    }

    #[tokio::test]
    async fn test_collect_session_start() {
        let stream = mock_stream(vec![
            AgentEvent::SessionStart {
                session_id: "sess-42".into(),
                model: Some("opus".into()),
            },
            AgentEvent::TextDelta { text: "hi".into() },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.session_id, Some("sess-42".into()));
        assert_eq!(result.content, "hi");
    }

    #[tokio::test]
    async fn test_collect_complete_overrides_session_id() {
        let stream = mock_stream(vec![
            AgentEvent::SessionStart {
                session_id: "sess-1".into(),
                model: None,
            },
            AgentEvent::Complete {
                session_id: Some("sess-final".into()),
                usage: AgentUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    duration_ms: 2000,
                    ..Default::default()
                },
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.session_id, Some("sess-final".into()));
        assert_eq!(result.usage.input_tokens, Some(100));
        assert_eq!(result.usage.output_tokens, Some(50));
        assert_eq!(result.usage.duration_ms, 2000);
    }

    #[tokio::test]
    async fn test_collect_assistant_message() {
        let stream = mock_stream(vec![
            AgentEvent::SessionStart {
                session_id: "s".into(),
                model: Some("sonnet".into()),
            },
            AgentEvent::AssistantMessage {
                content: vec![ContentBlock::Text {
                    text: "response".into(),
                }],
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.messages.len(), 1);
        match &result.messages[0] {
            AgentMessage::Assistant { content, model } => {
                assert_eq!(content.len(), 1);
                assert_eq!(model.as_deref(), Some("sonnet"));
            }
            _ => panic!("expected Assistant message"),
        }
    }

    #[tokio::test]
    async fn test_collect_tool_roundtrip() {
        let stream = mock_stream(vec![
            AgentEvent::ToolUse {
                tool_name: "read_file".into(),
                input: serde_json::json!({"path": "/tmp/foo"}),
            },
            AgentEvent::ToolResult {
                tool_name: "read_file".into(),
                output: "file contents".into(),
                is_error: false,
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.messages.len(), 1);
        match &result.messages[0] {
            AgentMessage::ToolRoundtrip {
                tool_name,
                input,
                output,
                is_error,
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(input["path"], "/tmp/foo");
                assert_eq!(output, "file contents");
                assert!(!is_error);
            }
            _ => panic!("expected ToolRoundtrip message"),
        }
    }

    #[tokio::test]
    async fn test_collect_tool_result_without_prior_use() {
        let stream = mock_stream(vec![
            AgentEvent::ToolResult {
                tool_name: "bash".into(),
                output: "ok".into(),
                is_error: false,
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.messages.len(), 1);
        match &result.messages[0] {
            AgentMessage::ToolRoundtrip {
                tool_name,
                input,
                output,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert!(input.is_null());
                assert_eq!(output, "ok");
            }
            _ => panic!("expected ToolRoundtrip"),
        }
    }

    #[tokio::test]
    async fn test_collect_orphaned_tool_use_flushed() {
        // ToolUse without a matching ToolResult — should be flushed at end
        let stream = mock_stream(vec![
            AgentEvent::ToolUse {
                tool_name: "write_file".into(),
                input: serde_json::json!({"path": "/tmp/x"}),
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.messages.len(), 1);
        match &result.messages[0] {
            AgentMessage::ToolRoundtrip {
                tool_name, output, ..
            } => {
                assert_eq!(tool_name, "write_file");
                assert!(output.is_empty());
            }
            _ => panic!("expected ToolRoundtrip"),
        }
    }

    #[tokio::test]
    async fn test_collect_consecutive_tool_uses_flush_previous() {
        // Two ToolUse events in a row — the first should be flushed
        let stream = mock_stream(vec![
            AgentEvent::ToolUse {
                tool_name: "tool_a".into(),
                input: serde_json::json!({}),
            },
            AgentEvent::ToolUse {
                tool_name: "tool_b".into(),
                input: serde_json::json!({}),
            },
            AgentEvent::ToolResult {
                tool_name: "tool_b".into(),
                output: "b result".into(),
                is_error: false,
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.messages.len(), 2);
        match &result.messages[0] {
            AgentMessage::ToolRoundtrip {
                tool_name, output, ..
            } => {
                assert_eq!(tool_name, "tool_a");
                assert!(output.is_empty()); // flushed without result
            }
            _ => panic!("expected flushed ToolRoundtrip for tool_a"),
        }
        match &result.messages[1] {
            AgentMessage::ToolRoundtrip {
                tool_name, output, ..
            } => {
                assert_eq!(tool_name, "tool_b");
                assert_eq!(output, "b result");
            }
            _ => panic!("expected ToolRoundtrip for tool_b"),
        }
    }

    #[tokio::test]
    async fn test_collect_structured_output() {
        let data = serde_json::json!({"title": "My PR", "body": "description"});
        let stream = mock_stream(vec![
            AgentEvent::TextDelta {
                text: "partial text".into(),
            },
            AgentEvent::StructuredOutput { data: data.clone() },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        // StructuredOutput replaces accumulated text
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["title"], "My PR");
    }

    #[tokio::test]
    async fn test_collect_usage_mid_stream() {
        let stream = mock_stream(vec![
            AgentEvent::Usage(AgentUsage {
                input_tokens: Some(50),
                output_tokens: Some(25),
                duration_ms: 500,
                ..Default::default()
            }),
            AgentEvent::TextDelta {
                text: "text".into(),
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    duration_ms: 1000,
                    ..Default::default()
                },
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        // Complete usage should override mid-stream usage
        assert_eq!(result.usage.input_tokens, Some(100));
        assert_eq!(result.usage.output_tokens, Some(50));
        assert_eq!(result.usage.duration_ms, 1000);
    }

    #[tokio::test]
    async fn test_collect_fatal_error() {
        let stream = mock_stream(vec![
            AgentEvent::TextDelta {
                text: "partial".into(),
            },
            AgentEvent::Error {
                message: "something broke".into(),
                is_fatal: true,
            },
        ]);

        let err = StreamCollector::collect(stream).await.unwrap_err();
        assert!(err.to_string().contains("something broke"));
    }

    #[tokio::test]
    async fn test_collect_non_fatal_error_continues() {
        let stream = mock_stream(vec![
            AgentEvent::Error {
                message: "rate limit".into(),
                is_fatal: false,
            },
            AgentEvent::TextDelta {
                text: "recovered".into(),
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.content, "recovered");
    }

    #[tokio::test]
    async fn test_collect_empty_stream() {
        let stream = mock_stream(vec![]);
        let result = StreamCollector::collect(stream).await.unwrap();
        assert!(result.content.is_empty());
        assert!(result.session_id.is_none());
        assert!(result.messages.is_empty());
        assert_eq!(result.usage.duration_ms, 0);
    }

    #[tokio::test]
    async fn test_collect_user_message_with_tool_result() {
        let stream = mock_stream(vec![
            AgentEvent::ToolUse {
                tool_name: "read_file".into(),
                input: serde_json::json!({"path": "/tmp/foo"}),
            },
            AgentEvent::UserMessage {
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tu_1".into(),
                    output: "file contents".into(),
                    is_error: false,
                }],
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.messages.len(), 1);
        match &result.messages[0] {
            AgentMessage::ToolRoundtrip {
                tool_name,
                input,
                output,
                is_error,
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(input["path"], "/tmp/foo");
                assert_eq!(output, "file contents");
                assert!(!is_error);
            }
            _ => panic!("expected ToolRoundtrip message"),
        }
    }

    #[tokio::test]
    async fn test_collect_full_conversation() {
        // Simulate a realistic conversation with multiple turns
        let stream = mock_stream(vec![
            AgentEvent::SessionStart {
                session_id: "sess-full".into(),
                model: Some("opus".into()),
            },
            // First assistant turn with text
            AgentEvent::TextDelta {
                text: "Let me ".into(),
            },
            AgentEvent::TextDelta {
                text: "check that file.".into(),
            },
            AgentEvent::AssistantMessage {
                content: vec![ContentBlock::Text {
                    text: "Let me check that file.".into(),
                }],
            },
            // Tool use
            AgentEvent::ToolUse {
                tool_name: "read_file".into(),
                input: serde_json::json!({"path": "src/main.rs"}),
            },
            AgentEvent::ToolResult {
                tool_name: "read_file".into(),
                output: "fn main() {}".into(),
                is_error: false,
            },
            // Second assistant turn
            AgentEvent::TextDelta {
                text: " Done!".into(),
            },
            AgentEvent::AssistantMessage {
                content: vec![ContentBlock::Text {
                    text: "Done!".into(),
                }],
            },
            AgentEvent::Complete {
                session_id: Some("sess-full".into()),
                usage: AgentUsage {
                    input_tokens: Some(200),
                    output_tokens: Some(100),
                    duration_ms: 3000,
                    duration_api_ms: Some(2500),
                    num_turns: Some(2),
                    raw: None,
                },
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        assert_eq!(result.content, "Let me check that file. Done!");
        assert_eq!(result.session_id, Some("sess-full".into()));
        assert_eq!(result.messages.len(), 3); // assistant + tool roundtrip + assistant
        assert_eq!(result.usage.num_turns, Some(2));

        // Verify message types
        assert!(matches!(
            &result.messages[0],
            AgentMessage::Assistant { .. }
        ));
        assert!(matches!(
            &result.messages[1],
            AgentMessage::ToolRoundtrip { .. }
        ));
        assert!(matches!(
            &result.messages[2],
            AgentMessage::Assistant { .. }
        ));
    }

    #[tokio::test]
    async fn test_collect_content_fallback_from_assistant_messages() {
        // Simulates a non-streaming response (no TextDelta events).
        // The CLI sends AssistantMessage with text but no stream_event deltas.
        // Content should be extracted from AssistantMessage blocks as fallback.
        let stream = mock_stream(vec![
            AgentEvent::SessionStart {
                session_id: "sess-ns".into(),
                model: Some("sonnet".into()),
            },
            AgentEvent::AssistantMessage {
                content: vec![ContentBlock::Text { text: "4".into() }],
            },
            AgentEvent::Complete {
                session_id: Some("sess-ns".into()),
                usage: AgentUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    duration_ms: 500,
                    ..Default::default()
                },
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        // Content should come from the AssistantMessage fallback
        assert_eq!(result.content, "4");
        assert_eq!(result.session_id, Some("sess-ns".into()));
    }

    #[tokio::test]
    async fn test_collect_content_no_fallback_when_text_deltas_present() {
        // When TextDelta events are present, the fallback should NOT trigger,
        // so content isn't double-counted.
        let stream = mock_stream(vec![
            AgentEvent::TextDelta {
                text: "streamed text".into(),
            },
            AgentEvent::AssistantMessage {
                content: vec![ContentBlock::Text {
                    text: "streamed text".into(),
                }],
            },
            AgentEvent::Complete {
                session_id: None,
                usage: AgentUsage::default(),
            },
        ]);

        let result = StreamCollector::collect(stream).await.unwrap();
        // Content should be from TextDelta only, not doubled
        assert_eq!(result.content, "streamed text");
    }
}
