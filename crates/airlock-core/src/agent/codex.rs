//! Codex agent adapter.
//!
//! Implements [`AgentAdapter`] by spawning `codex exec` as a subprocess and
//! mapping its JSONL streaming events to [`AgentEvent`] values.
//!
//! ## Codex JSONL event format
//!
//! When invoked with `--json`, `codex exec` emits newline-delimited JSON events:
//!
//! - `thread.started` — session initialization with `thread_id`
//! - `turn.started` — beginning of an agentic turn
//! - `turn.completed` — turn finished, includes `usage` with token counts
//! - `turn.failed` — turn encountered an error
//! - `item.started` — an item (message, tool call, etc.) begins
//! - `item.completed` — an item finishes
//! - `item.content_text.delta` — incremental text from the assistant
//! - `error` — an error event

use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Instant;
use tracing::debug;

use super::subprocess::{is_cli_available, parse_jsonl_line, SubprocessReader};
use super::types::{AgentEvent, AgentEventStream, AgentRequest, AgentUsage, ContentBlock};
use super::AgentAdapter;
use crate::error::{AirlockError, Result};

/// Codex adapter that spawns `codex exec` as a subprocess.
#[derive(Default)]
pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "Codex"
    }

    fn is_available(&self) -> bool {
        is_cli_available("codex")
    }

    async fn run(&self, request: &AgentRequest) -> Result<AgentEventStream> {
        if !self.is_available() {
            return Err(AirlockError::Agent(
                "Codex CLI not found.\n\n\
                 To use Codex with Airlock, you need:\n\
                 1. Install Codex: https://github.com/openai/codex\n\
                 2. Log in by running: codex"
                    .to_string(),
            ));
        }

        let args = build_args(request)?;
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        debug!(
            "Starting Codex stream (cwd: {:?}): {} args",
            request.cwd,
            args.len()
        );

        let reader = SubprocessReader::spawn("codex", &arg_refs, request.cwd.as_deref())?;

        Ok(codex_event_stream(reader))
    }
}

/// Build the command-line arguments for `codex exec`.
fn build_args(request: &AgentRequest) -> Result<Vec<String>> {
    let mut args = vec!["exec".to_string(), "--json".to_string()];

    args.push("--dangerously-bypass-approvals-and-sandbox".to_string());

    // Working directory
    if let Some(cwd) = &request.cwd {
        args.push("-C".to_string());
        args.push(cwd.to_string_lossy().to_string());
    }

    // Model override
    if let Some(model) = &request.model {
        args.push("-m".to_string());
        args.push(model.clone());
    }

    // Structured output via --output-schema temp file
    if let Some(schema) = &request.output_schema {
        let schema_path = write_temp_schema(schema)?;
        args.push("--output-schema".to_string());
        args.push(schema_path.to_string_lossy().to_string());
    }

    // Build the prompt, combining context if present
    args.push(request.full_prompt());

    Ok(args)
}

// build_prompt is now AgentRequest::full_prompt() in types.rs

/// Write a JSON schema to a temporary file and return the path.
///
/// The schema is normalized first: `"additionalProperties": false` is added
/// to every object-type schema node, which the OpenAI API requires for
/// structured outputs.
fn write_temp_schema(schema: &serde_json::Value) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("airlock-codex");
    std::fs::create_dir_all(&dir)?;

    let mut normalized = schema.clone();
    normalize_schema_for_openai(&mut normalized);

    let file_path = dir.join(format!("schema-{}.json", uuid::Uuid::new_v4()));
    let content = serde_json::to_string_pretty(&normalized)?;
    std::fs::write(&file_path, content)?;

    debug!("Wrote output schema to {:?}", file_path);
    Ok(file_path)
}

