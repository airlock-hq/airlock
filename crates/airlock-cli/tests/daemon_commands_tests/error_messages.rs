//! Error message tests (Section 12.3: CLI Errors).

use airlock_core::AirlockPaths;
use tempfile::TempDir;

/// Test that socket path is clearly communicated in error context.
///
/// Per MVP Test Plan Section 12.3: "Clear error message when daemon not running"
#[test]
fn test_socket_path_in_error_context() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());

    // Socket path should be deterministic and accessible
    #[cfg(unix)]
    {
        let socket_path = paths.socket();
        assert!(
            socket_path.to_string_lossy().contains("socket"),
            "Socket path should contain 'socket': {:?}",
            socket_path
        );
    }

    // Socket name should be usable for error messages
    let socket_name = paths.socket_name();
    assert!(
        !socket_name.is_empty(),
        "Socket name should not be empty for error messages"
    );
}

/// Test that connecting to non-existent socket produces clear error.
///
/// Per MVP Test Plan Section 12.3: "Clear error message when daemon not running"
#[tokio::test]
async fn test_connection_error_when_daemon_not_running() {
    use interprocess::local_socket::tokio::prelude::*;
    use interprocess::local_socket::tokio::Stream;
    #[cfg(unix)]
    use interprocess::local_socket::GenericFilePath;

    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Socket should not exist
    #[cfg(unix)]
    {
        let socket_path = paths.socket();
        assert!(
            !socket_path.exists(),
            "Socket should not exist when daemon is not running"
        );
    }

    // Attempt to connect should fail
    #[cfg(unix)]
    {
        let socket_name = paths.socket_name();
        let name = socket_name.to_fs_name::<GenericFilePath>();

        match name {
            Ok(n) => {
                let connect_result = Stream::connect(n).await;
                assert!(
                    connect_result.is_err(),
                    "Connection should fail when daemon is not running"
                );

                // The error should be related to connection failure
                let error = connect_result.err().unwrap();
                let error_str = error.to_string();

                // Verify it's a recognizable connection error
                // Common error kinds: ConnectionRefused, NotFound, etc.
                assert!(
                    error.kind() == std::io::ErrorKind::NotFound
                        || error.kind() == std::io::ErrorKind::ConnectionRefused
                        || error_str.contains("No such file")
                        || error_str.contains("Connection refused"),
                    "Error should indicate connection failure, got: {} (kind: {:?})",
                    error_str,
                    error.kind()
                );
            }
            Err(e) => {
                // If we can't even create the socket name, that's also an error
                panic!("Failed to create socket name: {}", e);
            }
        }
    }
}

/// Test that socket file absence provides a detectable condition.
/// This allows CLI tools to show "Daemon is not running" before attempting connection.
#[test]
fn test_can_detect_daemon_not_running() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // The socket file should not exist, which is the primary way to detect daemon not running
    #[cfg(unix)]
    {
        let socket_path = paths.socket();

        // This is how CLI tools check if daemon is running:
        // if !socket_path.exists() { return Err("Daemon is not running") }
        let daemon_running = socket_path.exists();

        assert!(
            !daemon_running,
            "Daemon should be detectable as not running when socket file is absent"
        );

        // Verify the check provides enough info for a good error message
        let socket_path_str = socket_path.to_string_lossy();
        assert!(
            !socket_path_str.is_empty(),
            "Socket path should be non-empty for error messages"
        );

        // A CLI tool could show: "Daemon is not running. Socket not found at: {socket_path}"
    }
}

// =============================================================================
// Invalid Argument Error Message Tests (Section 12.3: CLI Errors)
// =============================================================================

