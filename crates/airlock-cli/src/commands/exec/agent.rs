//! Agent stage implementation - run prompts through an agent CLI.
//!
//! Usage:
//!   airlock exec agent "Generate a PR description for this diff"
//!   airlock exec agent "Extract info" --output-schema '{"type": "object", ...}'
//!   airlock exec agent "Extract info" --output-schema schema.json
//!   airlock exec agent "Summarize" --adapter codex
//!   git diff "$AIRLOCK_BASE_SHA" "$AIRLOCK_HEAD_SHA" | airlock exec agent "Summarize"
//!
//! Streaming is the only output mode. `JSONL` [`AgentEvent`] lines are written to
//! **stderr** for real-time observability. The final result (text or structured
//! JSON) is written to **stdout** only after the stream completes.

use airlock_core::{
    create_adapter, load_global_config, resolve_agent_adapter_name, resolve_global_config_path,
    AgentEvent, AgentOptions, AgentRequest, StreamCollector,
};
use anyhow::{Context, Result};
use futures::StreamExt;
use std::io::{self, Read, Write};
use std::path::Path;
use tracing::{debug, info};

/// Execute the `agent` command.
///
/// Runs a prompt through an agent CLI with optional stdin context.
/// Streams JSONL events to stderr and writes the final result to stdout.
pub async fn agent(
    prompt: String,
    output_schema: Option<String>,
    cli_adapter: Option<String>,
) -> Result<()> {
    info!("Running agent with prompt...");

    // Resolve adapter name and create it
    let adapter_name = resolve_agent_adapter_name(cli_adapter.as_deref(), None);
    debug!("Using agent adapter: {}", adapter_name);

    let adapter = create_adapter(&adapter_name)
        .context(format!("Failed to create adapter '{adapter_name}'"))?;

    // Check if agent is available
    if !adapter.is_available() {
        anyhow::bail!(
            "{} CLI not found.\n\n\
             To use the agent command, install one of the supported agent CLIs:\n\
             - Claude Code: https://claude.ai/code\n\
             - Codex: https://github.com/openai/codex",
            adapter.name()
        );
    }

    info!("Using adapter: {}", adapter.name());

    // Read context from stdin if available (non-blocking check)
    let context = read_stdin_if_available()?;

    // Parse output schema if provided
    let output_schema = parse_output_schema(output_schema)?;

    // Load model/max_turns from config if not overridden
    let agent_options = load_agent_options();

    let request = AgentRequest {
        prompt: prompt.clone(),
        context: if context.is_empty() {
            None
        } else {
            Some(context.clone())
        },
        cwd: std::env::current_dir().ok(),
        output_schema,
        model: std::env::var("AIRLOCK_AGENT_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| agent_options.as_ref().and_then(|o| o.model.clone())),
        max_turns: agent_options.as_ref().and_then(|o| o.max_turns),
        ..Default::default()
    };

    debug!(
        "Prompt length: {} chars, Context length: {} chars",
        prompt.len(),
        context.len()
    );

    // Run the agent — returns a stream
    let stream = adapter.run(&request).await.context("Failed to run agent")?;

    // Tee the stream: write each event as JSONL to stderr, feed to StreamCollector
    let stderr = io::stderr();

    // We need to consume the stream manually to both stream to stderr and collect
    let mut pinned = stream;
    let mut collector_events: Vec<std::result::Result<AgentEvent, airlock_core::AirlockError>> =
        Vec::new();

    while let Some(event_result) = pinned.next().await {
        match &event_result {
            Ok(event) => {
                // Write JSONL to stderr for real-time streaming
                if let Ok(json) = serde_json::to_string(event) {
                    let mut handle = stderr.lock();
                    let _ = writeln!(handle, "{}", json);
                }
            }
            Err(e) => {
                // Write error to stderr too
                let mut handle = stderr.lock();
                let _ = writeln!(
                    handle,
                    "{}",
                    serde_json::json!({"type": "error", "message": e.to_string()})
                );
            }
        }
        collector_events.push(event_result);
    }

    // Now collect the events into a result using StreamCollector
    let replay_stream: airlock_core::AgentEventStream =
        Box::pin(futures::stream::iter(collector_events));
    let result = StreamCollector::collect(replay_stream)
        .await
        .context("Failed to collect agent response")?;

    // Write final output to stdout
    println!("{}", result.content);

    Ok(())
}

fn load_agent_options() -> Option<AgentOptions> {
    let config_path = resolve_global_config_path(None)?;
    if !config_path.exists() {
        return None;
    }

    load_global_config(&config_path)
        .ok()
        .map(|c| c.agent.options)
}

/// Parse output schema from either a file path or inline JSON string.
fn parse_output_schema(schema: Option<String>) -> Result<Option<serde_json::Value>> {
    let Some(schema_str) = schema else {
        return Ok(None);
    };

    // Check if it looks like a file path (exists as a file)
    let path = Path::new(&schema_str);
    if path.exists() && path.is_file() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read schema file: {}", path.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse schema file as JSON: {}", path.display()))?;
        return Ok(Some(parsed));
    }

    // Otherwise, try to parse as inline JSON
    let parsed: serde_json::Value = serde_json::from_str(&schema_str).context(
        "Failed to parse output schema as JSON. Provide valid JSON or a path to a JSON file.",
    )?;
    Ok(Some(parsed))
}

