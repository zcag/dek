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
    /// Generate shell completions (raw output)
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Install dek completions for your shell
    Setup,
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
        Commands::Setup => run_setup(),
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
    use owo_colors::OwoColorize;

    // Check docker is available
    if which::which("docker").is_err() {
        bail!("docker not found in PATH");
    }

    let config_path = resolve_config(config_path)?;
    let config_abs = std::fs::canonicalize(&config_path)?;
    let cwd = std::env::current_dir()?;

    output::print_header(&format!("Testing {} in {}", config_path.display(), image));
    println!();

    // Build dek binary
    println!("  {} Building dek...", "→".yellow());
    let build_status = Command::new("cargo")
        .args(["build", "--release", "--quiet"])
        .status()?;
    if !build_status.success() {
        bail!("cargo build failed");
    }

    // Find the built binary
    let dek_binary = cwd.join("target/release/dek");
    if !dek_binary.exists() {
        bail!("dek binary not found at {}", dek_binary.display());
    }

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

    // Mount dek binary
    args.push("-v".to_string());
    args.push(format!("{}:/usr/local/bin/dek:ro", dek_binary.display()));

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

    // Config path inside container
    let config_in_container = if config_abs.starts_with(&cwd) {
        format!("/workspace/{}", config_path.display())
    } else {
        "/config".to_string()
    };

    // Run shell with dek apply
    args.push("sh".to_string());
    args.push("-c".to_string());
    args.push(format!(
        r#"dek apply {} && echo "" && echo "Dropping into shell..." && exec sh"#,
        config_in_container
    ));

    println!("  {} Starting container...", "→".yellow());
    println!();

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

fn run_setup() -> Result<()> {
    use owo_colors::OwoColorize;
    use std::fs;

    output::print_header("Setting up dek");
    println!();

    let shell = detect_shell();
    println!("  {} Detected shell: {}", "•".blue(), shell);

    // Generate completions
    let mut completions = Vec::new();
    let clap_shell = match shell.as_str() {
        "zsh" => Shell::Zsh,
        "bash" => Shell::Bash,
        "fish" => Shell::Fish,
        _ => {
            println!("  {} Unknown shell, skipping completions", "•".yellow());
            return Ok(());
        }
    };
    generate(clap_shell, &mut Cli::command(), "dek", &mut completions);
    let completions_str = String::from_utf8(completions)?;

    // Determine completions path and install
    let home = std::env::var("HOME")?;
    let (comp_path, source_line) = match shell.as_str() {
        "zsh" => {
            let dir = format!("{}/.zsh/completions", home);
            fs::create_dir_all(&dir)?;
            (
                format!("{}/_dek", dir),
                Some(format!("fpath=(~/.zsh/completions $fpath) && autoload -Uz compinit && compinit")),
            )
        }
        "bash" => {
            let dir = format!("{}/.local/share/bash-completion/completions", home);
            fs::create_dir_all(&dir)?;
            (format!("{}/dek", dir), None)
        }
        "fish" => {
            let dir = format!("{}/.config/fish/completions", home);
            fs::create_dir_all(&dir)?;
            (format!("{}/dek.fish", dir), None)
        }
        _ => return Ok(()),
    };

    fs::write(&comp_path, &completions_str)?;
    println!("  {} Wrote completions to {}", "✓".green(), comp_path);

    // Ensure source line in rc if needed (for zsh)
    if let Some(line) = source_line {
        let rc_path = format!("{}/.zshrc", home);
        let rc_content = fs::read_to_string(&rc_path).unwrap_or_default();

        if !rc_content.contains("/.zsh/completions") {
            let mut new_content = rc_content;
            if !new_content.ends_with('\n') && !new_content.is_empty() {
                new_content.push('\n');
            }
            new_content.push_str(&line);
            new_content.push('\n');
            fs::write(&rc_path, &new_content)?;
            println!("  {} Added completions to .zshrc", "✓".green());
        } else {
            println!("  {} Completions already configured in .zshrc", "•".dimmed());
        }
    }

    println!();
    println!("  {} Restart your shell or run: exec {}", "✓".green(), shell);

    Ok(())
}

fn detect_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.contains("zsh") {
            return "zsh".to_string();
        } else if shell.contains("fish") {
            return "fish".to_string();
        } else if shell.contains("bash") {
            return "bash".to_string();
        }
    }
    "bash".to_string()
}
