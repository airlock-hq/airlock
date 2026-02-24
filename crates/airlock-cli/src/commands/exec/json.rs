//! JSON helper - extract fields or modify JSON.
//!
//! Usage:
//!   echo '{"title": "Hello"}' | airlock exec json title           # Extract field
//!   echo '{"title": "Hello"}' | airlock exec json .               # Pass through (pretty)
//!   echo '{"a": "b"}' | airlock exec json --set c=123 --set d=foo # Add fields

use anyhow::{Context, Result};
use std::io::{self, Read};

/// Arguments for the json command.
pub struct JsonArgs {
    /// Field path to extract (e.g., "title" or "a.b.c"), or "." for whole object.
    pub path: String,

    /// Add/set fields (key=value format, value is parsed as JSON if valid, else string).
    pub set_fields: Vec<String>,
}

/// Execute the `json` helper command.
///
/// Reads JSON from stdin, extracts fields or modifies the object, and outputs the result.
pub async fn json(args: JsonArgs) -> Result<()> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("Failed to read from stdin")?;

    let mut value: serde_json::Value =
        serde_json::from_str(&input).context("Failed to parse JSON from stdin")?;

    // Apply --set modifications
    if let Some(obj) = value.as_object_mut() {
        for field in &args.set_fields {
            let (key, val) = field
                .split_once('=')
                .context("--set format must be KEY=VALUE")?;
            // Try parsing as JSON, fall back to string
            let json_val = serde_json::from_str(val)
                .unwrap_or_else(|_| serde_json::Value::String(val.to_string()));
            obj.insert(key.to_string(), json_val);
        }
    }

    // Extract path or output whole object
    if args.path == "." {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        let mut current = &value;
        for key in args.path.split('.') {
            current = current
                .get(key)
                .with_context(|| format!("Key '{}' not found in JSON", key))?;
        }
        match current {
            serde_json::Value::String(s) => println!("{}", s),
            other => println!("{}", other),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_args_creation() {
        let args = JsonArgs {
            path: "title".to_string(),
            set_fields: vec![],
        };
        assert_eq!(args.path, "title");
        assert!(args.set_fields.is_empty());
    }

    #[test]
    fn test_json_args_with_set_fields() {
        let args = JsonArgs {
            path: ".".to_string(),
            set_fields: vec!["key1=value1".to_string(), "key2=123".to_string()],
        };
        assert_eq!(args.path, ".");
        assert_eq!(args.set_fields.len(), 2);
    }
}
