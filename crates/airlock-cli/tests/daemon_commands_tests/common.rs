use airlock_core::AirlockPaths;
use std::path::PathBuf;
use std::process::{Child, Command};
use tempfile::TempDir;

/// Shared test environment for daemon lifecycle tests.
pub struct DaemonTestEnv {
    pub _temp_dir: TempDir,
    pub airlock_home: PathBuf,
    pub paths: AirlockPaths,
    pub daemon_path: PathBuf,
}

impl DaemonTestEnv {
    /// Set up an isolated daemon test environment.
    /// Returns `None` if the daemon binary is not found (test should be skipped).
    pub fn setup() -> Option<Self> {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let airlock_home = temp_dir.path().join("airlock_home");
        std::fs::create_dir_all(&airlock_home).expect("Failed to create airlock home");

        let paths = AirlockPaths::with_root(airlock_home.clone());
        paths.ensure_dirs().expect("Failed to ensure dirs");

        // Create the database so daemon can connect
        let db =
            airlock_core::Database::open(&paths.database()).expect("Failed to create database");
        drop(db);

        let daemon_path = find_daemon_binary()?;

        Some(Self {
            _temp_dir: temp_dir,
            airlock_home,
            paths,
            daemon_path,
        })
    }

    /// Spawn the daemon with log file redirection.
    /// `log_prefix` is used to create unique log file names (e.g., "daemon", "daemon1").
    pub fn spawn_daemon(&self, log_prefix: &str) -> Child {
        let logs_dir = self.airlock_home.join("logs");
        std::fs::create_dir_all(&logs_dir).expect("Failed to create logs dir");

        let stdout_file =
            std::fs::File::create(logs_dir.join(format!("{log_prefix}.stdout.log"))).unwrap();
        let stderr_file =
            std::fs::File::create(logs_dir.join(format!("{log_prefix}.stderr.log"))).unwrap();

        Command::new(&self.daemon_path)
            .env("AIRLOCK_HOME", &self.airlock_home)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .spawn()
            .expect("Failed to start daemon")
    }

    /// Wait for the daemon socket to appear.
    /// Returns `true` if the socket appeared within the timeout.
    pub async fn wait_for_socket(&self) -> bool {
        use std::time::Duration;
        let socket_path = self.paths.socket();
        for _ in 1..=20 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if socket_path.exists() {
                return true;
            }
        }
        false
    }

    /// Send a shutdown request to the daemon and wait briefly for it to process.
    #[cfg(unix)]
    pub async fn send_shutdown(&self) {
        use interprocess::local_socket::tokio::prelude::*;
        use interprocess::local_socket::tokio::Stream;
        use interprocess::local_socket::GenericFilePath;
        use tokio::io::AsyncWriteExt;

        let socket_name = self.paths.socket_name();
        if let Ok(name) = socket_name.to_fs_name::<GenericFilePath>() {
            if let Ok(stream) = Stream::connect(name).await {
                let (_reader, mut writer) = stream.split();
                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "shutdown",
                    "params": {},
                    "id": 1
                });
                let request_json = serde_json::to_string(&request).unwrap();
                let _ = writer.write_all(request_json.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
                let _ = writer.flush().await;
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Send a JSON-RPC request to the daemon and return the `result` field.
///
/// Connects to the daemon socket, sends the request, reads the response,
/// and returns the `result` value. Panics if the response contains an error.
#[cfg(unix)]
pub async fn send_rpc(
    paths: &AirlockPaths,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    use interprocess::local_socket::tokio::prelude::*;
    use interprocess::local_socket::tokio::Stream;
    use interprocess::local_socket::GenericFilePath;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let socket_name = paths.socket_name();
    let name = socket_name
        .to_fs_name::<GenericFilePath>()
        .expect("Failed to create socket name");
    let stream = Stream::connect(name)
        .await
        .expect("Failed to connect to daemon socket");
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });
    let request_json = serde_json::to_string(&request).unwrap();
    writer
        .write_all(request_json.as_bytes())
        .await
        .expect("Failed to send request");
    writer
        .write_all(b"\n")
        .await
        .expect("Failed to send newline");
    writer.flush().await.expect("Failed to flush");

    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .expect("Failed to read response");

    let response: serde_json::Value =
        serde_json::from_str(&line).expect("Failed to parse JSON-RPC response");

    if let Some(error) = response.get("error") {
        panic!(
            "RPC '{}' returned error: {}",
            method,
            serde_json::to_string_pretty(error).unwrap()
        );
    }

    response
        .get("result")
        .cloned()
        .expect("Response missing 'result' field")
}

/// Find the daemon binary in the build output directory.
fn find_daemon_binary() -> Option<PathBuf> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("target/debug/airlockd"))
        .filter(|p| p.exists())
}