/// Test that unknown subcommand produces clear error message.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for invalid arguments"
#[test]
fn test_unknown_subcommand_error_message() {
    use clap::error::ErrorKind;
    use clap::Parser;

    // Reproduce the CLI struct for testing
    #[derive(Parser, Debug)]
    #[command(name = "airlock")]
    struct TestCli {
        #[arg(short, long, global = true)]
        verbose: bool,

        #[command(subcommand)]
        command: Option<TestCommands>,
    }

    #[derive(clap::Subcommand, Debug)]
    enum TestCommands {
        Init,
        Eject,
        Status,
        Runs {
            #[arg(short, long, default_value = "20")]
            limit: u32,
        },
        Show {
            run_id: String,
        },
        Doctor,
        Daemon {
            #[command(subcommand)]
            action: TestDaemonAction,
        },
    }

    #[derive(clap::Subcommand, Debug)]
    enum TestDaemonAction {
        Start,
        Stop,
        Restart,
        Status,
        Install,
        Uninstall,
    }

    // Test unknown subcommand
    let result = TestCli::try_parse_from(["airlock", "foobar"]);
    assert!(
        result.is_err(),
        "Unknown subcommand should produce an error"
    );

    let error = result.unwrap_err();
    assert_eq!(
        error.kind(),
        ErrorKind::InvalidSubcommand,
        "Error kind should be InvalidSubcommand"
    );

    let error_string = error.to_string();
    assert!(
        error_string.contains("foobar"),
        "Error message should mention the invalid subcommand: {}",
        error_string
    );
    // Clap suggests similar commands if available
    assert!(
        error_string.contains("error:")
            || error_string.contains("invalid")
            || error_string.contains("unrecognized"),
        "Error message should be clear about the problem: {}",
        error_string
    );
}

/// Test that unknown option produces clear error message.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for invalid arguments"
#[test]
fn test_unknown_option_error_message() {
    use clap::error::ErrorKind;
    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(name = "airlock")]
    struct TestCli {
        #[arg(short, long, global = true)]
        verbose: bool,

        #[command(subcommand)]
        command: Option<TestCommands>,
    }

    #[derive(clap::Subcommand, Debug)]
    enum TestCommands {
        Init,
        Status,
    }

    // Test unknown option
    let result = TestCli::try_parse_from(["airlock", "--unknown-option"]);
    assert!(result.is_err(), "Unknown option should produce an error");

    let error = result.unwrap_err();
    assert_eq!(
        error.kind(),
        ErrorKind::UnknownArgument,
        "Error kind should be UnknownArgument"
    );

    let error_string = error.to_string();
    assert!(
        error_string.contains("--unknown-option"),
        "Error message should mention the unknown option: {}",
        error_string
    );
    assert!(
        error_string.contains("error:") || error_string.contains("unexpected"),
        "Error message should be clear about the problem: {}",
        error_string
    );
}

/// Test that missing required argument produces clear error message.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for invalid arguments"
#[test]
fn test_missing_required_argument_error_message() {
    use clap::error::ErrorKind;
    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(name = "airlock")]
    struct TestCli {
        #[command(subcommand)]
        command: Option<TestCommands>,
    }

    #[derive(clap::Subcommand, Debug)]
    enum TestCommands {
        Show {
            /// Required argument
            run_id: String,
        },
    }

    // Test missing required argument for 'show' command
    let result = TestCli::try_parse_from(["airlock", "show"]);
    assert!(
        result.is_err(),
        "Missing required argument should produce an error"
    );

    let error = result.unwrap_err();
    assert_eq!(
        error.kind(),
        ErrorKind::MissingRequiredArgument,
        "Error kind should be MissingRequiredArgument"
    );

    let error_string = error.to_string();
    assert!(
        error_string.contains("run_id") || error_string.contains("<RUN_ID>"),
        "Error message should mention the missing argument: {}",
        error_string
    );
    assert!(
        error_string.contains("required"),
        "Error message should indicate the argument is required: {}",
        error_string
    );
}

/// Test that invalid argument value produces clear error message.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for invalid arguments"
#[test]
fn test_invalid_argument_value_error_message() {
    use clap::error::ErrorKind;
    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(name = "airlock")]
    struct TestCli {
        #[command(subcommand)]
        command: Option<TestCommands>,
    }

    #[derive(clap::Subcommand, Debug)]
    enum TestCommands {
        Runs {
            #[arg(short, long, default_value = "20")]
            limit: u32,
        },
    }

    // Test invalid value for --limit (expects u32, gets string)
    let result = TestCli::try_parse_from(["airlock", "runs", "--limit", "not-a-number"]);
    assert!(
        result.is_err(),
        "Invalid argument value should produce an error"
    );

    let error = result.unwrap_err();
    assert_eq!(
        error.kind(),
        ErrorKind::ValueValidation,
        "Error kind should be ValueValidation"
    );

    let error_string = error.to_string();
    assert!(
        error_string.contains("not-a-number") || error_string.contains("invalid"),
        "Error message should indicate the invalid value: {}",
        error_string
    );
}

