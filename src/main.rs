mod config;
mod output;
mod providers;
mod runner;
mod util;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(name = "dek")]
#[command(version, about = "Declarative environment setup from TOML")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply configuration
    Apply {
        /// Config file or directory (default: dek.toml or dek/)
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,
    },
    /// Check what would change (dry-run)
    Check {
        /// Config file or directory
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,
    },
    /// List items from config (no state check)
    Plan {
        /// Config file or directory
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,
    },
    /// Spin up container, apply config, drop into shell
    Test {
        /// Config file or directory
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,

        /// Base image (default: archlinux)
        #[arg(short, long, default_value = "archlinux")]
        image: String,

        /// Keep container after exit
        #[arg(short, long)]
        keep: bool,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Apply { config } => run_apply(config),
        Commands::Check { config } => run_check(config),
        Commands::Plan { config } => run_plan(config),
        Commands::Test { config, image, keep } => run_test(config, image, keep),
        Commands::Completions { shell } => {
            generate(shell, &mut Cli::command(), "dek", &mut io::stdout());
            Ok(())
        }
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
    output::print_header(&format!("Applying {}", path.display()));
    println!();

    let config = config::load(&path)?;
    let runner = runner::Runner::new(runner::Mode::Apply);
    runner.run(&config)
}

fn run_check(config_path: Option<PathBuf>) -> Result<()> {
    let path = resolve_config(config_path)?;
    output::print_header(&format!("Checking {}", path.display()));
    println!();

    let config = config::load(&path)?;
    let runner = runner::Runner::new(runner::Mode::Check);
    runner.run(&config)
}

fn run_plan(config_path: Option<PathBuf>) -> Result<()> {
    let path = resolve_config(config_path)?;
    output::print_header(&format!("Plan for {}", path.display()));
    println!();

    let config = config::load(&path)?;
    let runner = runner::Runner::new(runner::Mode::Plan);
    runner.run(&config)
}

fn run_test(config_path: Option<PathBuf>, image: String, keep: bool) -> Result<()> {
    // Check docker is available
    if which::which("docker").is_err() {
        bail!("docker not found in PATH");
    }

    let config_path = resolve_config(config_path)?;
    let config_abs = std::fs::canonicalize(&config_path)?;
    let cwd = std::env::current_dir()?;

    output::print_header(&format!("Testing {} in {}", config_path.display(), image));
    println!();

    // Build docker args
    let container_name = format!("dek-test-{}", std::process::id());
    let mut args = vec![
        "run".to_string(),
        "-it".to_string(),
        "--name".to_string(),
        container_name.clone(),
    ];

    if !keep {
        args.push("--rm".to_string());
    }

    // Mount current directory
    args.push("-v".to_string());
    args.push(format!("{}:/workspace", cwd.display()));

    // Mount config if it's outside cwd
    if !config_abs.starts_with(&cwd) {
        args.push("-v".to_string());
        args.push(format!("{}:/config:ro", config_abs.display()));
    }

    args.push("-w".to_string());
    args.push("/workspace".to_string());

    args.push(image);

    // Run shell with dek apply
    args.push("sh".to_string());
    args.push("-c".to_string());

    let config_in_container = if config_abs.starts_with(&cwd) {
        format!("/workspace/{}", config_path.display())
    } else {
        "/config".to_string()
    };

    args.push(format!(
        r#"echo "Installing dek..." && \
        (command -v cargo >/dev/null 2>&1 || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && source $HOME/.cargo/env)) && \
        cargo install --path /workspace --quiet 2>/dev/null || cargo install dek --quiet && \
        echo "" && \
        dek apply {} && \
        echo "" && \
        echo "Dropping into shell..." && \
        exec sh"#,
        config_in_container
    ));

    // Run docker
    let status = Command::new("docker")
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        bail!("docker exited with status {}", status);
    }

    if keep {
        println!();
        println!("Container kept: {}", container_name);
        println!("  docker start -ai {}", container_name);
        println!("  docker rm {}", container_name);
    }

    Ok(())
}
