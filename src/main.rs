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

    /// Remote target (user@host or ssh hostname)
    #[arg(short, long, global = true, value_name = "TARGET")]
    target: Option<String>,

    /// Remote targets from inventory (glob pattern, e.g., 'logger*')
    #[arg(short = 'r', long, global = true, value_name = "PATTERN")]
    remotes: Option<String>,

    /// Suppress banner and extra output
    #[arg(short, long, global = true)]
    quiet: bool,

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
    // If first arg has no dot, treat as: dek run <name> [args...]
    if !cli.inline.is_empty() {
        if !cli.inline[0].contains('.') {
            let mut args = cli.inline;
            let name = args.remove(0);
            return run_command(cli.config, Some(name), args);
        }
        return run_inline(&cli.inline);
    }

    let config = cli.config;
    let target = cli.target;
    let remotes = cli.remotes;
    let quiet = cli.quiet;

    match cli.command {
        Some(Commands::Apply { configs }) => {
            if let Some(pattern) = remotes {
                run_remotes(&pattern, "apply", config, &configs)
            } else if let Some(t) = target {
                run_remote(&t, "apply", config.clone(), &configs)
            } else {
                run_mode(runner::Mode::Apply, config, configs, quiet)
            }
        }
        Some(Commands::Check { configs }) => {
            if let Some(pattern) = remotes {
                run_remotes(&pattern, "check", config, &configs)
            } else if let Some(t) = target {
                run_remote(&t, "check", config.clone(), &configs)
            } else {
                run_mode(runner::Mode::Check, config, configs, quiet)
            }
        }
        Some(Commands::Plan { configs }) => {
            if let Some(pattern) = remotes {
                run_remotes(&pattern, "plan", config, &configs)
            } else if let Some(t) = target {
                run_remote(&t, "plan", config.clone(), &configs)
            } else {
                run_mode(runner::Mode::Plan, config, configs, quiet)
            }
        }
        Some(Commands::List) => {
            if let Some(pattern) = remotes {
                run_remotes(&pattern, "list", config, &[])
            } else if let Some(t) = target {
                run_remote(&t, "list", config.clone(), &[])
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

fn run_mode(mode: runner::Mode, config_path: Option<PathBuf>, configs: Vec<String>, quiet: bool) -> Result<()> {
    let path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&path)?;
    let meta = config::load_meta(&resolved_path);

    let verb = match mode {
        runner::Mode::Apply => "Applying",
        runner::Mode::Check => "Checking",
        runner::Mode::Plan => "Plan for",
    };

    if !quiet {
        if let Some(banner) = meta.as_ref().and_then(|m| m.banner.as_ref()) {
            for line in banner.lines() {
                println!("{}", line.bold());
            }
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
    }

    let config = if configs.is_empty() {
        config::load(&resolved_path)?
    } else {
        config::load_selected(&resolved_path, &configs)?
    };

    let runner = runner::Runner::new(mode);
    runner.run(&config, &resolved_path)
}

/// Pre-built archive and binary info for remote deployment
struct RemotePayload {
    local_tar: String,
    bin_hash: String,
    dek_binary: PathBuf,
}

impl RemotePayload {
    fn prepare(config_path: &std::path::Path) -> Result<Self> {
        let tar_data = util::create_tar_gz(config_path)?;
        let hash = format!("{:x}", md5::compute(&tar_data));
        let local_tar = format!("/tmp/dek-config-{}.tar.gz", &hash[..8]);
        std::fs::write(&local_tar, &tar_data)?;

        let dek_binary = std::env::current_exe()?;
        let bin_data = std::fs::read(&dek_binary)?;
        let bin_hash = format!("{:x}", md5::compute(&bin_data));

        Ok(Self { local_tar, bin_hash, dek_binary })
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.local_tar);
    }
}

fn run_remote(target: &str, cmd: &str, config_path: Option<PathBuf>, configs: &[String]) -> Result<()> {
    use owo_colors::OwoColorize;

    let config_path = resolve_config(config_path)?;
    let config_abs = std::fs::canonicalize(&config_path)?;

    output::print_header(&format!("{} on {}", cmd, target));
    println!();

    let payload = RemotePayload::prepare(&config_abs)?;
    let result = deploy_to_host(target, cmd, configs, &payload)?;
    payload.cleanup();

    for line in result.output.lines() {
        println!("  {}", line);
    }

    if !result.success {
        bail!("Remote command failed on {}", target);
    }

    Ok(())
}

/// Result of deploying to a single host
struct DeployResult {
    host: String,
    output: String,
    success: bool,
}

fn deploy_to_host(target: &str, cmd: &str, configs: &[String], payload: &RemotePayload) -> Result<DeployResult> {
    let remote_dir = "/tmp/dek-remote";
    let remote_bin = format!("{}/dek", remote_dir);
    let remote_config = format!("{}/config.tar.gz", remote_dir);
    let mut log = String::new();

    // Setup remote dir + check if binary already exists with same hash
    let check_cmd = format!(
        "mkdir -p {} && if [ -f {} ]; then md5sum {} | cut -d' ' -f1; fi",
        remote_dir, remote_bin, remote_bin
    );
    let check_output = Command::new("ssh")
        .args([target, &check_cmd])
        .output()?;
    if !check_output.status.success() {
        bail!("Failed to connect to {}", target);
    }

    let remote_hash = String::from_utf8_lossy(&check_output.stdout).trim().to_string();

    // Copy binary only if hash differs
    if remote_hash != payload.bin_hash {
        log.push_str("  binary: updated\n");
        let scp_bin = Command::new("scp")
            .args(["-q", &payload.dek_binary.to_string_lossy(), &format!("{}:{}", target, remote_bin)])
            .status()?;
        if !scp_bin.success() {
            bail!("Failed to copy dek binary to {}", target);
        }
    } else {
        log.push_str("  binary: cached\n");
    }

    // Copy config
    let scp_config = Command::new("scp")
        .args(["-q", &payload.local_tar, &format!("{}:{}", target, remote_config)])
        .status()?;
    if !scp_config.success() {
        bail!("Failed to copy config to {}", target);
    }

    // Run dek on remote
    let config_arg = format!("-C {}", remote_config);
    let configs_arg = configs.join(" ");
    let remote_cmd = format!("{} -q {} {} {}", remote_bin, cmd, config_arg, configs_arg);

    let output = Command::new("ssh")
        .args([target, &remote_cmd])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    log.push_str(&stdout);
    if !stderr.is_empty() {
        log.push_str(&stderr);
    }

    Ok(DeployResult {
        host: target.to_string(),
        output: log,
        success: output.status.success(),
    })
}

fn run_remotes(pattern: &str, cmd: &str, config_path: Option<PathBuf>, configs: &[String]) -> Result<()> {
    use std::io::{self, Write};

    let config_path = resolve_config(config_path.clone())?;
    let config_abs = std::fs::canonicalize(&config_path)?;
    let inventory = config::load_inventory(&config_path)
        .ok_or_else(|| anyhow::anyhow!("No inventory.ini found in config directory"))?;

    if inventory.hosts.is_empty() {
        bail!("No hosts defined in inventory");
    }

    // Match hosts against pattern (simple glob: * matches any chars)
    let regex_pattern = format!("^{}$", pattern.replace("*", ".*"));
    let re = regex::Regex::new(&regex_pattern)
        .map_err(|e| anyhow::anyhow!("Invalid pattern '{}': {}", pattern, e))?;

    let matched: Vec<&String> = inventory.hosts.iter().filter(|h| re.is_match(h)).collect();

    if matched.is_empty() {
        bail!("No hosts match pattern '{}'", pattern);
    }

    // Load config to check for local commands and includes
    let dek_config = config::load(&config_path)?;

    // Find local run commands
    let local_cmds: Vec<(&String, &config::RunConfig)> = dek_config
        .run
        .as_ref()
        .map(|runs| runs.iter().filter(|(_, cfg)| cfg.local).collect())
        .unwrap_or_default();

    // Show plan
    println!("{} {} on {} host(s):", "::".blue(), cmd, matched.len());
    for host in &matched {
        println!("  {}", host);
    }
    if !local_cmds.is_empty() {
        println!();
        println!("{} Local commands to run first:", "::".blue());
        for (name, _) in &local_cmds {
            println!("  {}", name);
        }
    }
    if let Some(ref includes) = dek_config.include {
        if !includes.is_empty() {
            println!();
            println!("{} Files to include:", "::".blue());
            for (src, dst) in includes {
                println!("  {} → {}", src, dst);
            }
        }
    }
    println!();

    print!("Proceed? [y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("Aborted");
        return Ok(());
    }
    println!();

    // Run local commands first
    if !local_cmds.is_empty() {
        println!("{} Running local commands...", "::".blue());
        for (name, run_cfg) in &local_cmds {
            println!("  {} {}", "→".yellow(), name);
            run_local_command(name, run_cfg, &config_abs)?;
        }
        println!();
    }

    // Prepare config with includes
    let prepared_config = prepare_config_with_includes(&config_abs, &dek_config)?;
    let prepared_abs = std::fs::canonicalize(&prepared_config)?;

    // Create archive and compute binary hash once
    println!("{} Preparing payload...", "::".blue());
    let payload = RemotePayload::prepare(&prepared_abs)?;

    // Deploy to all hosts in parallel
    let total = matched.len();
    println!("{} Deploying to {} hosts...\n", "::".blue(), total);
    let start = std::time::Instant::now();

    let (tx, rx) = std::sync::mpsc::channel::<Result<DeployResult>>();

    std::thread::scope(|s| {
        for host in &matched {
            let tx = tx.clone();
            let payload = &payload;
            let configs = configs;
            s.spawn(move || {
                let result = deploy_to_host(host, cmd, configs, payload);
                let _ = tx.send(result);
            });
        }
        drop(tx);

        // Print results as they arrive
        let mut done = 0;
        let mut failed_hosts: Vec<String> = Vec::new();
        for result in rx {
            done += 1;
            match result {
                Ok(r) => {
                    let icon = if r.success { "✓".green().to_string() } else { "✗".red().to_string() };
                    println!("{} {} ({}/{})", icon, r.host.bold(), done, total);
                    for line in r.output.lines() {
                        println!("  {}", line);
                    }
                    if !r.success {
                        failed_hosts.push(r.host);
                    }
                }
                Err(e) => {
                    println!("{} error: {} ({}/{})", "✗".red(), e, done, total);
                    failed_hosts.push("?".into());
                }
            }
        }

        // Summary
        let elapsed = start.elapsed();
        let timing = format!("({:.1}s)", elapsed.as_secs_f64());
        let succeeded = total - failed_hosts.len();
        println!();
        if failed_hosts.is_empty() {
            println!("{} {}/{} hosts completed {}", "✓".green(), succeeded, total, timing.dimmed());
        } else {
            println!("{} {}/{} hosts completed, {} failed {}", "!".yellow(), succeeded, total, failed_hosts.len(), timing.dimmed());
            for h in &failed_hosts {
                println!("  {} {}", "✗".red(), h);
            }
        }

        if !failed_hosts.is_empty() {
            std::process::exit(1);
        }
    });

    payload.cleanup();

    Ok(())
}

fn run_local_command(name: &str, run_cfg: &config::RunConfig, config_path: &std::path::Path) -> Result<()> {
    let base_dir = if config_path.is_dir() {
        config_path
    } else {
        config_path.parent().unwrap_or(std::path::Path::new("."))
    };

    let script = if let Some(ref cmd) = run_cfg.cmd {
        cmd.clone()
    } else if let Some(ref script_path) = run_cfg.script {
        let full_path = base_dir.join(script_path);
        std::fs::read_to_string(&full_path)
            .map_err(|e| anyhow::anyhow!("Failed to read script '{}': {}", full_path.display(), e))?
    } else {
        bail!("Local command '{}' has no cmd or script", name);
    };

    let status = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(base_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        bail!("Local command '{}' failed", name);
    }
    Ok(())
}

fn prepare_config_with_includes(config_path: &std::path::Path, dek_config: &config::Config) -> Result<PathBuf> {
    use std::fs;

    let includes = match &dek_config.include {
        Some(inc) if !inc.is_empty() => inc,
        _ => return Ok(config_path.to_path_buf()), // No includes, use original
    };

    // Determine the base directory (config dir, or parent of config file)
    let base_dir = if config_path.is_dir() {
        config_path
    } else {
        config_path.parent().unwrap()
    };

    // Create temp directory and copy entire config directory
    let temp_dir = tempfile::tempdir()?;
    let temp_path = temp_dir.into_path(); // Keep it around
    copy_dir_recursive(base_dir, &temp_path)?;

    // Resolve and copy includes
    for (src, dst) in includes {
        let src_path = if src.starts_with('/') {
            PathBuf::from(src)
        } else {
            base_dir.join(src)
        };
        let dst_path = temp_path.join(dst);

        // Create parent directories
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| anyhow::anyhow!("Failed to copy '{}' to '{}': {}", src_path.display(), dst_path.display(), e))?;
        }
    }

    Ok(temp_path)
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    use std::fs;

    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
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

    let (main_configs, optional_configs): (Vec<_>, Vec<_>) =
        configs.into_iter().partition(|c| !c.optional);

    output::print_header("Available configs");
    println!();
    for cfg in &main_configs {
        print_config_entry(cfg);
    }

    if !optional_configs.is_empty() {
        println!();
        output::print_header("Optional configs");
        println!();
        for cfg in &optional_configs {
            print_config_entry(cfg);
        }
    }

    // Show inventory info
    if let Some(inv) = config::load_inventory(&path) {
        if !inv.hosts.is_empty() {
            println!("  {} - {}", "inventory".green(), format!("{} hosts", inv.hosts.len()).dimmed());
        }
    }

    Ok(())
}

