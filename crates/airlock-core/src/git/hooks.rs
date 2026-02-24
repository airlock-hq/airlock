//! Hook scripts for bare repositories.

use crate::error::Result;
use crate::AirlockPaths;
use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Build the pre-receive hook script, embedding `crate::BANNER`.
///
/// The banner is displayed in the brand signal color (ANSI 256-color 98, orbital violet)
/// when stderr is a terminal and `NO_COLOR` is not set.
pub fn pre_receive_hook() -> String {
    format!(
        r#"#!/bin/sh
# Airlock pre-receive hook
# Accepts pushes locally and displays push info.

# Use brand color (signal violet) when stderr is a terminal and NO_COLOR is unset
if [ -t 2 ] && [ -z "${{NO_COLOR:-}}" ]; then
    BRAND='\033[1;38;5;{color}m'
    RESET='\033[0m'
else
    BRAND=''
    RESET=''
fi

printf "${{BRAND}}" >&2
cat >&2 <<'BANNER'
{}BANNER
printf "${{RESET}}" >&2

echo "" >&2

while read oldrev newrev refname; do
    branch="${{refname#refs/heads/}}"
    if [ "$oldrev" = "0000000000000000000000000000000000000000" ]; then
        echo "${{branch}} (new branch)" >&2
    elif [ "$newrev" = "0000000000000000000000000000000000000000" ]; then
        echo "${{branch}} (delete)" >&2
    else
        short_old=$(echo "$oldrev" | cut -c1-7)
        short_new=$(echo "$newrev" | cut -c1-7)
        echo "${{branch}} (${{short_old}}..${{short_new}})" >&2
    fi
done

echo "" >&2

# Always accept the push (soft gate)
exit 0
"#,
        crate::BANNER,
        color = crate::BRAND_COLOR_256,
    )
}

/// Post-receive hook script.
/// This hook triggers the transformation pipeline after push is accepted.
pub const POST_RECEIVE: &str = r#"#!/bin/sh
# Airlock post-receive hook
# Triggers the transformation pipeline after a push is accepted.

SOCKET="${HOME}/.airlock/socket"
REPO_PATH="$(pwd)"

# Collect all ref updates
REF_UPDATES=""
while read oldrev newrev refname; do
    if [ -n "$REF_UPDATES" ]; then
        REF_UPDATES="${REF_UPDATES},"
    fi
    REF_UPDATES="${REF_UPDATES}{\"ref_name\":\"${refname}\",\"old_sha\":\"${oldrev}\",\"new_sha\":\"${newrev}\"}"
done

# Notify daemon of push received
if [ -S "$SOCKET" ]; then
    echo "{\"jsonrpc\":\"2.0\",\"method\":\"push_received\",\"params\":{\"gate_path\":\"${REPO_PATH}\",\"ref_updates\":[${REF_UPDATES}]},\"id\":null}" | nc -U "$SOCKET" > /dev/null 2>&1 &
    echo "A new change has entered Airlock. Track it in the Airlock app." >&2
else
    echo "! Daemon is not running" >&2
    echo "Run 'airlock daemon start' to process this push." >&2
fi

echo "" >&2
exit 0
"#;

/// Upload-pack wrapper script content.
///
/// This script is installed at `~/.airlock/bin/airlock-upload-pack` and configured
/// as `remote.origin.uploadpack` in working repositories. When the user runs
/// `git fetch origin`, git invokes this wrapper instead of `git-upload-pack`.
///
/// The wrapper notifies the daemon to sync from upstream (if stale) before
/// exec-ing the real `git-upload-pack` to serve the fetch.
pub const UPLOAD_PACK_WRAPPER: &str = r#"#!/bin/sh
# Airlock upload-pack wrapper
# Notifies the daemon to sync from upstream before serving a fetch.
#
# This is configured as remote.origin.uploadpack in working repos
# so that git fetch triggers a sync-on-fetch via the daemon.

SOCKET="${HOME}/.airlock/socket"

# The repo path is the last argument (the gate path)
REPO_PATH=""
for arg in "$@"; do
    REPO_PATH="$arg"
done

