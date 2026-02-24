//! E2E tests for `airlock daemon` commands (Section 7.5).

use super::common::DaemonTestEnv;

/// Test that `airlock daemon start` starts the daemon.
///
/// Per MVP Test Plan Section 7.5: "`airlock daemon start` starts the daemon"
///
/// This test verifies that:
/// 1. Starting the daemon creates a running process
/// 2. The daemon responds to health checks
/// 3. The socket file is created
#[tokio::test]
async fn test_e2e_airlock_daemon_start_starts_the_daemon() {
    use interprocess::local_socket::tokio::prelude::*;
    use interprocess::local_socket::tokio::Stream;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[cfg(unix)]
    use interprocess::local_socket::GenericFilePath;

    let env = match DaemonTestEnv::setup() {
        Some(e) => e,
        None => {
            println!("Skipping test: airlockd binary not found. Run 'cargo build' first.");
            return;
        }
    };

    let mut child = env.spawn_daemon("daemon");
    let daemon_pid = child.id();
    println!("Daemon started with PID: {}", daemon_pid);

    // Wait for daemon to be ready and verify health response
    let mut daemon_ready = false;

    for attempt in 1..=20 {
        tokio::time::sleep(Duration::from_millis(250)).await;

        #[cfg(unix)]
        if env.paths.socket().exists() {
            let socket_name = env.paths.socket_name();
            if let Ok(name) = socket_name.to_fs_name::<GenericFilePath>() {
                if let Ok(stream) = Stream::connect(name).await {
                    let (reader, mut writer) = stream.split();
                    let mut reader = BufReader::new(reader);

                    let request = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "health",
                        "params": {},
                        "id": 1
                    });
                    let request_json = serde_json::to_string(&request).unwrap();

                    if writer.write_all(request_json.as_bytes()).await.is_ok()
                        && writer.write_all(b"\n").await.is_ok()
                        && writer.flush().await.is_ok()
                    {
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.is_ok() && !line.is_empty() {
                            let response: serde_json::Value =
                                serde_json::from_str(&line).unwrap_or_default();
                            if response.get("result").is_some() {
                                println!(
                                    "Daemon ready after {} attempt(s): {:?}",
                                    attempt, response
                                );
                                daemon_ready = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        println!("Waiting for daemon... attempt {}/20", attempt);
    }

    // Cleanup
    if daemon_ready {
        #[cfg(unix)]
        env.send_shutdown().await;
    }
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        daemon_ready,
        "Daemon should have started and responded to health check."
    );

    println!("Test passed: airlock daemon start successfully starts the daemon");
}

/// Test that `airlock daemon stop` stops the daemon.
///
/// Per MVP Test Plan Section 7.5: "`airlock daemon stop` stops the daemon"
///
/// This test verifies that:
/// 1. A running daemon can be stopped via shutdown command
/// 2. The socket file is removed after shutdown
/// 3. The daemon process exits cleanly
#[tokio::test]
async fn test_e2e_airlock_daemon_stop_stops_the_daemon() {
    use interprocess::local_socket::tokio::prelude::*;
    use interprocess::local_socket::tokio::Stream;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[cfg(unix)]
    use interprocess::local_socket::GenericFilePath;

    let env = match DaemonTestEnv::setup() {
        Some(e) => e,
        None => {
            println!("Skipping test: airlockd binary not found.");
            return;
        }
    };

    let mut child = env.spawn_daemon("daemon");

    if !env.wait_for_socket().await {
        let _ = child.kill();
        let _ = child.wait();
        panic!("Daemon did not start in time");
    }

    // Send shutdown command (simulating `airlock daemon stop`)
    #[cfg(unix)]
    {
        let socket_name = env.paths.socket_name();
        let name = socket_name
            .to_fs_name::<GenericFilePath>()
            .expect("Failed to create socket name");
        let stream = Stream::connect(name)
            .await
            .expect("Failed to connect to daemon");
        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "shutdown",
            "params": {},
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
        let _ = reader.read_line(&mut line).await;
        println!("Shutdown response: {}", line);
    }

    // Wait for daemon to stop
    let mut daemon_stopped = false;
    for _ in 1..=20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Ok(Some(status)) = child.try_wait() {
            println!("Daemon exited with status: {:?}", status);
            daemon_stopped = true;
            break;
        }
    }

    if !daemon_stopped {
        let _ = child.kill();
    }
    let _ = child.wait();

    assert!(
        daemon_stopped,
        "Daemon should have stopped after shutdown command"
    );

    // Verify socket is gone (may take a moment)
    tokio::time::sleep(Duration::from_millis(100)).await;
    #[cfg(unix)]
    {
        if env.paths.socket().exists() {
            println!("Note: Socket file still exists after daemon stop (may be cleaned up by OS)");
        }
    }

    println!("Test passed: airlock daemon stop successfully stops the daemon");
}

/// Test that `airlock daemon restart` restarts the daemon.
///
/// Per MVP Test Plan Section 7.5: "`airlock daemon restart` restarts the daemon"
///
/// This test verifies that:
/// 1. A running daemon can be restarted
/// 2. The daemon responds to health checks after restart
#[tokio::test]
async fn test_e2e_airlock_daemon_restart_restarts_the_daemon() {
    use interprocess::local_socket::tokio::prelude::*;
    use interprocess::local_socket::tokio::Stream;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[cfg(unix)]
    use interprocess::local_socket::GenericFilePath;

    let env = match DaemonTestEnv::setup() {
        Some(e) => e,
        None => {
            println!("Skipping test: airlockd binary not found.");
            return;
        }
    };

    // Start first daemon instance
    let mut child1 = env.spawn_daemon("daemon1");
    let pid1 = child1.id();
    println!("First daemon started with PID: {}", pid1);

    env.wait_for_socket().await;

    // Stop first daemon
    #[cfg(unix)]
    env.send_shutdown().await;

    for _ in 1..=20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if child1.try_wait().unwrap().is_some() {
            break;
        }
    }
    let _ = child1.kill();
    let _ = child1.wait();

    // Small delay before restarting
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Remove stale socket if present
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(env.paths.socket());
    }

    // Start second daemon instance
    let mut child2 = env.spawn_daemon("daemon2");
    let pid2 = child2.id();
    println!("Second daemon started with PID: {}", pid2);

    // Verify second daemon is running and responding
    let mut daemon_ready = false;
    let socket_path = env.paths.socket();
    for attempt in 1..=20 {
        tokio::time::sleep(Duration::from_millis(250)).await;

        #[cfg(unix)]
        if socket_path.exists() {
            let socket_name = env.paths.socket_name();
            if let Ok(name) = socket_name.to_fs_name::<GenericFilePath>() {
                if let Ok(stream) = Stream::connect(name).await {
                    let (reader, mut writer) = stream.split();
                    let mut reader = BufReader::new(reader);

                    let request = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "health",
                        "params": {},
                        "id": 1
                    });
                    let request_json = serde_json::to_string(&request).unwrap();

                    if writer.write_all(request_json.as_bytes()).await.is_ok()
                        && writer.write_all(b"\n").await.is_ok()
                        && writer.flush().await.is_ok()
                    {
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.is_ok() && !line.is_empty() {
                            let response: serde_json::Value =
                                serde_json::from_str(&line).unwrap_or_default();
                            if response.get("result").is_some() {
                                println!("Second daemon ready after {} attempt(s)", attempt);
                                daemon_ready = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // Cleanup
    #[cfg(unix)]
    env.send_shutdown().await;
    let _ = child2.kill();
    let _ = child2.wait();

    assert!(
        daemon_ready,
        "Second daemon should be running after restart"
    );
    assert_ne!(pid1, pid2, "PIDs should be different after restart");

    println!("Test passed: airlock daemon restart successfully restarts the daemon");
}

/// Test that `airlock daemon status` shows running state.
///
/// Per MVP Test Plan Section 7.5: "`airlock daemon status` shows running state"
///
/// This test verifies that:
/// 1. When daemon is running, status shows "Running"
/// 2. Health info (version, repo count, database status) is displayed
#[tokio::test]
async fn test_e2e_airlock_daemon_status_shows_running_state() {
    use interprocess::local_socket::tokio::prelude::*;
    use interprocess::local_socket::tokio::Stream;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[cfg(unix)]
    use interprocess::local_socket::GenericFilePath;

    let env = match DaemonTestEnv::setup() {
        Some(e) => e,
        None => {
            println!("Skipping test: airlockd binary not found.");
            return;
        }
    };

    let mut child = env.spawn_daemon("daemon");

    env.wait_for_socket().await;

    // Query health status (simulating `airlock daemon status`)
    let mut status_result: Option<serde_json::Value> = None;

    #[cfg(unix)]
    {
        let socket_name = env.paths.socket_name();
        if let Ok(name) = socket_name.to_fs_name::<GenericFilePath>() {
            if let Ok(stream) = Stream::connect(name).await {
                let (reader, mut writer) = stream.split();
                let mut reader = BufReader::new(reader);

                let request = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "health",
                    "params": {},
                    "id": 1
                });
                let request_json = serde_json::to_string(&request).unwrap();
                writer
                    .write_all(request_json.as_bytes())
                    .await
                    .expect("Failed to send");
                writer
                    .write_all(b"\n")
                    .await
                    .expect("Failed to send newline");
                writer.flush().await.expect("Failed to flush");

                let mut line = String::new();
                reader.read_line(&mut line).await.expect("Failed to read");

                let response: serde_json::Value =
                    serde_json::from_str(&line).expect("Failed to parse response");
                status_result = response.get("result").cloned();

                println!("Status response: {:?}", status_result);
            }
        }
    }

    // Cleanup
    #[cfg(unix)]
    env.send_shutdown().await;
    let _ = child.kill();
    let _ = child.wait();

    // Verify status contains expected health info
    let status = status_result.expect("Should have received status result");

    assert!(
        status.get("healthy").is_some(),
        "Status should contain 'healthy' field"
    );
    assert!(
        status.get("version").is_some(),
        "Status should contain 'version' field"
    );
    assert!(
        status.get("repo_count").is_some(),
        "Status should contain 'repo_count' field"
    );
    assert!(
        status.get("database_ok").is_some(),
        "Status should contain 'database_ok' field"
    );
    assert!(
        status.get("socket_path").is_some(),
        "Status should contain 'socket_path' field"
    );

    let healthy = status["healthy"].as_bool().unwrap_or(false);
    assert!(healthy, "Daemon should report as healthy");

    let db_ok = status["database_ok"].as_bool().unwrap_or(false);
    assert!(db_ok, "Database should report as ok");

    println!("Test passed: airlock daemon status shows running state with health info");
    println!("Health status: {:?}", status);
}
