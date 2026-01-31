mod bake;
mod config;
mod output;
mod providers;
mod runner;
mod util;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand};
use owo_colors::OwoColorize;
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
    command: Option<Commands>,

    /// Config directory path (default: dek.toml or dek/)
    #[arg(short = 'C', long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Remote target (user@host)
    #[arg(short, long, global = true, value_name = "TARGET")]
    target: Option<String>,

    /// Inline install: provider.package (e.g., cargo.bat apt.htop)
    #[arg(value_name = "SPEC", trailing_var_arg = true)]
    inline: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply configuration (all or specific configs)
    Apply {
        /// Configs to apply (e.g., "tools", "config"). Applies all if omitted.
        #[arg(value_name = "CONFIGS")]
        configs: Vec<String>,
    },
    /// Check what would change (dry-run)
    Check {
        /// Configs to check
        #[arg(value_name = "CONFIGS")]
        configs: Vec<String>,
    },
    /// List items from config (no state check)
    Plan {
        /// Configs to plan
        #[arg(value_name = "CONFIGS")]
        configs: Vec<String>,
    },
    /// List available configs
    List,
    /// Run a command from config (no name = list commands)
    Run {
        /// Command name (omit to list available commands)
        name: Option<String>,

        /// Arguments to pass to the command
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Spin up container, apply config, drop into shell
    Test {
        /// Base image (default: archlinux)
        #[arg(short, long, default_value = "archlinux")]
        image: String,

        /// Keep container after exit
        #[arg(short, long)]
        keep: bool,
    },
    /// Bake config into standalone binary
    Bake {
        /// Config file or directory to embed
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,

        /// Output binary path
        #[arg(short, long, default_value = "dek-baked")]
        output: PathBuf,
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

    // Handle inline mode: dek cargo.bat apt.htop
    if !cli.inline.is_empty() {
        return run_inline(&cli.inline);
    }

    let config = cli.config;
    let target = cli.target;

    match cli.command {
        Some(Commands::Apply { configs }) => {
            if let Some(t) = target {
                run_remote(&t, "apply", config, &configs)
            } else {
                run_mode(runner::Mode::Apply, config, configs)
            }
        }
        Some(Commands::Check { configs }) => {
            if let Some(t) = target {
                run_remote(&t, "check", config, &configs)
            } else {
                run_mode(runner::Mode::Check, config, configs)
            }
        }
        Some(Commands::Plan { configs }) => {
            if let Some(t) = target {
                run_remote(&t, "plan", config, &configs)
            } else {
                run_mode(runner::Mode::Plan, config, configs)
            }
        }
        Some(Commands::List) => {
            if let Some(t) = target {
                run_remote(&t, "list", config, &[])
            } else {
                run_list(config)
            }
        }
        Some(Commands::Run { name, args }) => run_command(config, name, args),
        Some(Commands::Test { image, keep }) => run_test(config, image, keep),
        Some(Commands::Bake { config: bake_config, output }) => {
            bake::run(bake_config.or(config), output)
        }
        Some(Commands::Completions { shell }) => {
            generate(shell, &mut Cli::command(), "dek", &mut io::stdout());
            Ok(())
        }
        Some(Commands::Setup) => run_setup(),
        None => {
            // No command - show rich help
            let config_path = config
                .or_else(bake::check_embedded)
                .or_else(config::find_default_config);
            if let Some(path) = config_path {
                let meta = config::load_meta(&path);
                return print_rich_help(meta.as_ref(), &path);
            }
            // No config found - show basic clap help
            Cli::command().print_help()?;
            Ok(())
        }
    }
}

fn resolve_config(config: Option<PathBuf>) -> Result<PathBuf> {
    match config {
        Some(path) => Ok(path),
        None => {
            // Check for embedded config first (baked binary)
            if let Some(path) = bake::check_embedded() {
                return Ok(path);
            }
            config::find_default_config()
                .ok_or_else(|| anyhow::anyhow!("No config found. Create dek.toml or dek/ directory"))
        }
    }
}

fn run_mode(mode: runner::Mode, config_path: Option<PathBuf>, configs: Vec<String>) -> Result<()> {
    let path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&path)?;
    let meta = config::load_meta(&resolved_path);

    let verb = match mode {
        runner::Mode::Apply => "Applying",
        runner::Mode::Check => "Checking",
        runner::Mode::Plan => "Plan for",
    };

    if let Some(banner) = meta.as_ref().and_then(|m| m.banner.as_ref()) {
        println!("{}", banner.bold());
    } else {
        let header = if configs.is_empty() {
            format!("{} {}", verb, path.display())
        } else {
            format!("{} [{}]", verb, configs.join(", "))
        };
        output::print_header(&header);
    }
    if let Some(info) = bake::get_bake_info() {
        println!("{}", info.dimmed());
    }
    println!();

    let config = if configs.is_empty() {
        config::load(&resolved_path)?
    } else {
        config::load_selected(&resolved_path, &configs)?
    };

    let runner = runner::Runner::new(mode);
    runner.run(&config, &resolved_path)
}