fn print_config_entry(cfg: &config::ConfigInfo) {
    use config::eval_run_if;

    let skipped = cfg.run_if.as_ref().map(|c| !eval_run_if(c)).unwrap_or(false);

    let label = if cfg.name != cfg.key {
        format!("{} ({})", cfg.key, cfg.name)
    } else {
        cfg.key.clone()
    };

    if skipped {
        let desc = cfg.description.as_deref().unwrap_or("");
        let suffix = if desc.is_empty() {
            "(skipped)".to_string()
        } else {
            format!("{} (skipped)", desc)
        };
        println!("  {} - {}", label.dimmed(), suffix.dimmed());
    } else if let Some(ref d) = cfg.description {
        println!("  {} - {}", label.green(), d.dimmed());
    } else {
        println!("  {}", label.green());
    }
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
        for line in banner.lines() {
            println!("  {}", line.bold());
        }
    } else {
        println!();
        println!("  {}", name.bold());
    }
    let desc = meta.and_then(|m| m.description.as_ref());
    let version = meta.and_then(|m| m.version.as_ref());
    match (desc, version) {
        (Some(d), Some(v)) => println!("  {} {}", d.dimmed(), format!("v{}", v).dimmed()),
        (Some(d), None) => println!("  {}", d.dimmed()),
        (None, Some(v)) => println!("  {}", format!("v{}", v).dimmed()),
        (None, None) => {}
    }
    if let Some(info) = bake::get_bake_info() {
        println!("  {}", "Powered by dek (https://github.com/zcag/dek)".dimmed());
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
    println!("    {} {}", "--remotes <PATTERN>".white(), "Remote targets from inventory (glob)".dimmed());
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