/// Test that help option produces helpful output.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for invalid arguments"
#[test]
fn test_help_option_produces_useful_output() {
    use clap::error::ErrorKind;
    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(name = "airlock")]
    #[command(about = "Local Git proxy for AI-assisted development")]
    struct TestCli {
        #[arg(short, long, global = true)]
        verbose: bool,

        #[command(subcommand)]
        command: Option<TestCommands>,
    }

    #[derive(clap::Subcommand, Debug)]
    enum TestCommands {
        /// Initialize Airlock in the current repository
        Init,
        /// Quick status check
        Status,
    }

    // Test --help produces useful output
    let result = TestCli::try_parse_from(["airlock", "--help"]);
    assert!(result.is_err(), "--help should exit with special error");

    let error = result.unwrap_err();
    assert_eq!(
        error.kind(),
        ErrorKind::DisplayHelp,
        "Error kind should be DisplayHelp"
    );

    let help_string = error.to_string();
    assert!(
        help_string.contains("airlock"),
        "Help should mention the program name: {}",
        help_string
    );
    assert!(
        help_string.contains("init") || help_string.contains("Init"),
        "Help should list available commands: {}",
        help_string
    );
    assert!(
        help_string.contains("-v") || help_string.contains("--verbose"),
        "Help should list available options: {}",
        help_string
    );
}

/// Test that daemon subcommand requires an action.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for invalid arguments"
#[test]
fn test_daemon_subcommand_requires_action() {
    use clap::error::ErrorKind;
    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(name = "airlock")]
    struct TestCli {
        #[command(subcommand)]
        command: Option<TestCommands>,
    }

    #[derive(clap::Subcommand, Debug)]
    enum TestCommands {
        Daemon {
            #[command(subcommand)]
            action: TestDaemonAction,
        },
    }

    #[derive(clap::Subcommand, Debug)]
    enum TestDaemonAction {
        Start,
        Stop,
        Status,
    }

    // Test 'daemon' without action
    let result = TestCli::try_parse_from(["airlock", "daemon"]);
    assert!(
        result.is_err(),
        "daemon without action should produce an error"
    );

    let error = result.unwrap_err();
    // Clap may return MissingSubcommand or DisplayHelpOnMissingArgumentOrSubcommand
    assert!(
        error.kind() == ErrorKind::MissingSubcommand
            || error.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand,
        "Error kind should be MissingSubcommand or DisplayHelpOnMissingArgumentOrSubcommand, got: {:?}",
        error.kind()
    );

    let error_string = error.to_string();
    // Should suggest available subcommands or indicate one is required
    // When DisplayHelpOnMissingArgumentOrSubcommand, it shows the help text
    assert!(
        error_string.contains("subcommand")
            || error_string.contains("start")
            || error_string.contains("stop")
            || error_string.contains("Usage"),
        "Error message should indicate subcommand is needed or list available ones: {}",
        error_string
    );
}

// =============================================================================
// Git Operation Failure Error Message Tests (Section 12.3: CLI Errors)
// =============================================================================