/// Read from stdin if data is available (non-blocking for TTYs).
fn read_stdin_if_available() -> Result<String> {
    use std::io::IsTerminal;

    // If stdin is a TTY (terminal), don't try to read - there's no piped input
    if io::stdin().is_terminal() {
        return Ok(String::new());
    }

    // If stdin is not a TTY, it's likely piped input - read it
    let mut buffer = String::new();
    io::stdin()
        .read_to_string(&mut buffer)
        .context("Failed to read from stdin")?;

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    #[test]
    #[serial]
    fn test_load_agent_options_honors_airlock_home() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yml");
        std::fs::write(
            &config_path,
            "agent:\n  adapter: codex\n  options:\n    model: gpt-5\n    max_turns: 7\n",
        )
        .unwrap();

        let saved_home = std::env::var("AIRLOCK_HOME").ok();

        unsafe {
            std::env::set_var("AIRLOCK_HOME", temp_dir.path());
        }

        let options = load_agent_options().unwrap();

        match saved_home {
            Some(value) => unsafe {
                std::env::set_var("AIRLOCK_HOME", value);
            },
            None => unsafe {
                std::env::remove_var("AIRLOCK_HOME");
            },
        }

        assert_eq!(options.model.as_deref(), Some("gpt-5"));
        assert_eq!(options.max_turns, Some(7));
    }

    #[test]
    fn test_parse_output_schema_none() {
        let result = parse_output_schema(None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_output_schema_inline_json() {
        let schema = r#"{"type": "object", "properties": {"name": {"type": "string"}}}"#;
        let result = parse_output_schema(Some(schema.to_string())).unwrap();
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert_eq!(parsed["type"], "object");
    }

    #[test]
    fn test_parse_output_schema_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let schema_path = temp_dir.path().join("schema.json");
        let schema = r#"{"type": "array", "items": {"type": "string"}}"#;
        std::fs::write(&schema_path, schema).unwrap();

        let result = parse_output_schema(Some(schema_path.to_string_lossy().to_string())).unwrap();
        assert!(result.is_some());
        let parsed = result.unwrap();
        assert_eq!(parsed["type"], "array");
    }

    #[test]
    fn test_parse_output_schema_invalid_json() {
        let result = parse_output_schema(Some("not valid json".to_string()));
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn test_agent_model_env_var_overrides_config() {
        // When AIRLOCK_AGENT_MODEL is set, it should be used over config
        let saved = std::env::var("AIRLOCK_AGENT_MODEL").ok();
        unsafe {
            std::env::set_var("AIRLOCK_AGENT_MODEL", "haiku");
        }
        let model = std::env::var("AIRLOCK_AGENT_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .or(Some("opus-from-config".to_string()));
        assert_eq!(model, Some("haiku".to_string()));
        // Restore
        match saved {
            Some(v) => unsafe {
                std::env::set_var("AIRLOCK_AGENT_MODEL", v);
            },
            None => unsafe {
                std::env::remove_var("AIRLOCK_AGENT_MODEL");
            },
        }
    }

    #[test]
    #[serial]
    fn test_agent_model_env_var_empty_falls_through() {
        // Empty AIRLOCK_AGENT_MODEL should fall through to config
        let saved = std::env::var("AIRLOCK_AGENT_MODEL").ok();
        unsafe {
            std::env::set_var("AIRLOCK_AGENT_MODEL", "");
        }
        let model = std::env::var("AIRLOCK_AGENT_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .or(Some("opus-from-config".to_string()));
        assert_eq!(model, Some("opus-from-config".to_string()));
        match saved {
            Some(v) => unsafe {
                std::env::set_var("AIRLOCK_AGENT_MODEL", v);
            },
            None => unsafe {
                std::env::remove_var("AIRLOCK_AGENT_MODEL");
            },
        }
    }
}
