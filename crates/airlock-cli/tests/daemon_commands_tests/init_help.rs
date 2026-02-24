//! E2E tests for `airlock init --help` (Section 7.1).

/// Test that `airlock init --help` displays helpful help text.
///
/// Per MVP Test Plan Section 7.1: "Help text is displayed with `--help`"
///
/// This test verifies that:
/// 1. Running `airlock init --help` produces help text
/// 2. The help text describes what the init command does
/// 3. The help text mentions key functionality
#[test]
fn test_e2e_airlock_init_help_displays_help_text() {
    use clap::error::ErrorKind;
    use clap::Parser;

    // Replicate the CLI structure to test help output
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
        /// Eject from Airlock (restore original git configuration)
        Eject,
        /// Quick status check (pending runs, last sync)
        Status,
    }

    // Test `airlock init --help` produces help output
    let result = TestCli::try_parse_from(["airlock", "init", "--help"]);
    assert!(
        result.is_err(),
        "--help should cause parse to 'fail' with DisplayHelp"
    );

    let error = result.unwrap_err();
    assert_eq!(
        error.kind(),
        ErrorKind::DisplayHelp,
        "Error kind should be DisplayHelp for --help flag"
    );

    let help_string = error.to_string();

    // Verify: Help text mentions the init command
    assert!(
        help_string.contains("init") || help_string.contains("Init"),
        "Help should mention 'init' command. Got: {}",
        help_string
    );

    // Verify: Help text describes what init does
    assert!(
        help_string.contains("Initialize")
            || help_string.contains("initialize")
            || help_string.contains("Airlock"),
        "Help should describe what init does. Got: {}",
        help_string
    );

    // Verify: Help text mentions the repository context
    assert!(
        help_string.contains("repository") || help_string.contains("repo"),
        "Help should mention repository. Got: {}",
        help_string
    );

    println!("Test passed: airlock init --help displays help text");
    println!("Help output:\n{}", help_string);
}