/// Recursively normalize a JSON schema for the OpenAI structured outputs API.
///
/// The OpenAI API requires:
/// 1. `"additionalProperties": false` on every object-type node.
/// 2. `"required"` must list **all** keys in `"properties"`.
fn normalize_schema_for_openai(schema: &mut serde_json::Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };

    // If this node is an object-type schema, enforce OpenAI requirements.
    if obj.get("type").and_then(|t| t.as_str()) == Some("object") {
        obj.entry("additionalProperties")
            .or_insert(serde_json::Value::Bool(false));

        // Ensure `required` includes every property key.
        if let Some(serde_json::Value::Object(props)) = obj.get("properties") {
            let all_keys: Vec<serde_json::Value> = props
                .keys()
                .map(|k| serde_json::Value::String(k.clone()))
                .collect();
            obj.insert("required".to_string(), serde_json::Value::Array(all_keys));
        }
    }

    // Recurse into `properties` values.
    if let Some(serde_json::Value::Object(props)) = obj.get_mut("properties") {
        for prop in props.values_mut() {
            normalize_schema_for_openai(prop);
        }
    }

    // Recurse into `items` (array items).
    if let Some(items) = obj.get_mut("items") {
        normalize_schema_for_openai(items);
    }
}

/// Accumulation state for tracking usage across the Codex event stream.
///
/// Separated from the subprocess reader so it can be used in unit tests
/// without spawning a subprocess.
struct CodexAccumulator {
    start_time: Instant,
    num_turns: u32,
    total_input_tokens: Option<u64>,
    total_output_tokens: Option<u64>,
}

impl CodexAccumulator {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            num_turns: 0,
            total_input_tokens: None,
            total_output_tokens: None,
        }
    }

    fn build_complete_usage(&self) -> AgentUsage {
        AgentUsage {
            input_tokens: self.total_input_tokens,
            output_tokens: self.total_output_tokens,
            duration_ms: self.start_time.elapsed().as_millis() as u64,
            duration_api_ms: None,
            num_turns: if self.num_turns > 0 {
                Some(self.num_turns)
            } else {
                None
            },
            raw: None,
        }
    }
}

/// Create an `AgentEventStream` from a subprocess reader.
///
/// Uses `futures::stream::unfold` to read JSONL lines from the subprocess
/// and map them to `AgentEvent` values, emitting a `Complete` event when
/// the subprocess exits.
fn codex_event_stream(reader: SubprocessReader) -> AgentEventStream {
    let acc = CodexAccumulator::new();
    let pending: Vec<Result<AgentEvent>> = Vec::new();

    let stream = futures::stream::unfold(
        (reader, acc, pending, false),
        |(mut reader, mut acc, mut pending, completed)| async move {
            if completed {
                return None;
            }

            // Drain pending events first
            if !pending.is_empty() {
                let event = pending.remove(0);
                return Some((event, (reader, acc, pending, false)));
            }

            loop {
                match reader.next_line().await {
                    Err(e) => return Some((Err(e), (reader, acc, pending, true))),
                    Ok(None) => {
                        // Subprocess exited — emit Complete event and mark stream as done
                        let usage = acc.build_complete_usage();
                        return Some((
                            Ok(AgentEvent::Complete {
                                session_id: None,
                                usage,
                            }),
                            (reader, acc, pending, true),
                        ));
                    }
                    Ok(Some(line)) => match parse_jsonl_line(&line) {
                        Ok(None) => continue,
                        Ok(Some(json)) => {
                            let mut events = map_codex_event(&json, &mut acc);
                            if events.is_empty() {
                                continue;
                            }
                            let first = events.remove(0);
                            pending = events;
                            return Some((first, (reader, acc, pending, false)));
                        }
                        Err(e) => {
                            return Some((
                                Ok(AgentEvent::Error {
                                    message: format!("JSONL parse error: {e}"),
                                    is_fatal: false,
                                }),
                                (reader, acc, pending, false),
                            ));
                        }
                    },
                }
            }
        },
    );

    Box::pin(stream)
}

