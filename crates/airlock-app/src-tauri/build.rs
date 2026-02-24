use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Auto-build frontend if:
    // 1. AIRLOCK_AUTO_BUILD_FRONTEND=1 is set, OR
    // 2. The dist folder doesn't exist (first build)
    let auto_build = env::var("AIRLOCK_AUTO_BUILD_FRONTEND")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let frontend_dir = Path::new(&manifest_dir).parent().unwrap();
    let dist_dir = frontend_dir.join("dist");

    let should_build = auto_build || !dist_dir.exists();

    if should_build {
        println!("cargo:warning=Building frontend assets...");

        // Check if node_modules exists, if not run npm ci
        let node_modules = frontend_dir.join("node_modules");
        if !node_modules.exists() {
            println!("cargo:warning=Installing frontend dependencies...");
            let status = Command::new("npm")
                .arg("ci")
                .current_dir(frontend_dir)
                .status()
                .expect("Failed to run npm ci");

            if !status.success() {
                panic!("npm ci failed");
            }
        }

        // Build the frontend
        let status = Command::new("npm")
            .arg("run")
            .arg("build")
            .current_dir(frontend_dir)
            .status()
            .expect("Failed to run npm build");

        if !status.success() {
            panic!("Frontend build failed");
        }

        println!("cargo:warning=Frontend build complete.");
    }

    // Tell cargo to re-run if frontend source changes
    println!("cargo:rerun-if-changed=../src");
    println!("cargo:rerun-if-changed=../index.html");
    println!("cargo:rerun-if-changed=../package.json");
    println!("cargo:rerun-if-env-changed=AIRLOCK_AUTO_BUILD_FRONTEND");

    tauri_build::build()
}