fn run_remote(target: &str, cmd: &str, config_path: Option<PathBuf>, configs: &[String]) -> Result<()> {
    use owo_colors::OwoColorize;
    use std::fs;

    let config_path = resolve_config(config_path)?;
    let config_abs = fs::canonicalize(&config_path)?;

    output::print_header(&format!("{} on {}", cmd, target));
    println!();

    // Create tar.gz of config
    println!("  {} Creating config archive...", "→".yellow());
    let tar_data = util::create_tar_gz(&config_abs)?;
    let hash = format!("{:x}", md5::compute(&tar_data));
    let local_tar = format!("/tmp/dek-config-{}.tar.gz", &hash[..8]);
    fs::write(&local_tar, &tar_data)?;

    // Get dek binary path
    let dek_binary = std::env::current_exe()?;
    let remote_dir = "/tmp/dek-remote";
    let remote_bin = format!("{}/dek", remote_dir);
    let remote_config = format!("{}/config.tar.gz", remote_dir);

    // Create remote directory
    println!("  {} Setting up remote...", "→".yellow());
    let mkdir_status = Command::new("ssh")
        .args([target, &format!("mkdir -p {}", remote_dir)])
        .status()?;
    if !mkdir_status.success() {
        bail!("Failed to create remote directory");
    }

    // Copy dek binary
    println!("  {} Copying dek binary...", "→".yellow());
    let scp_bin = Command::new("scp")
        .args(["-q", &dek_binary.to_string_lossy(), &format!("{}:{}", target, remote_bin)])
        .status()?;
    if !scp_bin.success() {
        bail!("Failed to copy dek binary to remote");
    }

    // Copy config
    println!("  {} Copying config...", "→".yellow());
    let scp_config = Command::new("scp")
        .args(["-q", &local_tar, &format!("{}:{}", target, remote_config)])
        .status()?;
    if !scp_config.success() {
        bail!("Failed to copy config to remote");
    }

    // Build remote command
    let config_arg = format!("-C {}", remote_config);
    let configs_arg = configs.join(" ");
    let remote_cmd = format!("{} {} {} {}", remote_bin, cmd, config_arg, configs_arg);

    println!("  {} Running on {}...", "→".yellow(), target);
    println!();

    // Run dek on remote
    let status = Command::new("ssh")
        .args(["-t", target, &remote_cmd])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    // Cleanup local tar
    let _ = fs::remove_file(&local_tar);

    if !status.success() {
        bail!("Remote command failed");
    }

    Ok(())
}

fn run_list(config_path: Option<PathBuf>) -> Result<()> {
    let path = resolve_config(config_path)?;
    let configs = config::list_configs(&path)?;

    if configs.is_empty() {
        println!("No config files found");
        return Ok(());
    }

    output::print_header("Available configs");
    println!();
    for cfg in configs {
        let label = if cfg.name != cfg.key {
            format!("{} ({})", cfg.key, cfg.name)
        } else {
            cfg.key
        };
        if let Some(d) = cfg.description {
            println!("  {} - {}", label.green(), d.dimmed());
        } else {
            println!("  {}", label.green());
        }
    }
    Ok(())
}