# Notify daemon of fetch request (synchronous - wait for response)
if [ -S "$SOCKET" ]; then
    RESPONSE=$(echo "{\"jsonrpc\":\"2.0\",\"method\":\"fetch_notification\",\"params\":{\"gate_path\":\"${REPO_PATH}\"},\"id\":1}" | nc -U "$SOCKET" 2>/dev/null)

    # Log only on failure so the user knows data may be stale
    if echo "$RESPONSE" | grep -q '"synced":true'; then
        if ! echo "$RESPONSE" | grep -q '"success":true'; then
            echo "airlock: sync failed, using cached data" >&2
        fi
    fi
fi

# Execute the real git-upload-pack
exec git-upload-pack "$@"
"#;

/// Install hooks in a bare repository.
///
/// This installs the pre-receive and post-receive hooks needed for
/// Airlock to intercept pushes.
pub fn install_hooks(repo_path: &Path) -> Result<()> {
    let hooks_dir = repo_path.join("hooks");

    // Ensure hooks directory exists
    if !hooks_dir.exists() {
        fs::create_dir_all(&hooks_dir)?;
    }

    // Install pre-receive hook
    let pre_receive_path = hooks_dir.join("pre-receive");
    fs::write(&pre_receive_path, pre_receive_hook())?;
    make_executable(&pre_receive_path)?;
    tracing::debug!(
        "Installed pre-receive hook at {}",
        pre_receive_path.display()
    );

    // Install post-receive hook
    let post_receive_path = hooks_dir.join("post-receive");
    fs::write(&post_receive_path, POST_RECEIVE)?;
    make_executable(&post_receive_path)?;
    tracing::debug!(
        "Installed post-receive hook at {}",
        post_receive_path.display()
    );

    Ok(())
}

/// Remove hooks from a repository.
pub fn remove_hooks(repo_path: &Path) -> Result<()> {
    let hooks_dir = repo_path.join("hooks");

    for hook_name in &["pre-receive", "post-receive"] {
        let hook_path = hooks_dir.join(hook_name);
        if hook_path.exists() {
            fs::remove_file(&hook_path)?;
            tracing::debug!("Removed {} hook", hook_name);
        }
    }

    Ok(())
}

/// Install the upload-pack wrapper script.
///
/// This writes the wrapper script to `~/.airlock/bin/airlock-upload-pack`
/// and makes it executable. The wrapper is shared across all enrolled repos.
pub fn install_upload_pack_wrapper(paths: &AirlockPaths) -> Result<()> {
    let wrapper_path = paths.upload_pack_wrapper();

    // Ensure bin directory exists
    let bin_dir = paths.bin_dir();
    if !bin_dir.exists() {
        fs::create_dir_all(&bin_dir)?;
    }

    fs::write(&wrapper_path, UPLOAD_PACK_WRAPPER)?;
    make_executable(&wrapper_path)?;
    tracing::debug!(
        "Installed upload-pack wrapper at {}",
        wrapper_path.display()
    );

    Ok(())
}

/// Configure a working repository to use the upload-pack wrapper for fetches.
///
/// This sets `remote.origin.uploadpack` to point at the wrapper script,
/// so that `git fetch origin` triggers sync-on-fetch via the daemon.
pub fn configure_upload_pack(working_repo_path: &Path, wrapper_path: &Path) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["-C", working_repo_path.to_str().unwrap_or(".")])
        .args([
            "config",
            "remote.origin.uploadpack",
            wrapper_path.to_str().unwrap_or(""),
        ])
        .output()
        .map_err(|e| {
            crate::error::AirlockError::Git(format!(
                "Failed to configure remote.origin.uploadpack: {}",
                e
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::AirlockError::Git(format!(
            "Failed to configure remote.origin.uploadpack: {}",
            stderr.trim()
        )));
    }

    tracing::debug!(
        "Configured upload-pack wrapper for {}",
        working_repo_path.display()
    );
    Ok(())
}

/// Make a file executable.
///
/// On Unix, this sets the executable bit (chmod 755).
/// On Windows, this is a no-op since executability is determined by file extension.
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

/// Make a file executable.
///
/// On Windows, this is a no-op since executability is determined by file extension.
#[cfg(windows)]
fn make_executable(_path: &Path) -> Result<()> {
    // On Windows, files are executable based on extension, not permissions
    Ok(())
}