/// Test that git operation failures produce clear error messages.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for git operation failures"
///
/// This test verifies that when git operations fail, the error messages are:
/// 1. Clear and descriptive of what went wrong
/// 2. Include context about the operation that failed
/// 3. Suggest potential fixes or next steps where applicable
#[test]
fn test_git_operation_failure_error_messages() {
    use airlock_core::git;
    use git2::Repository;

    // Test 1: Discovering a non-existent repository
    // This simulates what happens when a user runs a command outside a git repo
    let non_existent_path = std::path::Path::new("/tmp/definitely_not_a_git_repo_12345xyz");
    let discover_result = git::discover_repo(non_existent_path);

    assert!(
        discover_result.is_err(),
        "discover_repo should fail for non-existent path"
    );

    // Use match since Repository doesn't implement Debug
    let error = match discover_result {
        Ok(_) => panic!("Expected error but got Ok"),
        Err(e) => e,
    };
    let error_string = error.to_string().to_lowercase();

    // The error should indicate the issue is related to git/repository
    assert!(
        error_string.contains("repository")
            || error_string.contains("git")
            || error_string.contains("not found")
            || error_string.contains("could not find"),
        "Error message should indicate git repository issue. Got: {}",
        error
    );

    // Test 2: Opening a path that is not a git repository
    let temp_dir = TempDir::new().unwrap();
    let non_repo_path = temp_dir.path().join("not_a_repo");
    std::fs::create_dir_all(&non_repo_path).unwrap();

    let open_result = git::open_repo(&non_repo_path);
    assert!(
        open_result.is_err(),
        "open_repo should fail for non-git directory"
    );

    // Use match since Repository doesn't implement Debug
    let error = match open_result {
        Ok(_) => panic!("Expected error but got Ok"),
        Err(e) => e,
    };
    let error_string = error.to_string().to_lowercase();
    assert!(
        error_string.contains("repository")
            || error_string.contains("git")
            || error_string.contains("not found"),
        "Error message should indicate repository issue. Got: {}",
        error
    );

    // Test 3: Adding a remote that already exists
    let repo_path = temp_dir.path().join("test_repo");
    std::fs::create_dir_all(&repo_path).unwrap();
    let repo = Repository::init(&repo_path).unwrap();

    // Add a remote first
    git::add_remote(&repo, "origin", "https://github.com/test/repo.git").unwrap();

    // Try to add the same remote again - should fail
    let duplicate_remote_result =
        git::add_remote(&repo, "origin", "https://github.com/other/repo.git");
    assert!(
        duplicate_remote_result.is_err(),
        "Adding duplicate remote should fail"
    );

    let error = duplicate_remote_result.unwrap_err();
    let error_string = error.to_string().to_lowercase();
    assert!(
        error_string.contains("origin")
            || error_string.contains("remote")
            || error_string.contains("exists")
            || error_string.contains("already"),
        "Error message should mention the conflicting remote. Got: {}",
        error
    );

    // Test 4: Getting URL of non-existent remote
    let get_url_result = git::get_remote_url(&repo, "nonexistent_remote");
    assert!(
        get_url_result.is_err(),
        "get_remote_url should fail for non-existent remote"
    );

    let error = get_url_result.unwrap_err();
    let error_string = error.to_string().to_lowercase();
    assert!(
        error_string.contains("remote")
            || error_string.contains("nonexistent")
            || error_string.contains("not found"),
        "Error message should indicate the remote was not found. Got: {}",
        error
    );

    // Test 5: Renaming a non-existent remote
    let rename_result = git::rename_remote(&repo, "does_not_exist", "new_name");
    assert!(
        rename_result.is_err(),
        "rename_remote should fail for non-existent remote"
    );

    let error = rename_result.unwrap_err();
    let error_string = error.to_string().to_lowercase();
    assert!(
        error_string.contains("remote")
            || error_string.contains("not found")
            || error_string.contains("does_not_exist"),
        "Error message should indicate the remote was not found. Got: {}",
        error
    );

    // Test 6: Removing a non-existent remote
    let remove_result = git::remove_remote(&repo, "another_nonexistent");
    assert!(
        remove_result.is_err(),
        "remove_remote should fail for non-existent remote"
    );

    let error = remove_result.unwrap_err();
    let error_string = error.to_string().to_lowercase();
    assert!(
        error_string.contains("remote")
            || error_string.contains("not found")
            || error_string.contains("another_nonexistent"),
        "Error message should indicate the remote was not found. Got: {}",
        error
    );
}

/// Test that creating a bare repo at an existing path produces clear error.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for git operation failures"
#[test]
fn test_create_bare_repo_at_existing_path_error_message() {
    use airlock_core::git;

    let temp_dir = TempDir::new().unwrap();
    let existing_path = temp_dir.path().join("existing_dir");
    std::fs::create_dir_all(&existing_path).unwrap();

    // Try to create a bare repo at an existing path
    let result = git::create_bare_repo(&existing_path);
    assert!(
        result.is_err(),
        "create_bare_repo should fail for existing path"
    );

    // Use match since Repository doesn't implement Debug
    let error = match result {
        Ok(_) => panic!("Expected error but got Ok"),
        Err(e) => e,
    };
    let error_string = error.to_string().to_lowercase();

    // The error message should indicate the path already exists
    assert!(
        error_string.contains("exists")
            || error_string.contains("already")
            || error_string.contains("path"),
        "Error message should indicate path already exists. Got: {}",
        error
    );
}

