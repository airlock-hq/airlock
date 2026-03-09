//! Interactive init wizard for first-time setup.

use anyhow::Result;
use console::Style;
use dialoguer::Select;

use airlock_core::{
    check_provider_setup, AgentAdapter, ClaudeCodeAdapter, CodexAdapter, ProviderCheck,
    ScmProvider, BANNER, BRAND_COLOR_256,
};

/// Result of the init wizard.
pub struct WizardResult {
    /// Agent adapter choice (only Some if first-time setup).
    pub agent_adapter: Option<String>,
}

/// Run the init wizard.
///
/// Shows a branded banner and walks the user through setup:
/// - If `first_time_setup`: asks user-level questions (agent selection)
/// - Detects SCM provider and validates CLI setup
pub fn run_wizard(first_time_setup: bool) -> Result<WizardResult> {
    // Print branded banner
    let brand = Style::new().bold().color256(BRAND_COLOR_256);
    println!("{}", brand.apply_to(BANNER.trim_start_matches('\n')));
    println!();
    println!("Welcome to Airlock! Let's get you set up.");
    println!();

    // User-level: agent selection (first time only)
    let agent_adapter = if first_time_setup {
        Some(ask_agent_selection()?)
    } else {
        None
    };

    // Provider check: detect SCM provider and validate CLI setup
    let provider_check = run_provider_check();
    if let Some(ref check) = provider_check {
        print_provider_check(check);
    }

    Ok(WizardResult { agent_adapter })
}

/// Ask the user which agent adapter to use.
fn ask_agent_selection() -> Result<String> {
    let claude = ClaudeCodeAdapter::new();
    let codex = CodexAdapter::new();

    let claude_status = if claude.is_available() {
        "available"
    } else {
        "not found"
    };
    let codex_status = if codex.is_available() {
        "available"
    } else {
        "not found"
    };

    let items = vec![
        format!("Auto-detect (recommended)"),
        format!("Claude Code ({})", claude_status),
        format!("Codex / OpenAI ({})", codex_status),
    ];

    let selection = Select::new()
        .with_prompt("Which AI agent should Airlock use?")
        .items(&items)
        .default(0)
        .interact()?;

    let adapter = match selection {
        0 => "auto",
        1 => "claude-code",
        2 => "codex",
        _ => "auto",
    };

    println!();
    Ok(adapter.to_string())
}

/// Try to detect the SCM provider from the current repo's origin remote.
/// Returns `None` if not in a git repo or no origin is configured.
fn run_provider_check() -> Option<ProviderCheck> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return None;
    }

    Some(check_provider_setup(&url))
}

/// Print provider check results with actionable guidance.
fn print_provider_check(check: &ProviderCheck) {
    println!("Detected provider: {}", check.provider.display_name());

    match check.provider {
        ScmProvider::Bitbucket | ScmProvider::AzureDevOps | ScmProvider::Unknown => {
            println!(
                "  ! {} is not supported by Airlock at the moment.",
                check.provider.display_name()
            );
            println!("    Airlock won't be able to create pull requests automatically.");
            println!("    Everything else will work normally.");
        }
        _ => {
            let cli = check.cli_name.as_deref().unwrap_or("unknown");

            if check.cli_installed && check.cli_authenticated {
                println!(
                    "  \u{2713} {} is installed and authenticated \u{2014} pull request creation is ready",
                    cli
                );
            } else if check.cli_installed {
                let auth_cmd = match check.provider {
                    ScmProvider::GitHub => "gh auth login",
                    ScmProvider::GitLab => "glab auth login",
                    _ => unreachable!(),
                };
                println!("  ! {} is installed but not authenticated", cli);
                println!("    Airlock won't be able to create pull requests automatically.");
                println!("    Everything else will work normally.");
                println!("    Run `{}` to authenticate.", auth_cmd);
            } else {
                let hint = check.provider.install_hint().unwrap_or("");
                println!("  ! {} is not installed", cli);
                println!("    Airlock won't be able to create pull requests automatically.");
                println!("    Everything else will work normally.");
                println!("    Install: {}", hint);
            }
        }
    }

    println!();
}
