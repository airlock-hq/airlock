//! Fixture generator for Airlock.
//!
//! This binary generates fixtures by exporting data from the Airlock SQLite database.
//! It uses the exact same IPC types as the daemon to ensure generated fixtures match
//! production serialization exactly.
//!
//! Usage:
//!   cargo run --bin generate-fixtures
//!   cargo run --bin generate-fixtures -- --db-path /path/to/state.sqlite

mod db_export;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "generate-fixtures")]
#[command(about = "Generate fixtures from Airlock database for frontend testing")]
struct Args {
    /// Path to database file (default: ~/.airlock/state.sqlite)
    #[arg(long)]
    db_path: Option<PathBuf>,

    /// Output directory for fixtures (default: crates/airlock-app/fixtures)
    #[arg(long, short = 'o')]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Determine database path
    let db_path = args.db_path.unwrap_or_else(|| {
        dirs::home_dir()
            .expect("Could not find home directory")
            .join(".airlock/state.sqlite")
    });

    if !db_path.exists() {
        eprintln!("Error: Database not found at {}", db_path.display());
        eprintln!();
        eprintln!("To generate fixtures, you need a populated database. Try:");
        eprintln!("  1. Initialize airlock in a test repo: airlock init");
        eprintln!("  2. Start the daemon: airlock daemon start");
        eprintln!("  3. Push some changes to trigger pipeline runs");
        eprintln!("  4. Run this command again");
        std::process::exit(1);
    }

    // Determine output directory
    let output_dir = args
        .output
        .unwrap_or_else(|| PathBuf::from("crates/airlock-app/fixtures"));

    // Clean and recreate output directory
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)?;
    }
    std::fs::create_dir_all(&output_dir)?;

    println!("Generating fixtures from database...");
    println!("  Database: {}", db_path.display());
    println!("  Output: {}", output_dir.display());
    println!();

    // Export from database
    db_export::export_from_database(&db_path, &output_dir)?;

    println!();
    println!("Fixtures generated at {}", output_dir.display());

    Ok(())
}
