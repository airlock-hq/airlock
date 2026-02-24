use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use airlock_core::{AirlockPaths, Database};
use git2::Repository;
use tempfile::TempDir;

pub(crate) fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

pub(crate) fn create_test_working_repo(dir: &Path) -> Repository {
    let repo = Repository::init(dir).expect("Failed to init repo");

    // Create an initial commit
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

    repo
}

pub(crate) fn create_test_gate_repo(dir: &Path) -> Repository {
    fs::create_dir_all(dir).unwrap();
    let repo = Repository::init_bare(dir).expect("Failed to init bare repo");
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();
    repo
}

/// Common test environment for doctor tests.
/// Sets up a TempDir, working directory, AirlockPaths, and Database.
pub(crate) struct TestEnv {
    pub _temp_dir: TempDir,
    pub working_dir: PathBuf,
    pub gate_path: PathBuf,
    pub paths: AirlockPaths,
    pub db: Database,
    pub repo: Repository,
}

impl TestEnv {
    pub fn setup() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let airlock_root = temp_dir.path().join("airlock");
        let gate_path = airlock_root.join("repos").join("test123.git");

        fs::create_dir_all(&working_dir).unwrap();
        let repo = create_test_working_repo(&working_dir);

        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let db = Database::open(&paths.database()).unwrap();

        Self {
            _temp_dir: temp_dir,
            working_dir,
            gate_path,
            paths,
            db,
            repo,
        }
    }
}
