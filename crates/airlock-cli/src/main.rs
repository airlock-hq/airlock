//! Airlock CLI
//!
//! Local Git proxy for AI-assisted development.
//!
//! When invoked without arguments, launches the desktop GUI application.

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;

/// Airlock - Local Git proxy for AI-assisted development
///
/// When invoked without arguments, launches the desktop application.
#[derive(Parser)]
#[command(name = "airlock")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize Airlock in the current repository
    Init,

    /// Eject from Airlock (restore original git configuration)
    Eject,

    /// Quick status check (pending runs, last sync)
    Status,

    /// List recent runs for the current repository
    Runs {
        /// Maximum number of runs to display
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },

    /// Show details for a specific run
    Show {
        /// The run ID (or prefix) to show details for
        run_id: String,
    },

    /// Cancel a stuck or running run
    Cancel {
        /// The run ID (or prefix) to cancel
        run_id: String,
    },

    /// Diagnose common issues
    Doctor,

    /// Completely remove all Airlock data (~/.airlock/)
    Nuke {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },

    /// Daemon management
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },

    /// Execute built-in helpers for pipeline stages
    ///
    /// These commands are designed to be called from within a pipeline stage.
    /// They read configuration from AIRLOCK_* environment variables.
    Exec {
        #[command(subcommand)]
        action: ExecAction,
    },

    /// Create artifacts for Push Request display
    ///
    /// These commands are designed to be called from within a pipeline stage.
    /// They write artifacts to $AIRLOCK_ARTIFACTS.
    Artifact {
        #[command(subcommand)]
        action: ArtifactAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the Airlock daemon
    Start,

    /// Stop the Airlock daemon
    Stop,

    /// Restart the Airlock daemon
    Restart,

    /// Check daemon status
    Status,

    /// Install daemon as a system service (auto-start at login)
    Install,

    /// Uninstall daemon from system services
    Uninstall,
}

#[derive(Subcommand)]
enum ExecAction {
    /// Run a prompt through an agent CLI
    ///
    /// Reads additional context from stdin if piped.
    /// Streams JSONL events to stderr, writes final output to stdout.
    Agent {
        /// The prompt to send to the agent
        prompt: String,

        /// JSON schema for structured output (file path or inline JSON)
        #[arg(long)]
        output_schema: Option<String>,

        /// Override the agent adapter (claude-code, codex, auto)
        #[arg(long)]
        adapter: Option<String>,
    },

    /// Apply patches and create a checkpoint commit
    ///
    /// Reads patches from $AIRLOCK_ARTIFACTS/patches/ and applies them.
    /// Creates a commit and writes the new SHA to $AIRLOCK_ARTIFACTS/.head_sha.
    Freeze,

    /// Push changes to upstream via the gate
    ///
    /// Two-phase push: updates gate ref, then pushes gate to upstream.
    /// Checks for upstream divergence before pushing.
    Push,

    /// Request human approval before continuing the pipeline
    ///
    /// Writes a marker file and exits 0. The executor detects the marker
    /// and pauses the pipeline for user approval in the UI.
    Await {
        /// Optional message explaining why approval is needed
        message: Option<String>,
    },

    /// JSON helper - extract fields or modify JSON from stdin
    ///
    /// Usage:
    ///   echo '{"title": "Hello"}' | airlock exec json title           # Extract field
    ///   echo '{"title": "Hello"}' | airlock exec json .               # Pass through
    ///   echo '{"a": "b"}' | airlock exec json --set c=123             # Add fields
    Json {
        /// Field path to extract (e.g., "title" or "a.b.c"), or "." for whole object
        #[arg(default_value = ".")]
        path: String,

        /// Add/set fields (key=value format)
        #[arg(long = "set", value_name = "KEY=VALUE")]
        set_fields: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ArtifactAction {
    /// Add markdown content artifact (summary, report, etc.)
    Content {
        /// Title for the content section
        #[arg(short, long)]
        title: String,

        /// File to read content from (reads from stdin if not provided)
        #[arg(short, long)]
        file: Option<std::path::PathBuf>,
    },

    /// Add code review comment artifact
    Comment {
        /// File path for single comment (required unless using --batch-file)
        #[arg(long)]
        file: Option<String>,

        /// Line number for single comment (required unless using --batch-file)
        #[arg(long)]
        line: Option<u32>,

        /// Message for single comment (required unless using --batch-file)
        #[arg(long)]
        message: Option<String>,

        /// Severity: info, warning, error (default: info)
        #[arg(long)]
        severity: Option<String>,

        /// JSON file with array of comments for batch mode
        #[arg(long)]
        batch_file: Option<std::path::PathBuf>,
    },

    /// Capture changes as a reviewable patch
    Patch {
        /// Title for the patch
        #[arg(short, long)]
        title: String,

        /// Explanation of what this patch does
        #[arg(short, long)]
        explanation: String,

        /// Diff file to use instead of capturing from git
        #[arg(long)]
        diff_file: Option<std::path::PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr) // Logs to stderr so stdout is clean for command output
        .init();

    match cli.command {
        // No subcommand: launch the GUI
        None => {
            commands::gui::launch()?;
            Ok(())
        }
        Some(Commands::Init) => commands::init::run().await,
        Some(Commands::Eject) => commands::eject::run().await,
        Some(Commands::Status) => commands::status::run().await,
        Some(Commands::Runs { limit }) => {
            let args = commands::runs::RunsArgs { limit };
            commands::runs::run(args).await
        }
        Some(Commands::Show { run_id }) => {
            let args = commands::show::ShowArgs { run_id };
            commands::show::run(args).await
        }
        Some(Commands::Cancel { run_id }) => {
            let args = commands::cancel::CancelArgs { run_id };
            commands::cancel::run(args).await
        }
        Some(Commands::Doctor) => commands::doctor::run().await,
        Some(Commands::Nuke { force }) => commands::nuke::run(force).await,
        Some(Commands::Daemon { action }) => match action {
            DaemonAction::Start => commands::daemon::start().await,
            DaemonAction::Stop => commands::daemon::stop().await,
            DaemonAction::Restart => commands::daemon::restart().await,
            DaemonAction::Status => commands::daemon::status().await,
            DaemonAction::Install => commands::daemon::install().await,
            DaemonAction::Uninstall => commands::daemon::uninstall().await,
        },
        Some(Commands::Exec { action }) => match action {
            ExecAction::Agent {
                prompt,
                output_schema,
                adapter,
            } => commands::exec::agent(prompt, output_schema, adapter).await,
            ExecAction::Freeze => commands::exec::freeze().await,
            ExecAction::Push => commands::exec::push().await,
            ExecAction::Await { message } => commands::exec::await_approval(message).await,
            ExecAction::Json { path, set_fields } => {
                let args = commands::exec::JsonArgs { path, set_fields };
                commands::exec::json(args).await
            }
        },
        Some(Commands::Artifact { action }) => match action {
            ArtifactAction::Content { title, file } => {
                let args = commands::artifact::content::ContentArgs { title, file };
                commands::artifact::content(args).await
            }
            ArtifactAction::Comment {
                file,
                line,
                message,
                severity,
                batch_file,
            } => {
                let args = commands::artifact::comment::CommentArgs {
                    file,
                    line,
                    message,
                    severity,
                    batch_file,
                };
                commands::artifact::comment(args).await
            }
            ArtifactAction::Patch {
                title,
                explanation,
                diff_file,
            } => {
                let args = commands::artifact::patch::PatchArgs {
                    title,
                    explanation,
                    diff_file,
                };
                commands::artifact::patch(args).await
            }
        },
    }
}
