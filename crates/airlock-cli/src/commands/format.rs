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

/// Format a Unix timestamp as an approximate `YYYY-MM-DD HH:MM` string.
///
/// Uses a simple calendar approximation (no leap-year handling) since
/// the output is only for human-readable CLI display. For exact
/// formatting, switch to `chrono`.
pub fn format_timestamp(timestamp: i64) -> String {
    let secs = timestamp;
    let days_since_epoch = secs / 86400;

    // Approximate year calculation
    let years = days_since_epoch / 365;
    let year = 1970 + years;

    // Remaining days in year
    let day_of_year = days_since_epoch % 365;

    // Approximate month and day
    let month = (day_of_year / 30) + 1;
    let day = (day_of_year % 30) + 1;

    // Time of day
    let secs_of_day = secs % 86400;
    let hours = secs_of_day / 3600;
    let minutes = (secs_of_day % 3600) / 60;

    format!(
        "{}-{:02}-{:02} {:02}:{:02}",
        year, month, day, hours, minutes
    )
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
