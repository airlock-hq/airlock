//! Claude Code agent adapter.
//!
//! Implements [`AgentAdapter`] by spawning the `claude` CLI directly with
//! `--output-format stream-json`. Streaming is the only execution mode —
//! the adapter reads JSONL from stdout, parses each line into SDK [`Message`]
//! values, and maps them to [`AgentEvent`] values yielded through a pinned
//! stream.
//!
//! We bypass the SDK's `query_stream()` because it terminates the stream
//! on any message parse error (e.g. unknown variants like `rate_limit_event`).
//! By spawning the CLI directly, we can skip unknown message types and
//! continue reading subsequent events.

use async_trait::async_trait;
use claude_agent_sdk_rs::{
    ContentBlock as SdkContentBlock, Message, StreamEvent, ToolResultContent,
};
use std::process::Command;
use tokio::io::AsyncBufReadExt;
use tracing::debug;

use super::types::{AgentEvent, AgentEventStream, AgentRequest, AgentUsage, ContentBlock};
use super::AgentAdapter;
use crate::error::{AirlockError, Result};

/// Claude Code adapter using the `claude-agent-sdk-rs` SDK.
#[derive(Default)]
pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn is_available(&self) -> bool {
        let result = if cfg!(target_os = "windows") {
            Command::new("where").arg("claude").output()
        } else {
            Command::new("which").arg("claude").output()
        };

        match result {
            Ok(output) => output.status.success(),
            Err(e) => {
                debug!("Failed to check for claude CLI: {}", e);
                false
            }
        }
    }

    async fn run(&self, request: &AgentRequest) -> Result<AgentEventStream> {
        if !self.is_available() {
            return Err(AirlockError::Agent(
                "Claude Code CLI not found.\n\n\
                 To use Airlock's AI features, you need:\n\
                 1. Install Claude Code: https://claude.ai/code\n\
                 2. Log in by running: claude"
                    .to_string(),
            ));
        }

        let prompt = build_prompt(request);
        let args = build_cli_args(request);

        debug!(
            "Starting Claude Code stream (cwd: {:?}): {} chars",
            request.cwd,
            prompt.len()
        );

        // Spawn claude CLI directly (bypassing SDK's query_stream which
        // terminates the stream on unknown message variants).
        let mut cmd = tokio::process::Command::new("claude");
        cmd.args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        if let Some(ref cwd) = request.cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| AirlockError::Agent(format!("Failed to spawn claude CLI: {}", e)))?;

        // Write prompt to stdin and close it
        let stdin = child.stdin.take().ok_or_else(|| {
            AirlockError::Agent("Failed to get stdin for claude process".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            AirlockError::Agent("Failed to get stdout for claude process".to_string())
        })?;

        // Send prompt asynchronously then close stdin
        let prompt_for_task = prompt;
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let mut stdin = stdin;
            let _ = stdin.write_all(prompt_for_task.as_bytes()).await;
            let _ = stdin.write_all(b"\n").await;
            let _ = stdin.shutdown().await;
        });

        let reader = tokio::io::BufReader::new(stdout);
        Ok(claude_code_event_stream(reader, child))
    }
}

/// Build the full prompt, combining context if present.
fn build_prompt(request: &AgentRequest) -> String {
    match &request.context {
        Some(ctx) if !ctx.is_empty() => {
            format!(
                "Context:\n```\n{}\n```\n\nTask: {}",
                ctx.trim(),
                request.prompt
            )
        }
        _ => request.prompt.clone(),
    }
}

/// Build CLI arguments for the `claude` command.
fn build_cli_args(request: &AgentRequest) -> Vec<String> {
    let mut args = vec![
        "--output-format".to_string(),
        "stream-json".to_string(),
        // --verbose is required with --output-format=stream-json
        "--verbose".to_string(),
        "--permission-mode".to_string(),
        "bypassPermissions".to_string(),
        "--setting-sources".to_string(),
        "project,user".to_string(),
    ];

    if let Some(ref model) = request.model {
        args.push("--model".to_string());
        args.push(model.clone());
    }

    if let Some(ref session_id) = request.resume_session {
        args.push("--resume".to_string());
        args.push(session_id.clone());
    }

    if let Some(max_turns) = request.max_turns {
        args.push("--max-turns".to_string());
        args.push(max_turns.to_string());
    }

    if let Some(ref schema) = request.output_schema {
        args.push("--json-schema".to_string());
        args.push(schema.to_string());
    }

    args
}

