//! Source control provider detection and CLI validation.
//!
//! Detects the SCM provider from a remote URL and checks whether the
//! corresponding CLI tool is installed and authenticated.

use crate::agent::subprocess::is_cli_available;

/// A source control management provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScmProvider {
    GitHub,
    GitLab,
    Bitbucket,
    AzureDevOps,
    Unknown,
}

impl ScmProvider {
    /// Human-readable name for the provider.
    pub fn display_name(&self) -> &str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
            Self::Bitbucket => "Bitbucket",
            Self::AzureDevOps => "Azure DevOps",
            Self::Unknown => "Unknown",
        }
    }

    /// CLI tool name required for PR creation, if any.
    pub fn cli_tool(&self) -> Option<&str> {
        match self {
            Self::GitHub => Some("gh"),
            Self::GitLab => Some("glab"),
            Self::Bitbucket | Self::AzureDevOps | Self::Unknown => None,
        }
    }

    /// Installation hint for the CLI tool.
    pub fn install_hint(&self) -> Option<&str> {
        match self {
            Self::GitHub => Some("https://cli.github.com"),
            Self::GitLab => Some("https://gitlab.com/gitlab-org/cli"),
            Self::Bitbucket | Self::AzureDevOps | Self::Unknown => None,
        }
    }
}

/// Result of checking a provider's CLI setup.
#[derive(Debug, Clone)]
pub struct ProviderCheck {
    pub provider: ScmProvider,
    pub cli_installed: bool,
    pub cli_authenticated: bool,
    pub cli_name: Option<String>,
}

/// Detect the SCM provider from a remote URL.
pub fn detect_provider(url: &str) -> ScmProvider {
    let lower = url.to_lowercase();
    if lower.contains("github.com") {
        ScmProvider::GitHub
    } else if lower.contains("gitlab.com") || lower.contains("gitlab.") {
        ScmProvider::GitLab
    } else if lower.contains("bitbucket.org") {
        ScmProvider::Bitbucket
    } else if lower.contains("dev.azure.com") || lower.contains("visualstudio.com") {
        ScmProvider::AzureDevOps
    } else {
        ScmProvider::Unknown
    }
}

/// Check whether the CLI for a provider is installed and authenticated.
pub fn check_provider_setup(url: &str) -> ProviderCheck {
    let provider = detect_provider(url);
    let cli_name = provider.cli_tool().map(|s| s.to_string());

    let cli_installed = cli_name.as_deref().map(is_cli_available).unwrap_or(false);

    let cli_authenticated = if cli_installed {
        check_cli_authenticated(&provider)
    } else {
        false
    };

    ProviderCheck {
        provider,
        cli_installed,
        cli_authenticated,
        cli_name,
    }
}

/// Run the provider-specific auth status command.
fn check_cli_authenticated(provider: &ScmProvider) -> bool {
    let (cmd, args) = match provider {
        ScmProvider::GitHub => ("gh", vec!["auth", "status"]),
        ScmProvider::GitLab => ("glab", vec!["auth", "status"]),
        _ => return false,
    };

    std::process::Command::new(cmd)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- detect_provider ---

    #[test]
    fn detect_github_https() {
        assert_eq!(
            detect_provider("https://github.com/user/repo.git"),
            ScmProvider::GitHub,
        );
    }

    #[test]
    fn detect_github_ssh() {
        assert_eq!(
            detect_provider("git@github.com:user/repo.git"),
            ScmProvider::GitHub,
        );
    }

    #[test]
    fn detect_gitlab_https() {
        assert_eq!(
            detect_provider("https://gitlab.com/user/repo.git"),
            ScmProvider::GitLab,
        );
    }

    #[test]
    fn detect_gitlab_selfhosted() {
        assert_eq!(
            detect_provider("https://gitlab.mycompany.com/user/repo"),
            ScmProvider::GitLab,
        );
    }

    #[test]
    fn detect_bitbucket() {
        assert_eq!(
            detect_provider("https://bitbucket.org/user/repo.git"),
            ScmProvider::Bitbucket,
        );
    }

    #[test]
    fn detect_azure_devops() {
        assert_eq!(
            detect_provider("https://dev.azure.com/org/project/_git/repo"),
            ScmProvider::AzureDevOps,
        );
    }

    #[test]
    fn detect_azure_visualstudio() {
        assert_eq!(
            detect_provider("https://org.visualstudio.com/project/_git/repo"),
            ScmProvider::AzureDevOps,
        );
    }

    #[test]
    fn detect_unknown() {
        assert_eq!(
            detect_provider("https://sr.ht/~user/repo"),
            ScmProvider::Unknown,
        );
    }

    #[test]
    fn detect_case_insensitive() {
        assert_eq!(
            detect_provider("https://GITHUB.COM/user/repo"),
            ScmProvider::GitHub,
        );
    }

    // --- ScmProvider methods ---

    #[test]
    fn display_names() {
        assert_eq!(ScmProvider::GitHub.display_name(), "GitHub");
        assert_eq!(ScmProvider::GitLab.display_name(), "GitLab");
        assert_eq!(ScmProvider::Bitbucket.display_name(), "Bitbucket");
        assert_eq!(ScmProvider::AzureDevOps.display_name(), "Azure DevOps");
        assert_eq!(ScmProvider::Unknown.display_name(), "Unknown");
    }

    #[test]
    fn cli_tools() {
        assert_eq!(ScmProvider::GitHub.cli_tool(), Some("gh"));
        assert_eq!(ScmProvider::GitLab.cli_tool(), Some("glab"));
        assert_eq!(ScmProvider::Bitbucket.cli_tool(), None);
        assert_eq!(ScmProvider::AzureDevOps.cli_tool(), None);
        assert_eq!(ScmProvider::Unknown.cli_tool(), None);
    }

    #[test]
    fn install_hints() {
        assert_eq!(
            ScmProvider::GitHub.install_hint(),
            Some("https://cli.github.com"),
        );
        assert_eq!(
            ScmProvider::GitLab.install_hint(),
            Some("https://gitlab.com/gitlab-org/cli"),
        );
        assert_eq!(ScmProvider::Bitbucket.install_hint(), None);
    }

    // --- check_provider_setup ---

    #[test]
    fn check_setup_unknown_provider() {
        let check = check_provider_setup("https://sr.ht/~user/repo");
        assert_eq!(check.provider, ScmProvider::Unknown);
        assert!(!check.cli_installed);
        assert!(!check.cli_authenticated);
        assert!(check.cli_name.is_none());
    }

    #[test]
    fn check_setup_bitbucket_no_cli() {
        let check = check_provider_setup("https://bitbucket.org/user/repo");
        assert_eq!(check.provider, ScmProvider::Bitbucket);
        assert!(!check.cli_installed);
        assert!(!check.cli_authenticated);
        assert!(check.cli_name.is_none());
    }
}
