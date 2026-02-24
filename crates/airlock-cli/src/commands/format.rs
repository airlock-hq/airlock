//! Shared formatting utilities for CLI commands.

use std::time::{SystemTime, UNIX_EPOCH};

/// Format a timestamp as "X ago" style relative time.
pub fn format_time_ago(timestamp: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let diff = now - timestamp;

    if diff < 0 {
        return "in the future".to_string();
    }

    let diff = diff as u64;

    if diff < 60 {
        return format!("{}s ago", diff);
    }

    let minutes = diff / 60;
    if minutes < 60 {
        return format!("{}m ago", minutes);
    }

    let hours = minutes / 60;
    if hours < 24 {
        return format!("{}h ago", hours);
    }

    let days = hours / 24;
    if days < 30 {
        return format!("{}d ago", days);
    }

    let months = days / 30;
    if months < 12 {
        return format!("{}mo ago", months);
    }

    let years = months / 12;
    format!("{}y ago", years)
}

/// Format run status with indicator symbol.
pub fn format_status(status: &str) -> String {
    match status {
        "running" => "● running".to_string(),
        "pending" => "○ pending".to_string(),
        "awaiting_approval" => "◐ awaiting".to_string(),
        "completed" => "✓ completed".to_string(),
        "failed" => "✗ failed".to_string(),
        other => format!("? {}", other),
    }
}