/// Create an [`AgentEventStream`] from a JSONL reader over the claude CLI stdout.
///
/// Reads JSONL lines, parses each into a [`Message`], and maps to [`AgentEvent`]
/// values. Unknown message variants (e.g. `rate_limit_event`) are silently
/// skipped so the stream continues.
fn claude_code_event_stream(
    reader: tokio::io::BufReader<tokio::process::ChildStdout>,
    _child: tokio::process::Child,
) -> AgentEventStream {
    let lines = reader.lines();

    let stream = futures::stream::unfold(
        (lines, _child, Vec::<Result<AgentEvent>>::new(), false),
        |(mut lines, child, mut pending, completed)| async move {
            if completed {
                return None;
            }

            // Drain pending events first
            if !pending.is_empty() {
                let event = pending.remove(0);
                return Some((event, (lines, child, pending, false)));
            }

            loop {
                match lines.next_line().await {
                    Err(e) => {
                        return Some((
                            Err(AirlockError::Agent(format!(
                                "Error reading claude stdout: {}",
                                e
                            ))),
                            (lines, child, pending, true),
                        ));
                    }
                    Ok(None) => {
                        // EOF — process exited
                        return None;
                    }
                    Ok(Some(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        // Try to parse as Message directly from the JSON string.
                        // This avoids the SDK's query_stream which breaks on unknown variants.
                        match serde_json::from_str::<Message>(trimmed) {
                            Ok(msg) => {
                                let mut events = map_message_to_events(msg);
                                if events.is_empty() {
                                    continue;
                                }
                                let first = events.remove(0);
                                pending = events;
                                return Some((first, (lines, child, pending, false)));
                            }
                            Err(e) => {
                                let err_str = e.to_string();
                                if err_str.contains("unknown variant") {
                                    // Unknown message types (e.g. rate_limit_event) are
                                    // expected as the CLI evolves. Skip and continue.
                                    debug!("Skipping unknown message variant: {}", err_str);
                                    continue;
                                }
                                // Other parse errors are non-fatal but reported
                                return Some((
                                    Ok(AgentEvent::Error {
                                        message: format!("Message parse error: {}", err_str),
                                        is_fatal: false,
                                    }),
                                    (lines, child, pending, false),
                                ));
                            }
                        }
                    }
                }
            }
        },
    );

    Box::pin(stream)
}

/// Map a single SDK [`Message`] into one or more [`AgentEvent`] values.
///
/// A single SDK message can produce multiple events. For example, an
/// `Assistant` message with both text and tool_use blocks produces
/// `AssistantMessage`, `ToolUse`, etc.
fn map_message_to_events(message: Message) -> Vec<Result<AgentEvent>> {
    match message {
        Message::System(sys) => {
            // The CLI emits subtype "init" (older/current versions) or
            // "session_start" — treat both as session start.
            if sys.subtype == "session_start" || sys.subtype == "init" {
                if let Some(session_id) = sys.session_id {
                    return vec![Ok(AgentEvent::SessionStart {
                        session_id,
                        model: sys.model,
                    })];
                }
            }
            vec![]
        }

        Message::StreamEvent(stream_event) => map_stream_event(&stream_event),

        Message::Assistant(assistant) => {
            let mut events = Vec::new();
            let mut content_blocks = Vec::new();

            for block in &assistant.message.content {
                match block {
                    SdkContentBlock::Text(text_block) => {
                        content_blocks.push(ContentBlock::Text {
                            text: text_block.text.clone(),
                        });
                    }
                    SdkContentBlock::Thinking(thinking_block) => {
                        content_blocks.push(ContentBlock::Thinking {
                            thinking: thinking_block.thinking.clone(),
                        });
                    }
                    SdkContentBlock::ToolUse(tool_use) => {
                        // Emit as a dedicated ToolUse event only — not in AssistantMessage
                        events.push(Ok(AgentEvent::ToolUse {
                            tool_name: tool_use.name.clone(),
                            input: tool_use.input.clone(),
                        }));
                    }
                    SdkContentBlock::ToolResult(_) | SdkContentBlock::Image(_) => {
                        // ToolResult blocks arrive via Message::User, not here.
                        // Image blocks are not mapped to events.
                    }
                }
            }

            // Only emit AssistantMessage if there are text/thinking blocks
            if !content_blocks.is_empty() {
                events.insert(
                    0,
                    Ok(AgentEvent::AssistantMessage {
                        content: content_blocks,
                    }),
                );
            }

            events
        }

        Message::Result(result_msg) => {
            let mut events = Vec::new();

            // Emit StructuredOutput if present
            if let Some(structured) = result_msg.structured_output {
                events.push(Ok(AgentEvent::StructuredOutput { data: structured }));
            }

            // Extract usage from ResultMessage
            let (input_tokens, output_tokens) = extract_usage_tokens(&result_msg.usage);
            let usage = AgentUsage {
                input_tokens,
                output_tokens,
                duration_ms: result_msg.duration_ms,
                duration_api_ms: Some(result_msg.duration_api_ms),
                num_turns: Some(result_msg.num_turns),
                raw: result_msg.usage,
            };

            events.push(Ok(AgentEvent::Complete {
                session_id: Some(result_msg.session_id),
                usage,
            }));

            events
        }

        Message::User(user) => {
            // Emit user messages (tool results, etc.) as a single UserMessage event
            let mut content_blocks = Vec::new();
            if let Some(blocks) = &user.content {
                for block in blocks {
                    match block {
                        SdkContentBlock::ToolResult(tool_result) => {
                            let output = extract_tool_result_content(&tool_result.content);
                            let is_error = tool_result.is_error.unwrap_or(false);
                            content_blocks.push(ContentBlock::ToolResult {
                                tool_use_id: tool_result.tool_use_id.clone(),
                                output,
                                is_error,
                            });
                        }
                        SdkContentBlock::Text(text_block) => {
                            content_blocks.push(ContentBlock::Text {
                                text: text_block.text.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }
            if content_blocks.is_empty() {
                vec![]
            } else {
                vec![Ok(AgentEvent::UserMessage {
                    content: content_blocks,
                })]
            }
        }

        Message::ControlCancelRequest(_) => {
            vec![]
        }
    }
}

/// Map a [`StreamEvent`] to [`AgentEvent`] values.
///
/// StreamEvent carries incremental deltas. We extract `TextDelta` from
/// `content_block_delta` events.
fn map_stream_event(event: &StreamEvent) -> Vec<Result<AgentEvent>> {
    let event_data = &event.event;

    let event_type = event_data.get("type").and_then(|t| t.as_str());

    match event_type {
        Some("content_block_delta") => {
            if let Some(delta) = event_data.get("delta") {
                let delta_type = delta.get("type").and_then(|t| t.as_str());
                match delta_type {
                    Some("text_delta") => {
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            return vec![Ok(AgentEvent::TextDelta {
                                text: text.to_string(),
                            })];
                        }
                    }
                    Some("thinking_delta") => {
                        // Thinking deltas are not emitted as separate events;
                        // the full thinking block arrives with the Assistant message.
                    }
                    _ => {}
                }
            }
            vec![]
        }
        _ => vec![],
    }
}

/// Extract text content from an SDK [`ToolResultContent`].
fn extract_tool_result_content(content: &Option<ToolResultContent>) -> String {
    match content {
        Some(ToolResultContent::Text(text)) => text.clone(),
        Some(ToolResultContent::Blocks(blocks)) => {
            // Concatenate text from block entries
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        }
        None => String::new(),
    }
}

/// Extract input/output token counts from the SDK's raw usage JSON.
fn extract_usage_tokens(usage: &Option<serde_json::Value>) -> (Option<u64>, Option<u64>) {
    match usage {
        Some(u) => {
            let input = u.get("input_tokens").and_then(|v| v.as_u64());
            let output = u.get("output_tokens").and_then(|v| v.as_u64());
            (input, output)
        }
        None => (None, None),
    }
}

/// Try to extract JSON from text content.
///
/// Handles common patterns:
/// 1. Raw JSON object/array
/// 2. JSON in markdown code blocks (```json ... ``` or ``` ... ```)
/// 3. JSON embedded in text (find first { to last })
///
/// This is kept as a utility for callers that need to extract JSON from
/// assistant text responses (e.g., when structured output isn't available).
pub fn try_extract_json(text: &str) -> Option<String> {
    let trimmed = text.trim();

    // Pattern 1: Already valid JSON
    let looks_like_json = (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'));
    if looks_like_json && serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }

    // Pattern 2: JSON in markdown code block
    // Use literal markers; after finding one we skip any trailing whitespace
    // to reach the JSON content, and look for a closing ``` on its own line.
    let code_block_markers = ["```json", "```"];

    for marker in &code_block_markers {
        if let Some(marker_idx) = trimmed.find(marker) {
            let after_marker = marker_idx + marker.len();
            // Skip whitespace/newlines between the marker and the JSON content
            let content_start = trimmed[after_marker..]
                .find(|c: char| !c.is_whitespace())
                .map(|i| after_marker + i)
                .unwrap_or(after_marker);
            // Find the closing ``` after the content
            if let Some(end_offset) = trimmed[content_start..].find("\n```") {
                let json_content = trimmed[content_start..content_start + end_offset].trim();
                if serde_json::from_str::<serde_json::Value>(json_content).is_ok() {
                    return Some(json_content.to_string());
                }
            }
        }
    }

    // Pattern 3: Find JSON object in text (first { to matching })
    if let Some(start) = trimmed.find('{') {
        let mut depth = 0;
        let mut end = None;
        let bytes = trimmed.as_bytes();
        let mut in_string = false;
        let mut escape_next = false;

        for (i, &byte) in bytes.iter().enumerate().skip(start) {
            if escape_next {
                escape_next = false;
                continue;
            }

            match byte {
                b'\\' if in_string => escape_next = true,
                b'"' => in_string = !in_string,
                b'{' if !in_string => depth += 1,
                b'}' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        if let Some(end_idx) = end {
            let json_content = &trimmed[start..=end_idx];
            if serde_json::from_str::<serde_json::Value>(json_content).is_ok() {
                return Some(json_content.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ClaudeCodeAdapter basic tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_adapter_name() {
        let adapter = ClaudeCodeAdapter::new();
        assert_eq!(adapter.name(), "Claude Code");
    }

    #[test]
    fn test_is_available_returns_boolean() {
        let adapter = ClaudeCodeAdapter::new();
        let result = adapter.is_available();
        assert!(result == true || result == false);
    }

    // -----------------------------------------------------------------------
    // build_prompt tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_prompt_no_context() {
        let request = AgentRequest {
            prompt: "Hello".into(),
            ..Default::default()
        };
        assert_eq!(build_prompt(&request), "Hello");
    }

    #[test]
    fn test_build_prompt_empty_context() {
        let request = AgentRequest {
            prompt: "Hello".into(),
            context: Some("".into()),
            ..Default::default()
        };
        assert_eq!(build_prompt(&request), "Hello");
    }

    #[test]
    fn test_build_prompt_with_context() {
        let request = AgentRequest {
            prompt: "Summarize".into(),
            context: Some("some diff content".into()),
            ..Default::default()
        };
        let prompt = build_prompt(&request);
        assert!(prompt.contains("Context:"));
        assert!(prompt.contains("some diff content"));
        assert!(prompt.contains("Task: Summarize"));
    }

    // -----------------------------------------------------------------------
    // build_cli_args tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_cli_args_default() {
        let request = AgentRequest::default();
        let args = build_cli_args(&request);
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.contains(&"--permission-mode".to_string()));
        assert!(args.contains(&"bypassPermissions".to_string()));
        // No model, resume, max-turns, or json-schema
        assert!(!args.contains(&"--model".to_string()));
        assert!(!args.contains(&"--resume".to_string()));
        assert!(!args.contains(&"--max-turns".to_string()));
        assert!(!args.contains(&"--json-schema".to_string()));
    }

    #[test]
    fn test_build_cli_args_with_model_and_resume() {
        let request = AgentRequest {
            model: Some("opus".into()),
            resume_session: Some("sess-123".into()),
            max_turns: Some(5),
            ..Default::default()
        };
        let args = build_cli_args(&request);
        let model_idx = args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(args[model_idx + 1], "opus");
        let resume_idx = args.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(args[resume_idx + 1], "sess-123");
        let turns_idx = args.iter().position(|a| a == "--max-turns").unwrap();
        assert_eq!(args[turns_idx + 1], "5");
    }

    #[test]
    fn test_build_cli_args_with_output_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "result": {"type": "string"} }
        });
        let request = AgentRequest {
            output_schema: Some(schema.clone()),
            ..Default::default()
        };
        let args = build_cli_args(&request);
        let idx = args.iter().position(|a| a == "--json-schema").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&args[idx + 1]).unwrap();
        assert_eq!(parsed, schema);
    }

    // -----------------------------------------------------------------------
    // Message mapping tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_map_system_session_start() {
        use claude_agent_sdk_rs::SystemMessage;

        let msg = Message::System(SystemMessage {
            subtype: "session_start".into(),
            session_id: Some("sess-42".into()),
            model: Some("sonnet".into()),
            cwd: None,
            tools: None,
            mcp_servers: None,
            permission_mode: None,
            uuid: None,
            data: serde_json::Value::Null,
        });

        let events = map_message_to_events(msg);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::SessionStart { session_id, model } => {
                assert_eq!(session_id, "sess-42");
                assert_eq!(model.as_deref(), Some("sonnet"));
            }
            _ => panic!("expected SessionStart"),
        }
    }

    #[test]
    fn test_map_system_init_subtype() {
        use claude_agent_sdk_rs::SystemMessage;

        // Real CLI sends subtype "init" (not "session_start")
        let msg = Message::System(SystemMessage {
            subtype: "init".into(),
            session_id: Some("sess-init".into()),
            model: Some("opus".into()),
            cwd: None,
            tools: None,
            mcp_servers: None,
            permission_mode: None,
            uuid: None,
            data: serde_json::Value::Null,
        });

        let events = map_message_to_events(msg);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::SessionStart { session_id, model } => {
                assert_eq!(session_id, "sess-init");
                assert_eq!(model.as_deref(), Some("opus"));
            }
            _ => panic!("expected SessionStart for 'init' subtype"),
        }
    }

    #[test]
    fn test_map_assistant_text_only() {
        use claude_agent_sdk_rs::{AssistantMessage, AssistantMessageInner, TextBlock};

        let msg = Message::Assistant(AssistantMessage {
            message: AssistantMessageInner {
                content: vec![SdkContentBlock::Text(TextBlock {
                    text: "Hello, world!".into(),
                })],
                model: None,
                id: None,
                stop_reason: None,
                usage: None,
                error: None,
            },
            parent_tool_use_id: None,
            session_id: None,
            uuid: None,
        });

        let events = map_message_to_events(msg);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::AssistantMessage { content } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentBlock::Text { text } => assert_eq!(text, "Hello, world!"),
                    _ => panic!("expected Text block"),
                }
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn test_map_assistant_with_tool_use() {
        use claude_agent_sdk_rs::{AssistantMessage, AssistantMessageInner, ToolUseBlock};

        let msg = Message::Assistant(AssistantMessage {
            message: AssistantMessageInner {
                content: vec![SdkContentBlock::ToolUse(ToolUseBlock {
                    id: "tu_1".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({"path": "/tmp/foo"}),
                })],
                model: None,
                id: None,
                stop_reason: None,
                usage: None,
                error: None,
            },
            parent_tool_use_id: None,
            session_id: None,
            uuid: None,
        });

        let events = map_message_to_events(msg);
        // Only ToolUse event — no AssistantMessage since there are no text/thinking blocks
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::ToolUse { tool_name, input } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(input["path"], "/tmp/foo");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn test_map_user_with_tool_result() {
        use claude_agent_sdk_rs::{ToolResultBlock, UserMessage};

        let msg = Message::User(UserMessage {
            text: None,
            content: Some(vec![SdkContentBlock::ToolResult(ToolResultBlock {
                tool_use_id: "tu_1".into(),
                content: Some(ToolResultContent::Text("file contents".into())),
                is_error: Some(false),
            })]),
            uuid: None,
            parent_tool_use_id: None,
            extra: serde_json::Value::Null,
        });

        let events = map_message_to_events(msg);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::UserMessage { content } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentBlock::ToolResult {
                        output, is_error, ..
                    } => {
                        assert_eq!(output, "file contents");
                        assert!(!is_error);
                    }
                    _ => panic!("expected ToolResult block"),
                }
            }
            _ => panic!("expected UserMessage"),
        }
    }

    #[test]
    fn test_map_result_message_with_usage() {
        use claude_agent_sdk_rs::ResultMessage;

        let msg = Message::Result(ResultMessage {
            subtype: "query_complete".into(),
            duration_ms: 1500,
            duration_api_ms: 1200,
            is_error: false,
            num_turns: 3,
            session_id: "sess-abc".into(),
            total_cost_usd: Some(0.05),
            usage: Some(serde_json::json!({
                "input_tokens": 100,
                "output_tokens": 50
            })),
            result: None,
            structured_output: None,
        });

        let events = map_message_to_events(msg);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::Complete { session_id, usage } => {
                assert_eq!(session_id.as_deref(), Some("sess-abc"));
                assert_eq!(usage.input_tokens, Some(100));
                assert_eq!(usage.output_tokens, Some(50));
                assert_eq!(usage.duration_ms, 1500);
                assert_eq!(usage.duration_api_ms, Some(1200));
                assert_eq!(usage.num_turns, Some(3));
            }
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn test_map_result_message_with_structured_output() {
        use claude_agent_sdk_rs::ResultMessage;

        let structured = serde_json::json!({"title": "PR", "body": "desc"});
        let msg = Message::Result(ResultMessage {
            subtype: "query_complete".into(),
            duration_ms: 1000,
            duration_api_ms: 800,
            is_error: false,
            num_turns: 1,
            session_id: "sess-xyz".into(),
            total_cost_usd: None,
            usage: None,
            result: None,
            structured_output: Some(structured.clone()),
        });

        let events = map_message_to_events(msg);
        assert_eq!(events.len(), 2); // StructuredOutput + Complete
        match events[0].as_ref().unwrap() {
            AgentEvent::StructuredOutput { data } => {
                assert_eq!(data["title"], "PR");
            }
            _ => panic!("expected StructuredOutput"),
        }
        assert!(matches!(
            events[1].as_ref().unwrap(),
            AgentEvent::Complete { .. }
        ));
    }

    #[test]
    fn test_map_stream_event_text_delta() {
        let event = StreamEvent {
            uuid: "uuid-1".into(),
            session_id: "sess-1".into(),
            event: serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "text_delta",
                    "text": "Hello"
                }
            }),
            parent_tool_use_id: None,
        };

        let events = map_stream_event(&event);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::TextDelta { text } => assert_eq!(text, "Hello"),
            _ => panic!("expected TextDelta"),
        }
    }

    #[test]
    fn test_map_stream_event_non_text_ignored() {
        let event = StreamEvent {
            uuid: "uuid-1".into(),
            session_id: "sess-1".into(),
            event: serde_json::json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
            parent_tool_use_id: None,
        };

        let events = map_stream_event(&event);
        assert!(events.is_empty());
    }

    // -----------------------------------------------------------------------
    // extract_tool_result_content tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_tool_result_text() {
        let content = Some(ToolResultContent::Text("output".into()));
        assert_eq!(extract_tool_result_content(&content), "output");
    }

    #[test]
    fn test_extract_tool_result_blocks() {
        let content = Some(ToolResultContent::Blocks(vec![
            serde_json::json!({"text": "line1"}),
            serde_json::json!({"text": "line2"}),
        ]));
        assert_eq!(extract_tool_result_content(&content), "line1\nline2");
    }

    #[test]
    fn test_extract_tool_result_none() {
        assert_eq!(extract_tool_result_content(&None), "");
    }

    // -----------------------------------------------------------------------
    // extract_usage_tokens tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_usage_tokens_present() {
        let usage = Some(serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 50
        }));
        let (input, output) = extract_usage_tokens(&usage);
        assert_eq!(input, Some(100));
        assert_eq!(output, Some(50));
    }

    #[test]
    fn test_extract_usage_tokens_none() {
        let (input, output) = extract_usage_tokens(&None);
        assert!(input.is_none());
        assert!(output.is_none());
    }

    // -----------------------------------------------------------------------
    // try_extract_json tests (preserved from original)
    // -----------------------------------------------------------------------

    #[test]
    fn test_try_extract_json_raw_json() {
        let json = r#"{"title": "Test", "body": "Hello"}"#;
        let result = try_extract_json(json);
        assert_eq!(result, Some(json.to_string()));
    }

    #[test]
    fn test_try_extract_json_with_whitespace() {
        let json = r#"  {"title": "Test", "body": "Hello"}  "#;
        let result = try_extract_json(json);
        assert!(result.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(parsed["title"], "Test");
    }

    #[test]
    fn test_try_extract_json_in_markdown_code_block() {
        let text = r#"Here's the JSON:
```json
{"title": "Test PR", "body": "This is a test"}
```"#;
        let result = try_extract_json(text);
        assert!(result.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(parsed["title"], "Test PR");
    }

    #[test]
    fn test_try_extract_json_in_plain_code_block() {
        let text = r#"Here's the JSON:
```
{"title": "Test PR", "body": "This is a test"}
```"#;
        let result = try_extract_json(text);
        assert!(result.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(parsed["title"], "Test PR");
    }

    #[test]
    fn test_try_extract_json_embedded_in_text() {
        let text = r#"I'll generate the PR description:

{"title": "Fix login bug", "body": "This PR fixes a bug in the login flow"}

Let me know if you need changes."#;
        let result = try_extract_json(text);
        assert!(result.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(parsed["title"], "Fix login bug");
    }

    #[test]
    fn test_try_extract_json_with_nested_braces() {
        let text = r#"{"outer": {"inner": "value"}, "list": [1, 2, 3]}"#;
        let result = try_extract_json(text);
        assert!(result.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(parsed["outer"]["inner"], "value");
    }

    #[test]
    fn test_try_extract_json_with_escaped_quotes() {
        let text = r#"{"title": "Fix \"login\" bug", "body": "Test"}"#;
        let result = try_extract_json(text);
        assert!(result.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(parsed["title"], "Fix \"login\" bug");
    }

    #[test]
    fn test_try_extract_json_no_json() {
        let text = "This is just plain text with no JSON content.";
        let result = try_extract_json(text);
        assert!(result.is_none());
    }

    #[test]
    fn test_try_extract_json_array() {
        let json = r#"[{"name": "item1"}, {"name": "item2"}]"#;
        let result = try_extract_json(json);
        assert_eq!(result, Some(json.to_string()));
    }

    // -----------------------------------------------------------------------
    // Unknown variant / rate_limit_event E2E tests
    // -----------------------------------------------------------------------

    /// Simulate JSONL output with a rate_limit_event interspersed.
    /// This reproduces the bug where the SDK's query_stream() would
    /// terminate the entire stream on unknown message variants.
    #[tokio::test]
    async fn test_stream_continues_past_unknown_variant() {
        use futures::StreamExt;

        // Build a JSONL payload that includes:
        // 1. A valid system message (session_start)
        // 2. An unknown variant (rate_limit_event) — should be skipped
        // 3. A valid assistant message — should still be received
        // 4. A valid result message — should complete the stream
        let jsonl = [
            // system session_start
            r#"{"type":"system","subtype":"session_start","session_id":"sess-test","model":"sonnet","tools":null,"mcp_servers":null,"cwd":null,"permission_mode":null,"uuid":null,"data":null}"#,
            // unknown variant — this killed the SDK stream
            r#"{"type":"rate_limit_event","data":{"retry_after":5}}"#,
            // assistant message
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}],"model":null,"id":null,"stop_reason":null,"usage":null,"error":null},"parent_tool_use_id":null,"session_id":null,"uuid":null}"#,
            // result message
            r#"{"type":"result","subtype":"query_complete","duration_ms":1000,"duration_api_ms":800,"is_error":false,"num_turns":1,"session_id":"sess-test","total_cost_usd":0.01,"usage":{"input_tokens":100,"output_tokens":50},"result":null,"structured_output":null}"#,
        ]
        .join("\n");

        // Spawn a helper process that echoes the JSONL to stdout
        let mut child = tokio::process::Command::new("printf")
            .arg(&format!("{}\n", jsonl))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap();

        let stdout = child.stdout.take().unwrap();
        let reader = tokio::io::BufReader::new(stdout);
        let stream = claude_code_event_stream(reader, child);

        // Collect all events
        let events: Vec<_> = stream
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        // Verify we got events AFTER the unknown variant
        assert!(
            events.len() >= 3,
            "Expected at least 3 events (session_start, assistant, complete), got {}",
            events.len()
        );

        // First event should be SessionStart
        assert!(
            matches!(&events[0], AgentEvent::SessionStart { session_id, .. } if session_id == "sess-test"),
            "Expected SessionStart, got {:?}",
            events[0]
        );

        // Should have an AssistantMessage with "Hello!"
        let has_assistant = events.iter().any(|e| {
            matches!(e, AgentEvent::AssistantMessage { content } if content.iter().any(|b| {
                matches!(b, ContentBlock::Text { text } if text == "Hello!")
            }))
        });
        assert!(has_assistant, "Expected AssistantMessage with 'Hello!'");

        // Should have a Complete event
        let has_complete = events
            .iter()
            .any(|e| matches!(e, AgentEvent::Complete { .. }));
        assert!(has_complete, "Expected Complete event");
    }

    /// Verify that the stream works end-to-end with StreamCollector
    /// when unknown variants are present — the exact scenario that was failing.
    #[tokio::test]
    async fn test_stream_collector_survives_unknown_variant() {
        let jsonl = [
            r#"{"type":"system","subtype":"session_start","session_id":"sess-42","model":"sonnet","tools":null,"mcp_servers":null,"cwd":null,"permission_mode":null,"uuid":null,"data":null}"#,
            r#"{"type":"rate_limit_event","data":{"retry_after":3}}"#,
            r#"{"type":"another_unknown_event","foo":"bar"}"#,
            r#"{"type":"result","subtype":"query_complete","duration_ms":500,"duration_api_ms":400,"is_error":false,"num_turns":1,"session_id":"sess-42","total_cost_usd":0.005,"usage":{"input_tokens":50,"output_tokens":25},"result":null,"structured_output":null}"#,
        ]
        .join("\n");

        let mut child = tokio::process::Command::new("printf")
            .arg(&format!("{}\n", jsonl))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap();

        let stdout = child.stdout.take().unwrap();
        let reader = tokio::io::BufReader::new(stdout);
        let stream = claude_code_event_stream(reader, child);

        // This previously failed with:
        // "Agent error: SDK stream error: Message parse error: ... unknown variant `rate_limit_event`"
        let result = super::super::StreamCollector::collect(stream)
            .await
            .expect("StreamCollector should succeed even with unknown message variants");

        assert_eq!(result.session_id, Some("sess-42".into()));
        assert_eq!(result.usage.input_tokens, Some(50));
        assert_eq!(result.usage.output_tokens, Some(25));
    }

    // -----------------------------------------------------------------------
    // Integration tests (require Claude Code CLI)
    // -----------------------------------------------------------------------

    #[tokio::test]
    #[ignore = "requires Claude Code CLI"]
    async fn test_run_stream_integration() {
        let adapter = ClaudeCodeAdapter::new();
        if !adapter.is_available() {
            return;
        }

        let request = AgentRequest {
            prompt: "What is 2 + 2? Reply with just the number.".into(),
            ..Default::default()
        };

        let stream = adapter.run(&request).await.unwrap();
        let result = super::super::StreamCollector::collect(stream)
            .await
            .unwrap();
        assert!(!result.content.is_empty());
    }
}