fn run_command(config_path: Option<PathBuf>, name: Option<String>, args: Vec<String>) -> Result<()> {
    use std::process::Command;

    let path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&path)?;
    let config = config::load(&resolved_path)?;

    // If no name provided, list available commands
    let name = match name {
        Some(n) => n,
        None => {
            let commands = config.run.as_ref();
            if commands.is_none() || commands.unwrap().is_empty() {
                println!("No run commands defined in config");
                return Ok(());
            }

            output::print_header("Run Commands");
            println!();
            let mut cmds: Vec<_> = commands.unwrap().iter().collect();
            cmds.sort_by_key(|(k, _)| *k);
            for (cmd_name, cmd_config) in cmds {
                if let Some(ref desc) = cmd_config.description {
                    println!("  {} - {}", cmd_name.bold(), desc.dimmed());
                } else {
                    println!("  {}", cmd_name.bold());
                }
            }
            return Ok(());
        }
    };

    let base_dir = if resolved_path.is_file() {
        resolved_path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf()
    } else {
        resolved_path.clone()
    };

    let run_config = config.run.as_ref()
        .and_then(|r| r.get(&name))
        .ok_or_else(|| anyhow::anyhow!("Command '{}' not found in config", name))?;

    // Install dependencies first
    if !run_config.deps.is_empty() {
        output::print_header(&format!("Resolving deps for {}", name));
        println!();

        let items: Result<Vec<_>> = run_config.deps.iter().map(|d| parse_provider_spec(d)).collect();
        let runner = runner::Runner::new(runner::Mode::Apply);
        runner.run_items(&items?)?;
        println!();
    }

    // Apply inline file config if present
    if let Some(ref file_config) = run_config.file {
        let inline_config = config::Config {
            file: Some(file_config.clone()),
            ..Default::default()
        };
        let run = runner::Runner::new(runner::Mode::Apply);
        run.run(&inline_config, &resolved_path)?;
    }

    // Run shell command if present
    if let Some(ref cmd) = run_config.cmd {
        let status = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .arg("_")
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        if !status.success() {
            bail!("Command '{}' exited with status {}", name, status);
        }
    } else if let Some(ref script_path) = run_config.script {
        let full_path = base_dir.join(script_path);
        let script = std::fs::read_to_string(&full_path)
            .map_err(|e| anyhow::anyhow!("Failed to read script '{}': {}", full_path.display(), e))?;

        let status = Command::new("sh")
            .arg("-c")
            .arg(&script)
            .arg("_")
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        if !status.success() {
            bail!("Command '{}' exited with status {}", name, status);
        }
    } else if run_config.file.is_none() {
        bail!("Command '{}' has no action defined (needs cmd, script, or file)", name);
    }

    Ok(())
}

/// Parse "provider.package" spec into StateItem
fn parse_provider_spec(spec: &str) -> Result<providers::StateItem> {
    let (provider, package) = spec
        .split_once('.')
        .ok_or_else(|| anyhow::anyhow!("Invalid spec '{}'. Use provider.package (e.g., cargo.bat)", spec))?;

    let kind = match provider {
        "os" => "package.os",
        "apt" => "package.apt",
        "pacman" => "package.pacman",
        "cargo" => "package.cargo",
        "go" => "package.go",
        "npm" => "package.npm",
        "pip" => "package.pip",
        "webi" => "package.webi",
        _ => bail!("Unknown provider '{}'. Use: os, apt, pacman, cargo, go, npm, pip, webi", provider),
    };

    Ok(providers::StateItem::new(kind, package))
}

fn run_inline(specs: &[String]) -> Result<()> {
    output::print_header("Installing");
    println!();

    let items: Result<Vec<_>> = specs.iter().map(|s| parse_provider_spec(s)).collect();
    let runner = runner::Runner::new(runner::Mode::Apply);
    runner.run_items(&items?)
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

    // Get dek binary - use current exe if baked, otherwise build from source
    let dek_binary = if cwd.join("Cargo.toml").exists() {
        println!("  {} Building dek...", "→".yellow());
        let build_status = Command::new("cargo")
            .args(["build", "--release", "--quiet"])
            .status()?;
        if !build_status.success() {
            bail!("cargo build failed");
        }
        let binary = cwd.join("target/release/dek");
        if !binary.exists() {
            bail!("dek binary not found at {}", binary.display());
        }
        binary
    } else {
        std::env::current_exe()?
    };

    // Build docker args
    let container_name = format!("dek-test-{}", std::process::id());
    let mut args = vec!["run".to_string()];

    // Only use -it if we have a TTY
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        args.push("-it".to_string());
    }

    args.push("--name".to_string());
    args.push(container_name.clone());

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

    // Run shell with dek apply (always drop into shell even if apply fails)
    args.push("sh".to_string());
    args.push("-c".to_string());
    args.push(format!(
        r#"dek apply -C {}; echo ""; echo "Dropping into shell..."; exec bash -l"#,
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

    let shell = util::Shell::detect();
    println!("  {} Detected shell: {}", "•".blue(), shell.name());

    // Generate completions
    let mut completions = Vec::new();
    let clap_shell = match shell {
        util::Shell::Zsh => Shell::Zsh,
        util::Shell::Bash => Shell::Bash,
        util::Shell::Fish => Shell::Fish,
    };
    generate(clap_shell, &mut Cli::command(), "dek", &mut completions);
    let completions_str = String::from_utf8(completions)?;

    // Determine completions path and install
    let home = std::env::var("HOME")?;
    let (comp_path, source_line) = match shell {
        util::Shell::Zsh => {
            let dir = format!("{}/.zsh/completions", home);
            fs::create_dir_all(&dir)?;
            (
                format!("{}/_dek", dir),
                Some("fpath=(~/.zsh/completions $fpath) && autoload -Uz compinit && compinit"),
            )
        }
        util::Shell::Bash => {
            let dir = format!("{}/.local/share/bash-completion/completions", home);
            fs::create_dir_all(&dir)?;
            (format!("{}/dek", dir), None)
        }
        util::Shell::Fish => {
            let dir = format!("{}/.config/fish/completions", home);
            fs::create_dir_all(&dir)?;
            (format!("{}/dek.fish", dir), None)
        }
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
            new_content.push_str(line);
            new_content.push('\n');
            fs::write(&rc_path, &new_content)?;
            println!("  {} Added completions to .zshrc", "✓".green());
        } else {
            println!("  {} Completions already configured in .zshrc", "•".dimmed());
        }
    }

    println!();
    println!("  {} Restart your shell or run: exec {}", "✓".green(), shell.name());

    Ok(())
}

