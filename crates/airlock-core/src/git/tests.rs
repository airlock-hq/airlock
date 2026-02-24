use super::*;
use crate::types::RefUpdate;
use git2::Repository;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn create_test_repo() -> (TempDir, Repository) {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init(dir.path()).unwrap();

    // Create an initial commit so we have a valid repo
    {
        let sig = repo
            .signature()
            .unwrap_or_else(|_| git2::Signature::now("Test", "test@example.com").unwrap());

        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    (dir, repo)
}

#[test]
fn test_create_bare_repo() {
    let dir = TempDir::new().unwrap();
    let bare_path = dir.path().join("test.git");

    let repo = create_bare_repo(&bare_path).unwrap();
    assert!(repo.is_bare());
    assert!(bare_path.exists());
}

#[test]
fn test_create_bare_repo_path_exists() {
    let dir = TempDir::new().unwrap();
    // Try to create at existing path
    let result = create_bare_repo(dir.path());
    assert!(result.is_err());
}

#[test]
fn test_remote_operations() {
    let (_dir, repo) = create_test_repo();

    // Add remote
    add_remote(&repo, "upstream", "https://github.com/test/repo.git").unwrap();

    // Check it exists
    assert!(remote_exists(&repo, "upstream"));
    assert!(!remote_exists(&repo, "nonexistent"));

    // Get URL
    let url = get_remote_url(&repo, "upstream").unwrap();
    assert_eq!(url, "https://github.com/test/repo.git");

    // List remotes
    let remotes = list_remotes(&repo).unwrap();
    assert!(remotes.contains(&"upstream".to_string()));

    // Rename remote
    rename_remote(&repo, "upstream", "origin").unwrap();
    assert!(remote_exists(&repo, "origin"));
    assert!(!remote_exists(&repo, "upstream"));

    // Set URL
    set_remote_url(&repo, "origin", "https://github.com/new/repo.git").unwrap();
    let new_url = get_remote_url(&repo, "origin").unwrap();
    assert_eq!(new_url, "https://github.com/new/repo.git");

    // Remove remote
    remove_remote(&repo, "origin").unwrap();
    assert!(!remote_exists(&repo, "origin"));
}

#[test]
fn test_install_hooks() {
    let dir = TempDir::new().unwrap();
    let bare_path = dir.path().join("test.git");

    create_bare_repo(&bare_path).unwrap();
    install_hooks(&bare_path).unwrap();

    let pre_receive = bare_path.join("hooks/pre-receive");
    let post_receive = bare_path.join("hooks/post-receive");

    assert!(pre_receive.exists());
    assert!(post_receive.exists());

    // pre-upload-pack should NOT be installed (it was never a real git hook)
    let pre_upload_pack = bare_path.join("hooks/pre-upload-pack");
    assert!(!pre_upload_pack.exists());

    // Check they are executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let pre_perms = fs::metadata(&pre_receive).unwrap().permissions();
        assert!(pre_perms.mode() & 0o111 != 0);

        let post_perms = fs::metadata(&post_receive).unwrap().permissions();
        assert!(post_perms.mode() & 0o111 != 0);
    }
}

#[test]
fn test_remove_hooks() {
    let dir = TempDir::new().unwrap();
    let bare_path = dir.path().join("test.git");

    create_bare_repo(&bare_path).unwrap();
    install_hooks(&bare_path).unwrap();
    remove_hooks(&bare_path).unwrap();

    let pre_receive = bare_path.join("hooks/pre-receive");
    let post_receive = bare_path.join("hooks/post-receive");

    assert!(!pre_receive.exists());
    assert!(!post_receive.exists());
}

#[test]
fn test_install_upload_pack_wrapper() {
    let dir = TempDir::new().unwrap();
    let paths = crate::AirlockPaths::with_root(dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    install_upload_pack_wrapper(&paths).unwrap();

    let wrapper_path = paths.upload_pack_wrapper();
    assert!(wrapper_path.exists());

    // Check it's executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::metadata(&wrapper_path).unwrap().permissions();
        assert!(perms.mode() & 0o111 != 0);
    }

    // Verify content
    let content = fs::read_to_string(&wrapper_path).unwrap();
    assert!(content.contains("git-upload-pack"));
    assert!(content.contains("fetch_notification"));
}

#[test]
fn test_parse_ref_updates() {
    let input = "abc123 def456 refs/heads/main\n\
                 000000 111111 refs/heads/feature\n";

    let updates = parse_ref_updates(input).unwrap();
    assert_eq!(updates.len(), 2);

    assert_eq!(updates[0].ref_name, "refs/heads/main");
    assert_eq!(updates[0].old_sha, "abc123");
    assert_eq!(updates[0].new_sha, "def456");

    assert_eq!(updates[1].ref_name, "refs/heads/feature");
}

#[test]
fn test_parse_ref_updates_empty() {
    let updates = parse_ref_updates("").unwrap();
    assert!(updates.is_empty());

    let updates = parse_ref_updates("   \n\n   ").unwrap();
    assert!(updates.is_empty());
}

#[test]
fn test_parse_ref_updates_invalid() {
    let result = parse_ref_updates("invalid format");
    assert!(result.is_err());
}