/// Test that fetch from non-existent remote produces clear error.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for git operation failures"
#[test]
fn test_fetch_from_nonexistent_remote_error_message() {
    use airlock_core::git;
    use git2::Repository;

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("test_repo");
    std::fs::create_dir_all(&repo_path).unwrap();
    let repo = Repository::init(&repo_path).unwrap();

    // Create an initial commit to have a valid repo
    {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    // Try to fetch from a non-existent remote
    let result = git::fetch(&repo_path, "nonexistent_remote");
    assert!(result.is_err(), "fetch should fail for non-existent remote");

    let error = result.unwrap_err();
    let error_string = error.to_string().to_lowercase();

    // The error should indicate the remote was not found
    assert!(
        error_string.contains("remote")
            || error_string.contains("not found")
            || error_string.contains("nonexistent"),
        "Error message should indicate remote was not found. Got: {}",
        error
    );
}

/// Test that push to non-existent remote produces clear error.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for git operation failures"
#[test]
fn test_push_to_nonexistent_remote_error_message() {
    use airlock_core::git;
    use git2::Repository;

    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().join("test_repo");
    std::fs::create_dir_all(&repo_path).unwrap();
    let repo = Repository::init(&repo_path).unwrap();

    // Create an initial commit to have a valid repo
    {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();

        // Create a file
        let file_path = repo_path.join("README.md");
        std::fs::write(&file_path, "# Test").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("README.md")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    // Try to push to a non-existent remote
    let result = git::push(&repo_path, "nonexistent_remote", &["refs/heads/master"]);
    assert!(result.is_err(), "push should fail for non-existent remote");

    let error = result.unwrap_err();
    let error_string = error.to_string().to_lowercase();

    // The error should indicate the remote was not found
    assert!(
        error_string.contains("remote")
            || error_string.contains("not found")
            || error_string.contains("nonexistent"),
        "Error message should indicate remote was not found. Got: {}",
        error
    );
}

/// Test that parsing invalid ref update format produces clear error.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for git operation failures"
#[test]
fn test_parse_invalid_ref_update_error_message() {
    use airlock_core::git;

    // Test various invalid formats

    // Too few parts
    let result = git::parse_ref_updates("abc123 def456");
    assert!(result.is_err(), "Should fail with too few parts");

    let error = result.unwrap_err();
    let error_string = error.to_string().to_lowercase();
    assert!(
        error_string.contains("invalid") || error_string.contains("format"),
        "Error should indicate invalid format. Got: {}",
        error
    );

    // Too many parts
    let result = git::parse_ref_updates("abc123 def456 refs/heads/main extra_part");
    assert!(result.is_err(), "Should fail with too many parts");

    let error = result.unwrap_err();
    let error_string = error.to_string().to_lowercase();
    assert!(
        error_string.contains("invalid") || error_string.contains("format"),
        "Error should indicate invalid format. Got: {}",
        error
    );

    // Just a single word
    let result = git::parse_ref_updates("invalidformat");
    assert!(result.is_err(), "Should fail with single word");

    let error = result.unwrap_err();
    let error_string = error.to_string().to_lowercase();
    assert!(
        error_string.contains("invalid") || error_string.contains("format"),
        "Error should indicate invalid format. Got: {}",
        error
    );
}

/// Test that AirlockError from git2::Error conversion preserves useful message.
///
/// Per MVP Test Plan Section 12.3: "Clear error message for git operation failures"
#[test]
fn test_airlock_error_from_git2_preserves_message() {
    use airlock_core::error::AirlockError;

    // Create a git2 error and convert it to AirlockError
    let git2_error = git2::Error::from_str("Test error message with context");
    let airlock_error: AirlockError = git2_error.into();

    let error_string = airlock_error.to_string();

    // The converted error should preserve the original message
    assert!(
        error_string.contains("Test error message with context"),
        "Converted error should preserve original message. Got: {}",
        error_string
    );

    // The error should be wrapped with "Git error:" prefix
    assert!(
        error_string.contains("Git error"),
        "Error should indicate it's a git error. Got: {}",
        error_string
    );
}