/// Map a single Codex JSONL event to one or more [`AgentEvent`] values.
///
/// Updates the stream state (turn count, token usage) as a side effect.
fn map_codex_event(
    json: &serde_json::Value,
    state: &mut CodexAccumulator,
) -> Vec<Result<AgentEvent>> {
    let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match event_type {
        "thread.started" => {
            let thread_id = json
                .get("thread_id")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown")
                .to_string();
            vec![Ok(AgentEvent::SessionStart {
                session_id: thread_id,
                model: None,
            })]
        }

        "turn.started" => {
            // Internal bookkeeping only — no event emitted
            vec![]
        }

        "turn.completed" => {
            state.num_turns += 1;

            // Extract token usage if present
            if let Some(usage) = json.get("usage") {
                if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                    state.total_input_tokens = Some(state.total_input_tokens.unwrap_or(0) + input);
                }
                // Also count cached input tokens
                if let Some(cached) = usage.get("cached_input_tokens").and_then(|v| v.as_u64()) {
                    state.total_input_tokens = Some(state.total_input_tokens.unwrap_or(0) + cached);
                }
                if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                    state.total_output_tokens =
                        Some(state.total_output_tokens.unwrap_or(0) + output);
                }
            }

            // Emit a usage update event
            let duration_ms = state.start_time.elapsed().as_millis() as u64;
            vec![Ok(AgentEvent::Usage(AgentUsage {
                input_tokens: state.total_input_tokens,
                output_tokens: state.total_output_tokens,
                duration_ms,
                duration_api_ms: None,
                num_turns: Some(state.num_turns),
                raw: json.get("usage").cloned(),
            }))]
        }

        "turn.failed" => {
            let error_msg = json
                .get("error")
                .and_then(|e| e.as_str())
                .or_else(|| json.get("message").and_then(|m| m.as_str()))
                .unwrap_or("Turn failed")
                .to_string();
            vec![Ok(AgentEvent::Error {
                message: error_msg,
                is_fatal: false,
            })]
        }

        "item.content_text.delta" => {
            // Incremental text from the assistant
            if let Some(delta) = json.get("delta").and_then(|d| d.as_str()) {
                vec![Ok(AgentEvent::TextDelta {
                    text: delta.to_string(),
                })]
            } else {
                vec![]
            }
        }

        "item.started" => {
            let item = json.get("item").unwrap_or(json);
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match item_type {
                "command_execution" => {
                    let command = item
                        .get("command")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    vec![Ok(AgentEvent::ToolUse {
                        tool_name: "command_execution".to_string(),
                        input: serde_json::json!({ "command": command }),
                    })]
                }
                "mcp_tool_call" => {
                    let tool_name = item
                        .get("tool_name")
                        .or_else(|| item.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("mcp_tool")
                        .to_string();
                    let input = item
                        .get("input")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    vec![Ok(AgentEvent::ToolUse { tool_name, input })]
                }
                "file_change" => {
                    let path = item
                        .get("path")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string();
                    let change_type = item
                        .get("change_type")
                        .and_then(|c| c.as_str())
                        .unwrap_or("modify")
                        .to_string();
                    vec![Ok(AgentEvent::ToolUse {
                        tool_name: "file_change".to_string(),
                        input: serde_json::json!({
                            "path": path,
                            "change_type": change_type,
                        }),
                    })]
                }
                _ => vec![],
            }
        }

        "item.completed" => {
            let item = json.get("item").unwrap_or(json);
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match item_type {
                "agent_message" => {
                    let text = item
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Check if this looks like structured output
                    if !text.is_empty() {
                        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                            if data.is_object() || data.is_array() {
                                // Could be structured output — emit both
                                return vec![
                                    Ok(AgentEvent::AssistantMessage {
                                        content: vec![ContentBlock::Text { text: text.clone() }],
                                    }),
                                    Ok(AgentEvent::StructuredOutput { data }),
                                ];
                            }
                        }
                    }

                    if !text.is_empty() {
                        vec![Ok(AgentEvent::AssistantMessage {
                            content: vec![ContentBlock::Text { text }],
                        })]
                    } else {
                        vec![]
                    }
                }
                "command_execution" => {
                    let output = item
                        .get("output")
                        .and_then(|o| o.as_str())
                        .unwrap_or("")
                        .to_string();
                    let exit_code = item.get("exit_code").and_then(|c| c.as_i64()).unwrap_or(0);
                    vec![Ok(AgentEvent::ToolResult {
                        tool_name: "command_execution".to_string(),
                        output,
                        is_error: exit_code != 0,
                    })]
                }
                "mcp_tool_call" => {
                    let tool_name = item
                        .get("tool_name")
                        .or_else(|| item.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("mcp_tool")
                        .to_string();
                    let output = item
                        .get("output")
                        .and_then(|o| o.as_str())
                        .unwrap_or("")
                        .to_string();
                    let is_error = item
                        .get("is_error")
                        .and_then(|e| e.as_bool())
                        .unwrap_or(false);
                    vec![Ok(AgentEvent::ToolResult {
                        tool_name,
                        output,
                        is_error,
                    })]
                }
                "file_change" => {
                    let path = item
                        .get("path")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string();
                    vec![Ok(AgentEvent::ToolResult {
                        tool_name: "file_change".to_string(),
                        output: format!("File changed: {path}"),
                        is_error: false,
                    })]
                }
                _ => vec![],
            }
        }

        "error" => {
            let message = json
                .get("message")
                .and_then(|m| m.as_str())
                .or_else(|| json.get("error").and_then(|e| e.as_str()))
                .unwrap_or("Unknown error")
                .to_string();
            vec![Ok(AgentEvent::Error {
                message,
                is_fatal: true,
            })]
        }

        _ => {
            debug!("Unrecognized Codex event type: {}", event_type);
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // CodexAdapter basic tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_adapter_name() {
        let adapter = CodexAdapter::new();
        assert_eq!(adapter.name(), "Codex");
    }

    #[test]
    fn test_is_available_returns_boolean() {
        let adapter = CodexAdapter::new();
        let result = adapter.is_available();
        // Just verify it returns without panicking
        assert!(result == true || result == false);
    }

    // build_prompt tests moved to types.rs (AgentRequest::full_prompt)

    // -----------------------------------------------------------------------
    // build_args tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_args_basic() {
        let request = AgentRequest {
            prompt: "Do something".into(),
            ..Default::default()
        };
        let args = build_args(&request).unwrap();
        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
        assert_eq!(args.last().unwrap(), "Do something");
    }

    #[test]
    fn test_build_args_with_cwd() {
        let request = AgentRequest {
            prompt: "task".into(),
            cwd: Some("/test/dir".into()),
            ..Default::default()
        };
        let args = build_args(&request).unwrap();
        let c_idx = args.iter().position(|a| a == "-C").unwrap();
        assert_eq!(args[c_idx + 1], "/test/dir");
    }

    #[test]
    fn test_build_args_with_model() {
        let request = AgentRequest {
            prompt: "task".into(),
            model: Some("o3".into()),
            ..Default::default()
        };
        let args = build_args(&request).unwrap();
        let m_idx = args.iter().position(|a| a == "-m").unwrap();
        assert_eq!(args[m_idx + 1], "o3");
    }

    #[test]
    fn test_build_args_with_output_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "result": { "type": "string" } }
        });
        let request = AgentRequest {
            prompt: "task".into(),
            output_schema: Some(schema),
            ..Default::default()
        };
        let args = build_args(&request).unwrap();
        assert!(args.contains(&"--output-schema".to_string()));
        // Schema file should have been written
        let schema_idx = args.iter().position(|a| a == "--output-schema").unwrap();
        let schema_path = &args[schema_idx + 1];
        assert!(schema_path.contains("airlock-codex"));
        assert!(schema_path.ends_with(".json"));
        // Clean up
        let _ = std::fs::remove_file(schema_path);
    }

    // -----------------------------------------------------------------------
    // normalize_schema_for_openai tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ensure_additional_properties_added_to_root_object() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });
        normalize_schema_for_openai(&mut schema);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn test_ensure_additional_properties_preserves_existing() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true
        });
        normalize_schema_for_openai(&mut schema);
        // Should not overwrite an existing value
        assert_eq!(schema["additionalProperties"], true);
    }

    #[test]
    fn test_ensure_additional_properties_recurses_into_nested_objects() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "inner": {
                    "type": "object",
                    "properties": {
                        "val": { "type": "string" }
                    }
                }
            }
        });
        normalize_schema_for_openai(&mut schema);
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["inner"]["additionalProperties"], false);
    }

    #[test]
    fn test_ensure_additional_properties_recurses_into_array_items() {
        let mut schema = serde_json::json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" }
                }
            }
        });
        normalize_schema_for_openai(&mut schema);
        assert_eq!(schema["items"]["additionalProperties"], false);
        let required = schema["items"]["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("id")));
    }

    #[test]
    fn test_ensure_additional_properties_noop_for_non_object() {
        let mut schema = serde_json::json!({ "type": "string" });
        let original = schema.clone();
        normalize_schema_for_openai(&mut schema);
        assert_eq!(schema, original);
    }

    #[test]
    fn test_normalize_required_includes_all_properties() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "verdict": { "type": "string" },
                "summary": { "type": "string" },
                "details": { "type": "string" }
            },
            "required": ["verdict", "summary"]
        });
        normalize_schema_for_openai(&mut schema);
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 3);
        assert!(required.contains(&serde_json::json!("verdict")));
        assert!(required.contains(&serde_json::json!("summary")));
        assert!(required.contains(&serde_json::json!("details")));
    }

    #[test]
    fn test_normalize_adds_required_when_missing() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });
        normalize_schema_for_openai(&mut schema);
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert!(required.contains(&serde_json::json!("name")));
    }

    // -----------------------------------------------------------------------
    // Event mapping tests
    // -----------------------------------------------------------------------

    fn make_state() -> CodexAccumulator {
        CodexAccumulator::new()
    }

    #[test]
    fn test_map_thread_started() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "thread.started",
            "thread_id": "t-123"
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::SessionStart { session_id, model } => {
                assert_eq!(session_id, "t-123");
                assert!(model.is_none());
            }
            _ => panic!("expected SessionStart"),
        }
    }

    #[test]
    fn test_map_turn_started_empty() {
        let mut state = make_state();
        let json = serde_json::json!({ "type": "turn.started" });
        let events = map_codex_event(&json, &mut state);
        assert!(events.is_empty());
    }

    #[test]
    fn test_map_turn_completed_with_usage() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 100,
                "cached_input_tokens": 50,
                "output_tokens": 25
            }
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(state.num_turns, 1);
        assert_eq!(state.total_input_tokens, Some(150)); // 100 + 50 cached
        assert_eq!(state.total_output_tokens, Some(25));

        match events[0].as_ref().unwrap() {
            AgentEvent::Usage(usage) => {
                assert_eq!(usage.input_tokens, Some(150));
                assert_eq!(usage.output_tokens, Some(25));
                assert_eq!(usage.num_turns, Some(1));
            }
            _ => panic!("expected Usage"),
        }
    }

    #[test]
    fn test_map_turn_completed_accumulates_tokens() {
        let mut state = make_state();

        // First turn
        let json1 = serde_json::json!({
            "type": "turn.completed",
            "usage": { "input_tokens": 100, "output_tokens": 50 }
        });
        map_codex_event(&json1, &mut state);

        // Second turn
        let json2 = serde_json::json!({
            "type": "turn.completed",
            "usage": { "input_tokens": 200, "output_tokens": 75 }
        });
        let events = map_codex_event(&json2, &mut state);

        assert_eq!(state.num_turns, 2);
        assert_eq!(state.total_input_tokens, Some(300));
        assert_eq!(state.total_output_tokens, Some(125));

        match events[0].as_ref().unwrap() {
            AgentEvent::Usage(usage) => {
                assert_eq!(usage.input_tokens, Some(300));
                assert_eq!(usage.output_tokens, Some(125));
                assert_eq!(usage.num_turns, Some(2));
            }
            _ => panic!("expected Usage"),
        }
    }

    #[test]
    fn test_map_turn_failed() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "turn.failed",
            "error": "Rate limited"
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::Error { message, is_fatal } => {
                assert_eq!(message, "Rate limited");
                assert!(!is_fatal);
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_map_text_delta() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.content_text.delta",
            "delta": "Hello, world!"
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::TextDelta { text } => {
                assert_eq!(text, "Hello, world!");
            }
            _ => panic!("expected TextDelta"),
        }
    }

    #[test]
    fn test_map_item_started_command_execution() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.started",
            "item": {
                "id": "item_1",
                "type": "command_execution",
                "command": "bash -lc ls",
                "status": "in_progress"
            }
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::ToolUse { tool_name, input } => {
                assert_eq!(tool_name, "command_execution");
                assert_eq!(input["command"], "bash -lc ls");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn test_map_item_started_mcp_tool_call() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.started",
            "item": {
                "id": "item_2",
                "type": "mcp_tool_call",
                "tool_name": "read_file",
                "input": { "path": "/tmp/foo" }
            }
        });
        let events = map_codex_event(&json, &mut state);
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
    fn test_map_item_started_file_change() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.started",
            "item": {
                "id": "item_3",
                "type": "file_change",
                "path": "src/main.rs",
                "change_type": "modify"
            }
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::ToolUse { tool_name, input } => {
                assert_eq!(tool_name, "file_change");
                assert_eq!(input["path"], "src/main.rs");
                assert_eq!(input["change_type"], "modify");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn test_map_item_completed_agent_message() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item_5",
                "type": "agent_message",
                "text": "The repo has 3 directories."
            }
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::AssistantMessage { content } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentBlock::Text { text } => {
                        assert_eq!(text, "The repo has 3 directories.");
                    }
                    _ => panic!("expected Text block"),
                }
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn test_map_item_completed_agent_message_structured_json() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item_6",
                "type": "agent_message",
                "text": "{\"title\": \"My PR\", \"body\": \"description\"}"
            }
        });
        let events = map_codex_event(&json, &mut state);
        // Should produce AssistantMessage + StructuredOutput
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0].as_ref().unwrap(),
            AgentEvent::AssistantMessage { .. }
        ));
        match events[1].as_ref().unwrap() {
            AgentEvent::StructuredOutput { data } => {
                assert_eq!(data["title"], "My PR");
            }
            _ => panic!("expected StructuredOutput"),
        }
    }

    #[test]
    fn test_map_item_completed_command_execution() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item_7",
                "type": "command_execution",
                "command": "ls",
                "output": "file1.txt\nfile2.txt",
                "exit_code": 0
            }
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::ToolResult {
                tool_name,
                output,
                is_error,
            } => {
                assert_eq!(tool_name, "command_execution");
                assert_eq!(output, "file1.txt\nfile2.txt");
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn test_map_item_completed_command_execution_error() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item_8",
                "type": "command_execution",
                "command": "false",
                "output": "",
                "exit_code": 1
            }
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::ToolResult { is_error, .. } => {
                assert!(is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn test_map_item_completed_file_change() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item_9",
                "type": "file_change",
                "path": "src/lib.rs"
            }
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::ToolResult {
                tool_name,
                output,
                is_error,
            } => {
                assert_eq!(tool_name, "file_change");
                assert!(output.contains("src/lib.rs"));
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn test_map_error_event() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "error",
            "message": "API key invalid"
        });
        let events = map_codex_event(&json, &mut state);
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            AgentEvent::Error { message, is_fatal } => {
                assert_eq!(message, "API key invalid");
                assert!(is_fatal);
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_map_unknown_event_type() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "some.future.event",
            "data": "irrelevant"
        });
        let events = map_codex_event(&json, &mut state);
        assert!(events.is_empty());
    }

    #[test]
    fn test_map_text_delta_empty() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.content_text.delta"
            // no "delta" field
        });
        let events = map_codex_event(&json, &mut state);
        assert!(events.is_empty());
    }

    #[test]
    fn test_map_item_completed_empty_agent_message() {
        let mut state = make_state();
        let json = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item_10",
                "type": "agent_message",
                "text": ""
            }
        });
        let events = map_codex_event(&json, &mut state);
        assert!(events.is_empty());
    }

    // -----------------------------------------------------------------------
    // Stream termination tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_stream_terminates_after_eof() {
        use futures::StreamExt;

        // Spawn a subprocess that emits codex-like JSONL then exits.
        let jsonl = r#"{"type":"thread.started","thread_id":"t-1"}
{"type":"item.content_text.delta","delta":"hi"}
{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}"#;

        let reader = SubprocessReader::spawn("printf", &["%s", jsonl], None).unwrap();
        let mut stream = codex_event_stream(reader);

        let mut events = Vec::new();
        // Use a timeout to guarantee we don't hang if the bug regresses.
        let collect_future = async {
            while let Some(event_result) = stream.next().await {
                events.push(event_result.unwrap());
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), collect_future)
            .await
            .expect("stream should terminate, not loop forever");

        // Exactly one Complete event at the end
        let complete_count = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::Complete { .. }))
            .count();
        assert_eq!(
            complete_count, 1,
            "expected exactly 1 Complete, got {complete_count}"
        );
        assert!(
            matches!(events.last().unwrap(), AgentEvent::Complete { .. }),
            "last event should be Complete"
        );
    }

    #[tokio::test]
    async fn test_stream_terminates_on_immediate_eof() {
        use futures::StreamExt;

        // Subprocess that exits immediately with no output.
        let reader = SubprocessReader::spawn("true", &[], None).unwrap();
        let mut stream = codex_event_stream(reader);

        let mut events = Vec::new();
        let collect_future = async {
            while let Some(event_result) = stream.next().await {
                events.push(event_result.unwrap());
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), collect_future)
            .await
            .expect("stream should terminate on immediate EOF");

        assert_eq!(events.len(), 1, "expected exactly 1 event (Complete)");
        assert!(matches!(events[0], AgentEvent::Complete { .. }));
    }

    // -----------------------------------------------------------------------
    // Integration tests (require Codex CLI)
    // -----------------------------------------------------------------------

    #[tokio::test]
    #[ignore = "requires Codex CLI"]
    async fn test_run_stream_integration() {
        let adapter = CodexAdapter::new();
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

    #[tokio::test]
    #[ignore = "requires Codex CLI"]
    async fn test_codex_stream_terminates_e2e() {
        use futures::StreamExt;

        let adapter = CodexAdapter::new();
        if !adapter.is_available() {
            return;
        }

        let request = AgentRequest {
            prompt: "What is 2 + 2? Reply with just the number.".into(),
            ..Default::default()
        };

        let mut stream = adapter.run(&request).await.unwrap();

        let mut events = Vec::new();
        let collect_future = async {
            while let Some(event_result) = stream.next().await {
                events.push(event_result.unwrap());
            }
        };
        // Codex should finish quickly for a trivial prompt
        tokio::time::timeout(std::time::Duration::from_secs(60), collect_future)
            .await
            .expect("codex stream should terminate, not loop forever");

        let complete_count = events
            .iter()
            .filter(|e| matches!(e, AgentEvent::Complete { .. }))
            .count();
        assert_eq!(
            complete_count, 1,
            "expected exactly 1 Complete event, got {complete_count}"
        );
        assert!(
            matches!(events.last().unwrap(), AgentEvent::Complete { .. }),
            "last event should be Complete"
        );
    }
}