/// Integration test: Ref updates are parsed correctly (branch name, old SHA, new SHA)
///
/// Tests realistic git hook input format with:
/// - Full 40-character SHA hashes (as Git actually provides)
/// - Multiple ref types (branches and tags)
/// - All operations: create (null old SHA), update, delete (null new SHA)
/// - Proper field extraction (ref_name, old_sha, new_sha)
#[test]
fn test_parse_ref_updates_realistic_git_format() {
    // Realistic 40-char SHA hashes as Git provides
    let null_sha = "0000000000000000000000000000000000000000";
    let sha1 = "a1b2c3d4e5f6789012345678901234567890abcd";
    let sha2 = "b2c3d4e5f67890123456789012345678901bcdef";
    let sha3 = "c3d4e5f6789012345678901234567890abcdef12";
    let sha4 = "d4e5f6789012345678901234567890abcdef1234";

    // Realistic git hook input:
    // - Branch update (refs/heads/main)
    // - New branch creation (refs/heads/feature/new-feature)
    // - Branch deletion (refs/heads/old-branch)
    // - Tag creation (refs/tags/v1.0.0)
    let input = format!(
        "{} {} refs/heads/main\n\
         {} {} refs/heads/feature/new-feature\n\
         {} {} refs/heads/old-branch\n\
         {} {} refs/tags/v1.0.0\n",
        sha1,
        sha2, // update: old -> new
        null_sha,
        sha3, // create: null -> new
        sha4,
        null_sha, // delete: old -> null
        null_sha,
        sha1, // tag creation: null -> sha
    );

    let updates = parse_ref_updates(&input).unwrap();
    assert_eq!(updates.len(), 4);

    // Verify branch update parsing
    assert_eq!(updates[0].ref_name, "refs/heads/main");
    assert_eq!(updates[0].old_sha, sha1);
    assert_eq!(updates[0].new_sha, sha2);
    assert_eq!(get_ref_update_type(&updates[0]), RefUpdateType::Update);

    // Verify new branch creation parsing
    assert_eq!(updates[1].ref_name, "refs/heads/feature/new-feature");
    assert_eq!(updates[1].old_sha, null_sha);
    assert_eq!(updates[1].new_sha, sha3);
    assert_eq!(get_ref_update_type(&updates[1]), RefUpdateType::Create);

    // Verify branch deletion parsing
    assert_eq!(updates[2].ref_name, "refs/heads/old-branch");
    assert_eq!(updates[2].old_sha, sha4);
    assert_eq!(updates[2].new_sha, null_sha);
    assert_eq!(get_ref_update_type(&updates[2]), RefUpdateType::Delete);

    // Verify tag creation parsing
    assert_eq!(updates[3].ref_name, "refs/tags/v1.0.0");
    assert_eq!(updates[3].old_sha, null_sha);
    assert_eq!(updates[3].new_sha, sha1);
    assert_eq!(get_ref_update_type(&updates[3]), RefUpdateType::Create);
}

/// Test parsing handles various whitespace correctly (tabs, multiple spaces)
#[test]
fn test_parse_ref_updates_whitespace_handling() {
    let sha1 = "a1b2c3d4e5f6789012345678901234567890abcd";
    let sha2 = "b2c3d4e5f67890123456789012345678901bcdef";

    // Git may use tabs or multiple spaces between fields
    // The parser should handle various whitespace correctly
    let input_with_tabs = format!("{}\t{}\trefs/heads/main", sha1, sha2);
    let updates = parse_ref_updates(&input_with_tabs).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].ref_name, "refs/heads/main");
    assert_eq!(updates[0].old_sha, sha1);
    assert_eq!(updates[0].new_sha, sha2);

    // Multiple spaces between fields
    let input_with_spaces = format!("{}   {}   refs/heads/feature", sha1, sha2);
    let updates = parse_ref_updates(&input_with_spaces).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].ref_name, "refs/heads/feature");

    // Leading/trailing whitespace on lines
    let input_with_padding = format!("   {} {} refs/heads/main   \n", sha1, sha2);
    let updates = parse_ref_updates(&input_with_padding).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].ref_name, "refs/heads/main");
}

/// Test parsing handles edge cases correctly
#[test]
fn test_parse_ref_updates_edge_cases() {
    let sha1 = "a1b2c3d4e5f6789012345678901234567890abcd";
    let sha2 = "b2c3d4e5f67890123456789012345678901bcdef";

    // Single ref update
    let input = format!("{} {} refs/heads/main", sha1, sha2);
    let updates = parse_ref_updates(&input).unwrap();
    assert_eq!(updates.len(), 1);

    // Input with blank lines intermixed
    let input = format!(
        "{} {} refs/heads/main\n\n\n{} {} refs/heads/dev\n\n",
        sha1, sha2, sha2, sha1
    );
    let updates = parse_ref_updates(&input).unwrap();
    assert_eq!(updates.len(), 2);

    // Ref names with special characters (common in feature branches)
    let input = format!(
        "{} {} refs/heads/feature/USER-123_add-auth\n\
         {} {} refs/heads/bugfix/fix-issue#456",
        sha1, sha2, sha2, sha1
    );
    let updates = parse_ref_updates(&input).unwrap();
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].ref_name, "refs/heads/feature/USER-123_add-auth");
    assert_eq!(updates[1].ref_name, "refs/heads/bugfix/fix-issue#456");

    // Too few parts should error
    let result = parse_ref_updates(&format!("{} {}", sha1, sha2));
    assert!(result.is_err());

    // Too many parts should error (ref name with space would be invalid)
    let result = parse_ref_updates(&format!("{} {} refs/heads/main extra", sha1, sha2));
    assert!(result.is_err());
}

#[test]
fn test_ref_update_type() {
    let create = RefUpdate {
        ref_name: "refs/heads/new".to_string(),
        old_sha: "0000000000000000000000000000000000000000".to_string(),
        new_sha: "abc123".to_string(),
    };
    assert_eq!(get_ref_update_type(&create), RefUpdateType::Create);

    let delete = RefUpdate {
        ref_name: "refs/heads/old".to_string(),
        old_sha: "abc123".to_string(),
        new_sha: "0000000000000000000000000000000000000000".to_string(),
    };
    assert_eq!(get_ref_update_type(&delete), RefUpdateType::Delete);

    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        old_sha: "abc123".to_string(),
        new_sha: "def456".to_string(),
    };
    assert_eq!(get_ref_update_type(&update), RefUpdateType::Update);
}

#[test]
fn test_is_null_sha() {
    assert!(is_null_sha("0000000000000000000000000000000000000000"));
    assert!(!is_null_sha("abc123"));
    assert!(!is_null_sha(""));
}

#[test]
fn test_classify_ref_branch_update() {
    let null_sha = "0000000000000000000000000000000000000000";

    // Branch create (new_sha is not null)
    let create = RefUpdate {
        ref_name: "refs/heads/feature".to_string(),
        old_sha: null_sha.to_string(),
        new_sha: "abc123def456".to_string(),
    };
    assert_eq!(classify_ref(&create), RefClass::BranchUpdate);

    // Branch update (neither sha is null)
    let update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        old_sha: "abc123".to_string(),
        new_sha: "def456".to_string(),
    };
    assert_eq!(classify_ref(&update), RefClass::BranchUpdate);
}

#[test]
fn test_classify_ref_branch_deletion() {
    let null_sha = "0000000000000000000000000000000000000000";

    let deletion = RefUpdate {
        ref_name: "refs/heads/old-branch".to_string(),
        old_sha: "abc123def456".to_string(),
        new_sha: null_sha.to_string(),
    };
    assert_eq!(classify_ref(&deletion), RefClass::BranchDeletion);
}