fn print_rich_help(meta: Option<&config::Meta>, config_path: &PathBuf) -> Result<()> {
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "dek".to_string());

    let name = meta.and_then(|m| m.name.as_deref()).unwrap_or(&exe_name);
    let cfg = config::load(config_path)?;
    let configs = config::list_configs(config_path)?;

    // Banner or header
    if let Some(banner) = meta.and_then(|m| m.banner.as_ref()) {
        println!();
        println!("  {}", banner.bold());
    } else {
        println!();
        println!("  {}", name.bold());
    }
    if let Some(desc) = meta.and_then(|m| m.description.as_ref()) {
        println!("  {}", desc.dimmed());
    }
    if let Some(version) = meta.and_then(|m| m.version.as_ref()) {
        println!("  {}", format!("v{}", version).dimmed());
    }
    if let Some(info) = bake::get_bake_info() {
        println!("  {}", info.dimmed());
    }
    println!();

    // Usage
    println!("  {}", "USAGE".dimmed());
    println!("    {} {} {}", exe_name.cyan(), "[OPTIONS]".dimmed(), "<COMMAND>".white());
    println!();

    // Commands
    println!("  {}", "COMMANDS".dimmed());
    println!("    {}      {}", "apply".white(), "Apply configuration".dimmed());
    println!("    {}      {}", "check".white(), "Check what would change (dry-run)".dimmed());
    println!("    {}       {}", "plan".white(), "List items from config".dimmed());
    println!("    {}       {}", "list".white(), "List available configs".dimmed());
    println!("    {}        {}", "run".white(), "Run a command from config".dimmed());
    println!("    {}       {}", "test".white(), "Test in container".dimmed());
    println!("    {}       {}", "bake".white(), "Bake into standalone binary".dimmed());
    println!();

    // Options
    println!("  {}", "OPTIONS".dimmed());
    println!("    {}  {}", "-C, --config <PATH>".white(), "Config path".dimmed());
    println!("    {}  {}", "-t, --target <HOST>".white(), "Remote target (user@host)".dimmed());
    println!("    {}              {}", "-h, --help".white(), "Print help".dimmed());
    println!("    {}           {}", "-V, --version".white(), "Print version".dimmed());
    println!();

    // Available configs
    if !configs.is_empty() {
        println!("  {}", "CONFIGS".dimmed());
        for cfg_info in &configs {
            let label = if cfg_info.name != cfg_info.key {
                format!("{} ({})", cfg_info.key, cfg_info.name)
            } else {
                cfg_info.key.clone()
            };
            if let Some(d) = &cfg_info.description {
                println!("    {}  {}", label.green(), d.dimmed());
            } else {
                println!("    {}", label.green());
            }
        }
        println!();
    }

    // Run commands
    if let Some(run) = &cfg.run {
        if !run.is_empty() {
            println!("  {}", "RUN".dimmed());
            let mut cmds: Vec<_> = run.iter().collect();
            cmds.sort_by_key(|(k, _)| *k);
            for (cmd_name, rc) in cmds {
                if let Some(d) = &rc.description {
                    println!("    {}  {}", cmd_name.yellow(), d.dimmed());
                } else {
                    println!("    {}", cmd_name.yellow());
                }
            }
            println!();
        }
    }

    Ok(())
}
