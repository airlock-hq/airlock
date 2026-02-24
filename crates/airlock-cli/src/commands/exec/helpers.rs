//! Shared helper functions for exec commands.

/// Extract branch name from a ref string.
///
/// Handles both "refs/heads/branch-name" and "branch-name" formats.
#[cfg(test)]
pub fn extract_branch_name(branch_ref: &str) -> String {
    branch_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(branch_ref)
        .to_string()
}