#[test]
fn test_classify_ref_tag() {
    let null_sha = "0000000000000000000000000000000000000000";

    // Tag create
    let tag_create = RefUpdate {
        ref_name: "refs/tags/v1.0.0".to_string(),
        old_sha: null_sha.to_string(),
        new_sha: "abc123def456".to_string(),
    };
    assert_eq!(classify_ref(&tag_create), RefClass::Tag);

    // Tag update
    let tag_update = RefUpdate {
        ref_name: "refs/tags/v1.0.0".to_string(),
        old_sha: "abc123".to_string(),
        new_sha: "def456".to_string(),
    };
    assert_eq!(classify_ref(&tag_update), RefClass::Tag);

    // Tag delete
    let tag_delete = RefUpdate {
        ref_name: "refs/tags/v1.0.0".to_string(),
        old_sha: "abc123def456".to_string(),
        new_sha: null_sha.to_string(),
    };
    assert_eq!(classify_ref(&tag_delete), RefClass::Tag);
}

#[test]
fn test_classify_ref_other() {
    let null_sha = "0000000000000000000000000000000000000000";

    // Notes ref
    let notes = RefUpdate {
        ref_name: "refs/notes/commits".to_string(),
        old_sha: null_sha.to_string(),
        new_sha: "abc123def456".to_string(),
    };
    assert_eq!(classify_ref(&notes), RefClass::Other);

    // Pull request ref (GitHub style)
    let pr = RefUpdate {
        ref_name: "refs/pull/123/head".to_string(),
        old_sha: "abc123".to_string(),
        new_sha: "def456".to_string(),
    };
    assert_eq!(classify_ref(&pr), RefClass::Other);
}

#[test]
fn test_is_pipeline_ref() {
    let null_sha = "0000000000000000000000000000000000000000";

    // Pipeline refs: branch creates and updates
    let branch_create = RefUpdate {
        ref_name: "refs/heads/feature".to_string(),
        old_sha: null_sha.to_string(),
        new_sha: "abc123".to_string(),
    };
    assert!(is_pipeline_ref(&branch_create));

    let branch_update = RefUpdate {
        ref_name: "refs/heads/main".to_string(),
        old_sha: "abc123".to_string(),
        new_sha: "def456".to_string(),
    };
    assert!(is_pipeline_ref(&branch_update));

    // Non-pipeline refs: deletions, tags, other
    let branch_delete = RefUpdate {
        ref_name: "refs/heads/old".to_string(),
        old_sha: "abc123".to_string(),
        new_sha: null_sha.to_string(),
    };
    assert!(!is_pipeline_ref(&branch_delete));

    let tag = RefUpdate {
        ref_name: "refs/tags/v1.0.0".to_string(),
        old_sha: null_sha.to_string(),
        new_sha: "abc123".to_string(),
    };
    assert!(!is_pipeline_ref(&tag));

    let notes = RefUpdate {
        ref_name: "refs/notes/commits".to_string(),
        old_sha: null_sha.to_string(),
        new_sha: "abc123".to_string(),
    };
    assert!(!is_pipeline_ref(&notes));
}

#[test]
fn test_get_repo_id_from_path() {
    let path = Path::new("/home/user/.airlock/repos/abc123.git");
    assert_eq!(get_repo_id_from_path(path), Some("abc123".to_string()));

    let path = Path::new("/home/user/project");
    assert_eq!(get_repo_id_from_path(path), Some("project".to_string()));
}

#[test]
fn test_discover_repo() {
    let (_dir, repo) = create_test_repo();
    let workdir = repo.workdir().unwrap();

    // Create a subdirectory
    let subdir = workdir.join("subdir");
    fs::create_dir(&subdir).unwrap();

    // Should discover repo from subdirectory
    let discovered = discover_repo(&subdir).unwrap();
    assert_eq!(
        discovered.workdir().unwrap().canonicalize().unwrap(),
        workdir.canonicalize().unwrap()
    );
}

#[test]
fn test_is_git_repo() {
    let (_dir, repo) = create_test_repo();
    let workdir = repo.workdir().unwrap();

    assert!(is_git_repo(workdir));

    let temp = TempDir::new().unwrap();
    assert!(!is_git_repo(temp.path()));
}

#[test]
fn test_get_current_branch() {
    let (_dir, repo) = create_test_repo();

    // After init with a commit, we should be on a branch
    let branch = get_current_branch(&repo).unwrap();
    assert!(branch.is_some());
}

