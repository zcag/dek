mod config;
mod output;
mod providers;
mod runner;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dek")]
#[command(version, about = "Declarative environment setup from TOML")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply configuration (install packages, setup files, etc.)
    Apply {
        /// Config file or directory (default: dek.toml or dek/)
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,
    },
    /// Check what would change without applying
    Check {
        /// Config file or directory (default: dek.toml or dek/)
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Apply { config } => run_apply(config),
        Commands::Check { config } => run_check(config),
    }
}

fn resolve_config(config: Option<PathBuf>) -> Result<PathBuf> {
    match config {
        Some(path) => Ok(path),
        None => config::find_default_config()
            .ok_or_else(|| anyhow::anyhow!("No config found. Create dek.toml or dek/ directory")),
    }
}

fn run_apply(config_path: Option<PathBuf>) -> Result<()> {
    let path = resolve_config(config_path)?;
    println!("Applying config from: {}", path.display());

    let config = config::load(&path)?;
    let runner = runner::Runner::new(false);
    runner.run(&config)
}

fn run_check(config_path: Option<PathBuf>) -> Result<()> {
    let path = resolve_config(config_path)?;
    println!("Checking config from: {}", path.display());

    let config = config::load(&path)?;
    let runner = runner::Runner::new(true);
    runner.run(&config)
}