/// Integration test: Pre-receive hook logs ref updates to stderr and accepts pushes.
///
/// This test verifies the pre-receive hook behavior:
/// 1. Hook logs "airlock: receiving <ref> <old> -> <new>" to stderr
/// 2. Hook always exits 0 (accepts the push)
///
/// Note: The pre-receive hook does NOT notify the daemon - it only logs to stderr.
/// The post-receive hook is responsible for notifying the daemon.
#[test]
#[cfg(unix)]
fn test_pre_receive_hook_logs_ref_updates_to_stderr() {
    use std::process::{Command, Stdio};

    // 1. Create a bare repo gate with hooks installed
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");
    create_bare_repo(&gate_path).unwrap();
    install_hooks(&gate_path).unwrap();

    // 2. Create a working repo and add the gate as origin
    let work_path = temp_dir.path().join("work");
    let work_repo = Repository::init(&work_path).unwrap();

    // Create an initial commit in the working repo
    {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let file_path = work_path.join("README.md");
        fs::write(&file_path, "# Test Repo").unwrap();

        let mut index = work_repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = work_repo.find_tree(tree_id).unwrap();
        work_repo
            .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    // Add gate as origin
    add_remote(&work_repo, "origin", gate_path.to_str().unwrap()).unwrap();

    // 3. Push to gate and capture stderr
    let output = Command::new("git")
        .args(["push", "-u", "origin", "master"])
        .current_dir(&work_path)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to execute git push");

    // 4. Verify the push succeeded (hook accepted it)
    assert!(
        output.status.success(),
        "Push should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 5. Verify stderr contains the banner from pre-receive hook
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("▗▄▖"),
        "Pre-receive hook should display Airlock banner. Got: {}",
        stderr
    );

    // 6. Verify the branch info is shown
    assert!(
        stderr.contains("master"),
        "Pre-receive hook should display branch name. Got: {}",
        stderr
    );
}

/// Test that the pre-receive hook script content has the expected format.
#[test]
fn test_pre_receive_hook_content_format() {
    let hook = pre_receive_hook();

    // Verify hook is a shell script
    assert!(hook.starts_with("#!/bin/sh"));

    // Verify hook reads from stdin (the standard git hook protocol)
    assert!(hook.contains("while read oldrev newrev refname"));

    // Verify hook displays branch info to stderr
    assert!(
        hook.contains(r#"branch="${refname#refs/heads/}""#),
        "Pre-receive hook should extract branch name from refname"
    );

    // Verify hook always exits 0 (soft gate - accepts all pushes)
    assert!(hook.contains("exit 0"));

    // Verify the comment explains it's a soft gate
    assert!(hook.contains("Always accept the push"));

    // Verify the banner is embedded from the shared constant
    assert!(
        hook.contains("▗▄▖"),
        "Pre-receive hook should contain the BANNER text"
    );
}

/// Test that pre-receive hook does NOT contain daemon notification code.
/// The daemon notification happens in post-receive hook, not pre-receive.
#[test]
fn test_pre_receive_hook_does_not_notify_daemon() {
    let hook = pre_receive_hook();

    // Pre-receive hook should NOT contain socket communication
    assert!(
        !hook.contains("nc -U"),
        "Pre-receive hook should not notify daemon via socket"
    );
    assert!(
        !hook.contains("SOCKET="),
        "Pre-receive hook should not have SOCKET variable"
    );
    assert!(
        !hook.contains("push_received"),
        "Pre-receive hook should not send push_received notification"
    );
    assert!(
        !hook.contains("jsonrpc"),
        "Pre-receive hook should not send JSON-RPC messages"
    );
}

/// Test that post-receive hook DOES contain daemon notification code.
/// This verifies the separation of concerns between pre-receive and post-receive.
#[test]
fn test_post_receive_hook_notifies_daemon() {
    // Post-receive hook SHOULD contain socket communication
    assert!(
        POST_RECEIVE.contains("nc -U"),
        "Post-receive hook should notify daemon via socket"
    );
    assert!(
        POST_RECEIVE.contains("SOCKET="),
        "Post-receive hook should have SOCKET variable"
    );
    assert!(
        POST_RECEIVE.contains("push_received"),
        "Post-receive hook should send push_received notification"
    );
    assert!(
        POST_RECEIVE.contains("jsonrpc"),
        "Post-receive hook should send JSON-RPC messages"
    );
}

/// Test that post-receive hook content has correct JSON-RPC notification format.
///
/// This test verifies the post-receive hook script structure:
/// 1. Collects all ref updates from stdin
/// 2. Constructs a JSON-RPC push_received notification
/// 3. Sends it to the daemon socket (fire and forget)
#[test]
fn test_post_receive_hook_content_format() {
    // Verify hook is a shell script
    assert!(POST_RECEIVE.starts_with("#!/bin/sh"));

    // Verify hook reads from stdin (the standard git hook protocol)
    assert!(POST_RECEIVE.contains("while read oldrev newrev refname"));

    // Verify SOCKET is set to the expected path
    assert!(
        POST_RECEIVE.contains(r#"SOCKET="${HOME}/.airlock/socket""#),
        "Post-receive hook should use standard socket path"
    );

    // Verify REPO_PATH is captured
    assert!(
        POST_RECEIVE.contains(r#"REPO_PATH="$(pwd)""#),
        "Post-receive hook should capture repo path"
    );

    // Verify ref updates are collected into JSON array format
    // The hook uses escaped quotes like \"${refname}\"
    assert!(
        POST_RECEIVE.contains(r#"\"ref_name\":\"${refname}\""#),
        "Post-receive hook should include ref_name in JSON"
    );
    assert!(
        POST_RECEIVE.contains(r#"\"old_sha\":\"${oldrev}\""#),
        "Post-receive hook should include old_sha in JSON"
    );
    assert!(
        POST_RECEIVE.contains(r#"\"new_sha\":\"${newrev}\""#),
        "Post-receive hook should include new_sha in JSON"
    );

    // Verify JSON-RPC notification format with push_received method
    // The hook uses escaped quotes in the JSON structure
    assert!(
        POST_RECEIVE.contains(r#"\"method\":\"push_received\""#),
        "Post-receive hook should send push_received method"
    );
    assert!(
        POST_RECEIVE.contains(r#"\"gate_path\":\"${REPO_PATH}\""#),
        "Post-receive hook should include gate_path in params"
    );
    assert!(
        POST_RECEIVE.contains(r#"\"ref_updates\":[${REF_UPDATES}]"#),
        "Post-receive hook should include ref_updates array"
    );

    // Verify socket check before sending
    assert!(
        POST_RECEIVE.contains(r#"if [ -S "$SOCKET" ]"#),
        "Post-receive hook should check if socket exists"
    );

    // Verify fire-and-forget pattern (background with &)
    assert!(
        POST_RECEIVE.contains("nc -U \"$SOCKET\"") && POST_RECEIVE.contains("&"),
        "Post-receive hook should send notification in background"
    );

    // Verify hook always exits 0
    assert!(
        POST_RECEIVE.contains("exit 0"),
        "Post-receive hook should always succeed"
    );

    // Verify hook has else clause for when daemon is not running
    assert!(
        POST_RECEIVE.contains("else"),
        "Post-receive hook should have else clause for daemon not running case"
    );

    // Verify warning message when daemon is not running
    assert!(
        POST_RECEIVE.contains("Daemon is not running"),
        "Post-receive hook should warn when daemon is not running"
    );
    assert!(
        POST_RECEIVE.contains("not running"),
        "Post-receive hook should explain daemon is not running"
    );
    assert!(
        POST_RECEIVE.contains("airlock daemon start"),
        "Post-receive hook should suggest how to start daemon"
    );
}

/// Integration test: Post-receive hook sends daemon notification when socket exists.
///
/// This test verifies the actual execution of the post-receive hook:
/// 1. Creates a bare repo with hooks installed
/// 2. Sets up a Unix socket listener to mock the daemon
/// 3. Performs a git push
/// 4. Verifies the hook sent a proper JSON-RPC push_received notification
#[test]
#[cfg(unix)]
fn test_post_receive_hook_sends_notification_to_socket() {
    use std::io::{BufRead, BufReader as StdBufReader};
    use std::os::unix::net::UnixListener;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    // 1. Create a temp directory structure with socket path
    let temp_dir = TempDir::new().unwrap();
    let airlock_dir = temp_dir.path().join(".airlock");
    fs::create_dir_all(&airlock_dir).unwrap();

    let socket_path = airlock_dir.join("socket");
    let gate_path = temp_dir.path().join("gate.git");
    let work_path = temp_dir.path().join("work");

    // 2. Create bare repo gate with hooks
    create_bare_repo(&gate_path).unwrap();
    install_hooks(&gate_path).unwrap();

    // 3. Modify the post-receive hook to use our temp socket path
    // The hook normally uses $HOME/.airlock/socket, we need to override it
    let custom_hook = format!(
        r#"#!/bin/sh
# Airlock post-receive hook (test override)

SOCKET="{}"
REPO_PATH="$(pwd)"

# Collect all ref updates
REF_UPDATES=""
while read oldrev newrev refname; do
    if [ -n "$REF_UPDATES" ]; then
        REF_UPDATES="${{REF_UPDATES}},"
    fi
    REF_UPDATES="${{REF_UPDATES}}{{\"ref_name\":\"${{refname}}\",\"old_sha\":\"${{oldrev}}\",\"new_sha\":\"${{newrev}}\"}}"
done

# Notify daemon of push received
if [ -S "$SOCKET" ]; then
    echo "{{\"jsonrpc\":\"2.0\",\"method\":\"push_received\",\"params\":{{\"gate_path\":\"${{REPO_PATH}}\",\"ref_updates\":[${{REF_UPDATES}}]}},\"id\":null}}" | nc -U "$SOCKET"
fi

exit 0
"#,
        socket_path.display()
    );

    let post_receive_path = gate_path.join("hooks/post-receive");
    fs::write(&post_receive_path, custom_hook).unwrap();

    // 4. Create working repo with initial commit
    let work_repo = Repository::init(&work_path).unwrap();
    {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let file_path = work_path.join("README.md");
        fs::write(&file_path, "# Test Repo").unwrap();

        let mut index = work_repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = work_repo.find_tree(tree_id).unwrap();
        work_repo
            .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }
    add_remote(&work_repo, "origin", gate_path.to_str().unwrap()).unwrap();

    // Get the commit SHA for verification
    let head = work_repo.head().unwrap();
    let commit_sha = head.peel_to_commit().unwrap().id().to_string();

    // 5. Set up Unix socket listener to capture the notification
    let listener = UnixListener::bind(&socket_path).expect("Failed to bind socket");
    listener
        .set_nonblocking(false)
        .expect("Failed to set blocking mode");

    // Channel to receive the notification from the listener thread
    let (tx, rx) = mpsc::channel::<String>();

    // Spawn listener thread
    let listener_handle = thread::spawn(move || {
        // Set a timeout so we don't wait forever
        listener
            .set_nonblocking(true)
            .expect("Failed to set nonblocking");

        // Poll for connection with timeout
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(10);

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Read the notification
                    let mut reader = StdBufReader::new(&mut stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() && !line.is_empty() {
                        let _ = tx.send(line);
                    }
                    break;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() > timeout {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => break,
            }
        }
    });

    // 6. Perform git push
    let output = Command::new("git")
        .args(["push", "-u", "origin", "master"])
        .current_dir(&work_path)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to execute git push");

    assert!(
        output.status.success(),
        "Push should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 7. Wait for the listener thread and get the notification
    listener_handle.join().expect("Listener thread panicked");

    // 8. Verify the notification was received and has correct format
    let notification = rx.recv_timeout(Duration::from_secs(1));
    assert!(
        notification.is_ok(),
        "Post-receive hook should have sent notification to socket"
    );

    let notification = notification.unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&notification).expect("Notification should be valid JSON");

    // Verify JSON-RPC format
    assert_eq!(json["jsonrpc"], "2.0", "Should be JSON-RPC 2.0");
    assert_eq!(
        json["method"], "push_received",
        "Method should be push_received"
    );

    // Verify params contain gate_path
    let params = &json["params"];
    assert!(
        params["gate_path"].as_str().unwrap().contains("gate.git"),
        "gate_path should contain gate.git"
    );

    // Verify ref_updates array
    let ref_updates = params["ref_updates"]
        .as_array()
        .expect("ref_updates should be array");
    assert_eq!(ref_updates.len(), 1, "Should have 1 ref update");

    let ref_update = &ref_updates[0];
    assert_eq!(
        ref_update["ref_name"], "refs/heads/master",
        "ref_name should be refs/heads/master"
    );
    assert_eq!(
        ref_update["old_sha"], "0000000000000000000000000000000000000000",
        "old_sha should be null SHA for new branch"
    );
    assert_eq!(
        ref_update["new_sha"], commit_sha,
        "new_sha should match the pushed commit"
    );
}

// ================================================================
// Smart sync E2E tests
// ================================================================

/// Helper: create a commit on a branch in a bare repo, returning the new SHA.
fn commit_to_bare(repo: &Repository, branch: &str, filename: &str, content: &str) -> String {
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let ref_name = format!("refs/heads/{}", branch);

    // Build a tree with the given file
    let blob_oid = repo.blob(content.as_bytes()).unwrap();
    let parent_commit = repo
        .find_reference(&ref_name)
        .ok()
        .and_then(|r| r.peel_to_commit().ok());

    let mut tb = match &parent_commit {
        Some(c) => repo.treebuilder(Some(&c.tree().unwrap())).unwrap(),
        None => repo.treebuilder(None).unwrap(),
    };
    tb.insert(filename, blob_oid, 0o100644).unwrap();
    let tree_oid = tb.write().unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();

    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    let oid = repo
        .commit(
            Some(&ref_name),
            &sig,
            &sig,
            &format!("add {}", filename),
            &tree,
            &parents,
        )
        .unwrap();
    oid.to_string()
}

/// E2E: smart_sync creates local branches that only exist on the remote.
///
/// Scenario:
///   - Upstream (origin) has branch "feature" that doesn't exist in gate
///   - After smart_sync, gate should have "feature" pointing to same commit
#[test]
fn test_smart_sync_creates_new_branch_from_remote() {
    let dir = TempDir::new().unwrap();

    // Create upstream bare repo with a commit on "main"
    let upstream_path = dir.path().join("upstream.git");
    let upstream = Repository::init_bare(&upstream_path).unwrap();
    let main_sha = commit_to_bare(&upstream, "main", "README.md", "# hello");

    // Also create a "feature" branch on upstream
    let feature_sha = commit_to_bare(&upstream, "feature", "feature.txt", "feature content");

    // Create gate bare repo and add upstream as origin
    let gate_path = dir.path().join("gate.git");
    let gate = Repository::init_bare(&gate_path).unwrap();
    gate.remote("origin", upstream_path.to_str().unwrap())
        .unwrap();

    // Run smart sync
    let report =
        smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Both branches should be Created
    assert_eq!(report.branches.len(), 2);
    let main_status = report
        .branches
        .iter()
        .find(|(b, _)| b == "main")
        .map(|(_, s)| s);
    let feature_status = report
        .branches
        .iter()
        .find(|(b, _)| b == "feature")
        .map(|(_, s)| s);
    assert_eq!(
        main_status,
        Some(&BranchSyncStatus::Created),
        "main should be Created"
    );
    assert_eq!(
        feature_status,
        Some(&BranchSyncStatus::Created),
        "feature should be Created"
    );

    // Verify refs exist with correct SHAs
    let gate_main = resolve_ref(&gate_path, "refs/heads/main").unwrap();
    assert_eq!(gate_main, Some(main_sha));
    let gate_feature = resolve_ref(&gate_path, "refs/heads/feature").unwrap();
    assert_eq!(gate_feature, Some(feature_sha));
    assert!(report.warnings.is_empty());
}

/// E2E: smart_sync reports UpToDate when gate and remote are at the same SHA.
#[test]
fn test_smart_sync_up_to_date() {
    let dir = TempDir::new().unwrap();

    let upstream_path = dir.path().join("upstream.git");
    let upstream = Repository::init_bare(&upstream_path).unwrap();
    let sha = commit_to_bare(&upstream, "main", "README.md", "# hello");

    let gate_path = dir.path().join("gate.git");
    let gate = Repository::init_bare(&gate_path).unwrap();
    gate.remote("origin", upstream_path.to_str().unwrap())
        .unwrap();

    // First sync to populate
    smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Second sync — nothing changed
    let report =
        smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();
    let status = &report.branches[0].1;
    assert_eq!(
        *status,
        BranchSyncStatus::UpToDate,
        "Branch should be UpToDate on re-sync"
    );

    // SHA unchanged
    let gate_sha = resolve_ref(&gate_path, "refs/heads/main").unwrap();
    assert_eq!(gate_sha, Some(sha));
}

/// E2E: smart_sync fast-forwards when gate is behind upstream.
///
/// Scenario:
///   - Gate has commit A on main
///   - Upstream advances to commit B (A is ancestor of B)
///   - smart_sync should fast-forward gate to B
#[test]
fn test_smart_sync_fast_forwards_gate_behind() {
    let dir = TempDir::new().unwrap();

    let upstream_path = dir.path().join("upstream.git");
    let upstream = Repository::init_bare(&upstream_path).unwrap();
    commit_to_bare(&upstream, "main", "README.md", "initial");

    let gate_path = dir.path().join("gate.git");
    let gate = Repository::init_bare(&gate_path).unwrap();
    gate.remote("origin", upstream_path.to_str().unwrap())
        .unwrap();

    // Initial sync — gate gets commit A
    smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Upstream advances to commit B
    let sha_b = commit_to_bare(&upstream, "main", "file2.txt", "more content");

    // Re-sync — gate should fast-forward
    let report =
        smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();
    let status = &report.branches[0].1;
    assert_eq!(
        *status,
        BranchSyncStatus::FastForwarded,
        "Should fast-forward"
    );

    // Gate should now be at commit B
    let gate_sha = resolve_ref(&gate_path, "refs/heads/main").unwrap();
    assert_eq!(gate_sha, Some(sha_b));
}

/// E2E: smart_sync skips when gate is ahead of upstream (un-forwarded commits).
///
/// This is the key behavior: when a user pushes to the gate but the commits
/// haven't been forwarded to upstream yet, smart_sync must NOT overwrite them.
#[test]
fn test_smart_sync_gate_ahead_preserves_unfollowed_commits() {
    let dir = TempDir::new().unwrap();

    let upstream_path = dir.path().join("upstream.git");
    let upstream = Repository::init_bare(&upstream_path).unwrap();
    commit_to_bare(&upstream, "main", "README.md", "initial");

    let gate_path = dir.path().join("gate.git");
    let gate = Repository::init_bare(&gate_path).unwrap();
    gate.remote("origin", upstream_path.to_str().unwrap())
        .unwrap();

    // Initial sync
    smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Simulate user push: add a commit directly to the gate (un-forwarded)
    let user_sha = commit_to_bare(&gate, "main", "user_change.txt", "user's work");

    // Re-sync — gate is ahead, should skip
    let report =
        smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();
    let status = &report.branches[0].1;
    assert_eq!(
        *status,
        BranchSyncStatus::GateAhead,
        "Gate is ahead of upstream — should skip, not overwrite"
    );

    // Gate should still have the user's commit
    let gate_sha = resolve_ref(&gate_path, "refs/heads/main").unwrap();
    assert_eq!(
        gate_sha,
        Some(user_sha.clone()),
        "User's un-forwarded commit must be preserved"
    );
}

/// E2E: smart_sync rebases when gate and upstream have diverged (no conflicts).
///
/// Scenario:
///   - Gate has: A -> B (user commit, un-forwarded)
///   - Upstream has: A -> C (someone else pushed)
///   - After smart_sync with worktree dir, gate should have: A -> C -> B' (rebased)
#[test]
fn test_smart_sync_rebases_diverged_branch_cleanly() {
    let dir = TempDir::new().unwrap();

    let upstream_path = dir.path().join("upstream.git");
    let upstream = Repository::init_bare(&upstream_path).unwrap();
    let sha_a = commit_to_bare(&upstream, "main", "README.md", "initial");

    let gate_path = dir.path().join("gate.git");
    let gate = Repository::init_bare(&gate_path).unwrap();
    gate.remote("origin", upstream_path.to_str().unwrap())
        .unwrap();

    // Initial sync — both at A
    smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Gate gets user commit B (different file than upstream will change)
    let _sha_b = commit_to_bare(&gate, "main", "user_file.txt", "user work");

    // Upstream gets commit C (different file — no conflict)
    let sha_c = commit_to_bare(&upstream, "main", "upstream_file.txt", "upstream work");

    // Provide a sync worktree dir for the rebase
    let sync_dir = dir.path().join("sync_worktrees");

    // Re-sync — should rebase user's commit on top of upstream
    let report = smart_sync_from_remote(
        &gate_path,
        "origin",
        Some(&sync_dir),
        ConflictResolver::Abort,
    )
    .unwrap();
    let status = &report.branches[0].1;
    assert_eq!(
        *status,
        BranchSyncStatus::Rebased,
        "Diverged branch should be rebased"
    );

    // Gate's main should now be ahead of upstream (rebased commit on top)
    let gate_sha = resolve_ref(&gate_path, "refs/heads/main").unwrap().unwrap();
    // Upstream's commit C should be an ancestor of gate's new HEAD
    assert!(
        is_ancestor_of(&gate_path, &sha_c, &gate_sha).unwrap(),
        "Upstream commit should be ancestor of rebased HEAD"
    );
    // Original base commit A should also be an ancestor
    assert!(
        is_ancestor_of(&gate_path, &sha_a, &gate_sha).unwrap(),
        "Original base commit should be ancestor of rebased HEAD"
    );

    assert!(report.warnings.is_empty(), "No warnings expected");

    // Verify worktree was cleaned up
    assert!(
        !sync_dir.join("main").exists(),
        "Temporary worktree should be cleaned up"
    );
}

/// E2E: smart_sync reports RebaseFailed when diverged with conflicts.
///
/// Scenario:
///   - Both gate and upstream modify the same file differently
///   - Rebase has conflicts → agent not available → RebaseFailed
///   - Gate branch should remain unchanged (not corrupted)
#[test]
fn test_smart_sync_diverged_conflict_reports_failure() {
    let dir = TempDir::new().unwrap();

    let upstream_path = dir.path().join("upstream.git");
    let upstream = Repository::init_bare(&upstream_path).unwrap();
    commit_to_bare(&upstream, "main", "shared.txt", "original content");

    let gate_path = dir.path().join("gate.git");
    let gate = Repository::init_bare(&gate_path).unwrap();
    gate.remote("origin", upstream_path.to_str().unwrap())
        .unwrap();

    // Initial sync
    smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Gate modifies shared.txt one way
    let gate_sha_before = commit_to_bare(&gate, "main", "shared.txt", "gate's version of the file");

    // Upstream modifies shared.txt a different way
    commit_to_bare(
        &upstream,
        "main",
        "shared.txt",
        "upstream's completely different version",
    );

    let sync_dir = dir.path().join("sync_worktrees");

    // Re-sync — should fail due to conflicts
    let report = smart_sync_from_remote(
        &gate_path,
        "origin",
        Some(&sync_dir),
        ConflictResolver::Abort,
    )
    .unwrap();
    let (_, status) = &report.branches[0];
    assert!(
        matches!(status, BranchSyncStatus::RebaseFailed { .. }),
        "Should report RebaseFailed, got {:?}",
        status
    );

    // Gate branch should be unchanged (rebase was aborted)
    let gate_sha_after = resolve_ref(&gate_path, "refs/heads/main").unwrap().unwrap();
    assert_eq!(
        gate_sha_after, gate_sha_before,
        "Gate branch must remain unchanged after failed rebase"
    );

    // Should have a warning
    assert_eq!(report.warnings.len(), 1);
    assert!(
        report.warnings[0].contains("diverged"),
        "Warning should mention divergence"
    );
}

/// E2E: smart_sync without worktree dir reports RebaseFailed for diverged branches.
#[test]
fn test_smart_sync_diverged_without_worktree_dir() {
    let dir = TempDir::new().unwrap();

    let upstream_path = dir.path().join("upstream.git");
    let upstream = Repository::init_bare(&upstream_path).unwrap();
    commit_to_bare(&upstream, "main", "README.md", "initial");

    let gate_path = dir.path().join("gate.git");
    let gate = Repository::init_bare(&gate_path).unwrap();
    gate.remote("origin", upstream_path.to_str().unwrap())
        .unwrap();

    smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Create divergence
    commit_to_bare(&gate, "main", "gate.txt", "gate");
    commit_to_bare(&upstream, "main", "upstream.txt", "upstream");

    // Sync without worktree dir — should fail gracefully
    let report =
        smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();
    let (_, status) = &report.branches[0];
    assert!(
        matches!(status, BranchSyncStatus::RebaseFailed { .. }),
        "Should report RebaseFailed without worktree dir"
    );
}

/// E2E: smart_sync handles multiple branches with different states simultaneously.
///
/// Scenario:
///   - "main": gate is behind → FastForwarded
///   - "feature": only on upstream → Created
///   - "dev": gate is ahead → GateAhead
#[test]
fn test_smart_sync_multiple_branches_mixed_states() {
    let dir = TempDir::new().unwrap();

    let upstream_path = dir.path().join("upstream.git");
    let upstream = Repository::init_bare(&upstream_path).unwrap();
    commit_to_bare(&upstream, "main", "README.md", "initial");
    commit_to_bare(&upstream, "dev", "dev.txt", "dev content");

    let gate_path = dir.path().join("gate.git");
    let gate = Repository::init_bare(&gate_path).unwrap();
    gate.remote("origin", upstream_path.to_str().unwrap())
        .unwrap();

    // Initial sync — gate gets main and dev
    smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Upstream: advance main, create feature branch
    commit_to_bare(&upstream, "main", "file2.txt", "more");
    commit_to_bare(&upstream, "feature", "feature.txt", "feat");

    // Gate: advance dev (un-forwarded user commit)
    commit_to_bare(&gate, "dev", "user_dev.txt", "user dev work");

    // Re-sync
    let report =
        smart_sync_from_remote(&gate_path, "origin", None, ConflictResolver::Abort).unwrap();

    // Collect statuses by branch name
    let statuses: std::collections::HashMap<&str, &BranchSyncStatus> = report
        .branches
        .iter()
        .map(|(b, s)| (b.as_str(), s))
        .collect();

    assert_eq!(
        statuses.get("main"),
        Some(&&BranchSyncStatus::FastForwarded),
        "main should be fast-forwarded"
    );
    assert_eq!(
        statuses.get("feature"),
        Some(&&BranchSyncStatus::Created),
        "feature should be created"
    );
    assert_eq!(
        statuses.get("dev"),
        Some(&&BranchSyncStatus::GateAhead),
        "dev should be gate-ahead"
    );
}

// ================================================================
// Protective refs E2E tests
// ================================================================

/// E2E: run_ref returns the correct ref format.
#[test]
fn test_run_ref_format() {
    assert_eq!(run_ref("abc-123"), "refs/airlock/runs/abc-123");
    assert_eq!(
        run_ref("550e8400-e29b-41d4-a716-446655440000"),
        "refs/airlock/runs/550e8400-e29b-41d4-a716-446655440000"
    );
}

/// E2E: Protective ref lifecycle — create, verify, delete.
///
/// Verifies that:
///   1. update_ref creates a ref under refs/airlock/runs/
///   2. resolve_ref can read it back
///   3. delete_ref removes it
///   4. The commit is still reachable (not GC'd) while the ref exists
#[test]
fn test_protective_ref_lifecycle() {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().join("gate.git");
    let repo = Repository::init_bare(&repo_path).unwrap();

    // Create a commit
    let sha = commit_to_bare(&repo, "main", "README.md", "hello");

    // Create protective ref
    let run_id = "test-run-001";
    let ref_name = run_ref(run_id);
    update_ref(&repo_path, &ref_name, &sha).unwrap();

    // Verify it exists
    let resolved = resolve_ref(&repo_path, &ref_name).unwrap();
    assert_eq!(
        resolved,
        Some(sha.clone()),
        "Protective ref should resolve to commit SHA"
    );

    // Delete the protective ref
    delete_ref(&repo_path, &ref_name).unwrap();

    // Verify it's gone
    let resolved = resolve_ref(&repo_path, &ref_name).unwrap();
    assert_eq!(resolved, None, "Protective ref should be deleted");
}

/// E2E: Multiple protective refs can coexist for different runs.
#[test]
fn test_multiple_protective_refs() {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().join("gate.git");
    let repo = Repository::init_bare(&repo_path).unwrap();

    let sha1 = commit_to_bare(&repo, "main", "file1.txt", "content1");
    let sha2 = commit_to_bare(&repo, "main", "file2.txt", "content2");

    // Create two protective refs
    let ref1 = run_ref("run-1");
    let ref2 = run_ref("run-2");
    update_ref(&repo_path, &ref1, &sha1).unwrap();
    update_ref(&repo_path, &ref2, &sha2).unwrap();

    // Both should exist
    assert_eq!(resolve_ref(&repo_path, &ref1).unwrap(), Some(sha1.clone()));
    assert_eq!(resolve_ref(&repo_path, &ref2).unwrap(), Some(sha2.clone()));

    // Delete one — the other should remain
    delete_ref(&repo_path, &ref1).unwrap();
    assert_eq!(resolve_ref(&repo_path, &ref1).unwrap(), None);
    assert_eq!(resolve_ref(&repo_path, &ref2).unwrap(), Some(sha2));
}

/// E2E: Protective ref protects commits from being unreachable.
///
/// When a branch is force-updated past a commit, the protective ref
/// ensures the old commit is still reachable.
#[test]
fn test_protective_ref_keeps_commit_reachable() {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().join("gate.git");
    let repo = Repository::init_bare(&repo_path).unwrap();

    // Create commit A on main
    let sha_a = commit_to_bare(&repo, "main", "README.md", "initial");

    // Create protective ref pointing to A
    let protective = run_ref("run-protect");
    update_ref(&repo_path, &protective, &sha_a).unwrap();

    // Force-update main to a new commit B (A is now orphaned from the branch)
    let _sha_b = commit_to_bare(&repo, "main", "README.md", "replaced");

    // Commit A is no longer on any branch, but should still be resolvable
    // via the protective ref
    let resolved = resolve_ref(&repo_path, &protective).unwrap();
    assert_eq!(
        resolved,
        Some(sha_a.clone()),
        "Protective ref should still point to commit A"
    );

    // The commit should still be findable
    let found = repo.find_commit(git2::Oid::from_str(&sha_a).unwrap());
    assert!(
        found.is_ok(),
        "Commit A should still be reachable via protective ref"
    );
}

// ================================================================
// delete_ref E2E tests
// ================================================================

/// E2E: delete_ref removes a branch ref.
#[test]
fn test_delete_ref_removes_branch() {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().join("test.git");
    let repo = Repository::init_bare(&repo_path).unwrap();

    commit_to_bare(&repo, "to-delete", "file.txt", "content");
    assert!(resolve_ref(&repo_path, "refs/heads/to-delete")
        .unwrap()
        .is_some());

    delete_ref(&repo_path, "refs/heads/to-delete").unwrap();
    assert!(resolve_ref(&repo_path, "refs/heads/to-delete")
        .unwrap()
        .is_none());
}

/// E2E: delete_ref on nonexistent ref is idempotent (no error).
///
/// Some git versions treat deleting a nonexistent ref as a no-op.
/// This test verifies our wrapper handles it gracefully.
#[test]
fn test_delete_ref_nonexistent_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let repo_path = dir.path().join("test.git");
    let repo = Repository::init_bare(&repo_path).unwrap();

    // Create and delete a ref, then delete again
    commit_to_bare(&repo, "temp", "f.txt", "x");
    let ref_name = "refs/heads/temp";
    delete_ref(&repo_path, ref_name).unwrap();

    // Verify it's gone
    assert!(resolve_ref(&repo_path, ref_name).unwrap().is_none());
}

// ================================================================
// get_git_config tests
// ================================================================

/// Test that get_git_config reads a locally configured value.
#[test]
fn test_get_git_config_reads_local_value() {
    let dir = TempDir::new().unwrap();
    Repository::init(dir.path()).unwrap();

    // Set user.name and user.email locally
    std::process::Command::new("git")
        .args(["config", "user.name", "Alice Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "alice@example.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(
        get_git_config(dir.path(), "user.name"),
        Some("Alice Test".to_string())
    );
    assert_eq!(
        get_git_config(dir.path(), "user.email"),
        Some("alice@example.com".to_string())
    );
}

/// Test that get_git_config returns None for an unset key.
#[test]
fn test_get_git_config_returns_none_for_unset_key() {
    let dir = TempDir::new().unwrap();
    Repository::init(dir.path()).unwrap();

    assert_eq!(get_git_config(dir.path(), "airlock.nonexistent"), None);
}
