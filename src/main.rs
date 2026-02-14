/// Conditional color: uses if_supports_color to respect NO_COLOR / --no-color
#[macro_export]
macro_rules! c {
    ($text:expr, $method:ident) => {
        $text.if_supports_color(owo_colors::Stream::Stdout, |t| t.$method())
    };
}

mod bake;
mod cache;
mod config;
mod output;
mod providers;
mod runner;
mod util;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use owo_colors::OwoColorize;
use clap_complete::{generate, Shell};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ColorMode {
    Auto,
    Always,
    Never,
}
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

    /// Config is already prepared (skip prepare_config). Used by remote deploy.
    #[arg(long, hide = true, global = true)]
    prepared: bool,

    /// Color output: auto (default), always, never
    #[arg(long, global = true, default_value = "auto")]
    color: ColorMode,

    /// Inline install: provider.package (e.g., cargo.bat apt.htop)
    #[arg(value_name = "SPEC", trailing_var_arg = true)]
    inline: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply configuration (all or specific configs)
    #[command(alias = "a")]
    Apply {
        /// Configs to apply (e.g., "tools", "config"). Applies all if omitted.
        #[arg(value_name = "CONFIGS")]
        configs: Vec<String>,
    },
    /// Check what would change (dry-run)
    #[command(alias = "c")]
    Check {
        /// Configs to check
        #[arg(value_name = "CONFIGS")]
        configs: Vec<String>,
    },
    /// List items from config (no state check)
    #[command(alias = "p")]
    Plan {
        /// Configs to plan
        #[arg(value_name = "CONFIGS")]
        configs: Vec<String>,
    },
    /// Run a command from config (no name = list commands)
    #[command(alias = "r")]
    Run {
        /// Command name (omit to list available commands)
        name: Option<String>,

        /// Arguments to pass to the command
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Spin up container, apply config, drop into shell
    #[command(alias = "t")]
    Test {
        /// Base image (default: meta.toml [test].image or "archlinux")
        #[arg(short, long)]
        image: Option<String>,

        /// Remove container after exit (default: keep)
        #[arg(short = 'r', long)]
        rm: bool,

        /// Force new container (remove existing, rebake)
        #[arg(short, long)]
        fresh: bool,

        /// Attach to existing container
        #[arg(short, long)]
        attach: bool,

        /// Configs/selectors to apply (e.g., "tools", "@core")
        #[arg(value_name = "SELECTORS")]
        selectors: Vec<String>,
    },
    /// Run a command in the test container
    #[command(alias = "dx")]
    Exec {
        /// Command and arguments to run
        #[arg(trailing_var_arg = true, required = true)]
        cmd: Vec<String>,
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
    /// Query system state probes
    #[command(alias = "s")]
    State {
        /// Probe name (omit to list all)
        name: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Extra args: "is <val>" or "isnot <val>"
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
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

    match cli.color {
        ColorMode::Always => {
            owo_colors::set_override(true);
            std::env::remove_var("NO_COLOR");
        }
        ColorMode::Never => {
            owo_colors::set_override(false);
            std::env::set_var("NO_COLOR", "1");
        }
        ColorMode::Auto => {
            if std::env::var_os("NO_COLOR").is_some() {
                owo_colors::set_override(false);
            }
        }
    }

    // Ensure well-known user binary dirs are in PATH (non-interactive SSH won't have them)
    ensure_user_path();

    // Handle inline mode: dek cargo.bat apt.htop
    // If first arg has no dot, treat as: dek run <name> [args...]
    if !cli.inline.is_empty() {
        // Dynamic completion for shell scripts
        if cli.inline[0] == "_complete" {
            let what = cli.inline.get(1).map(|s| s.as_str()).unwrap_or("");
            return run_complete(cli.config, what);
        }
        if !cli.inline[0].contains('.') {
            let mut args = cli.inline;
            let name = args.remove(0);
            if cli.remotes.is_some() || cli.target.is_some() {
                return run_command_remote(cli.config, Some(name), args, cli.target, cli.remotes);
            }
            return run_command(cli.config, Some(name), args);
        }
        return run_inline(&cli.inline);
    }

    let config = cli.config;
    let target = cli.target;
    let remotes = cli.remotes;
    let quiet = cli.quiet;
    let prepared = cli.prepared;

    match cli.command {
        Some(Commands::Apply { configs }) => {
            if let Some(pattern) = remotes {
                run_remotes(&pattern, "apply", config, &configs)
            } else if let Some(t) = target {
                run_remote(&t, "apply", config.clone(), &configs)
            } else {
                run_mode(runner::Mode::Apply, config, configs, quiet, prepared)
            }
        }
        Some(Commands::Check { configs }) => {
            if let Some(pattern) = remotes {
                run_remotes(&pattern, "check", config, &configs)
            } else if let Some(t) = target {
                run_remote(&t, "check", config.clone(), &configs)
            } else {
                run_mode(runner::Mode::Check, config, configs, quiet, prepared)
            }
        }
        Some(Commands::Plan { configs }) => {
            if let Some(pattern) = remotes {
                run_remotes(&pattern, "plan", config, &configs)
            } else if let Some(t) = target {
                run_remote(&t, "plan", config.clone(), &configs)
            } else {
                run_mode(runner::Mode::Plan, config, configs, quiet, prepared)
            }
        }
        Some(Commands::Run { name, args }) => {
            if remotes.is_some() || target.is_some() {
                run_command_remote(config, name, args, target, remotes)
            } else {
                run_command(config, name, args)
            }
        }
        Some(Commands::Test { image, rm, fresh, attach, selectors }) => run_test(config, image, rm, fresh, attach, selectors),
        Some(Commands::Exec { cmd }) => run_exec(config, cmd),
        Some(Commands::State { name, json, args }) => run_state(config, name, json, args),
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

/// Ensure well-known user binary directories are in PATH.
/// Non-interactive SSH doesn't source .bashrc/.profile, so paths like
/// ~/.cargo/bin, ~/.local/bin, ~/go/bin etc. are missing.
fn ensure_user_path() {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };
    let extra = [
        format!("{}/.cargo/bin", home),
        format!("{}/.local/bin", home),
        format!("{}/.local/opt/go/bin", home),
        format!("{}/go/bin", home),
        format!("{}/.npm-global/bin", home),
    ];
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<&str> = current.split(':').collect();
    for dir in &extra {
        if !parts.contains(&dir.as_str()) && std::path::Path::new(dir).is_dir() {
            parts.push(dir);
        }
    }
    std::env::set_var("PATH", parts.join(":"));
}

/// Compare semver strings (e.g. "0.1.28" > "0.1.27")
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.').filter_map(|p| p.parse().ok()).collect()
    };
    parse(a).cmp(&parse(b))
}

/// Check min_version from meta.toml. If current dek is outdated, update and exit.
fn check_min_version(meta: Option<&config::Meta>) -> Result<()> {
    let min = match meta.and_then(|m| m.min_version.as_deref()) {
        Some(v) => v,
        None => return Ok(()),
    };
    let current = env!("CARGO_PKG_VERSION");
    if version_cmp(current, min) != std::cmp::Ordering::Less {
        return Ok(());
    }

    println!("  {} dek {} required (current: {}), updating...",
        c!("→", yellow), min, current);

    // Try cargo-binstall first (fast, pre-compiled), fall back to cargo install
    let ok = if util::command_exists("cargo-binstall") {
        std::process::Command::new("cargo-binstall")
            .args(["dek", "--no-confirm"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else if util::command_exists("cargo") {
        std::process::Command::new("cargo")
            .args(["install", "dek"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        false
    };

    if ok {
        println!();
        println!("  {} dek updated. Please rerun your command.", c!("✓", green));
        std::process::exit(0);
    } else {
        bail!("Failed to update dek to {}", min);
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

fn run_mode(mode: runner::Mode, config_path: Option<PathBuf>, configs: Vec<String>, quiet: bool, prepared: bool) -> Result<()> {
    let path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&path)?;
    let meta = config::load_meta(&resolved_path);
    check_min_version(meta.as_ref())?;

    let verb = match mode {
        runner::Mode::Apply => "Applying",
        runner::Mode::Check => "Checking",
        runner::Mode::Plan => "Plan for",
    };

    if !quiet {
        if let Some(banner) = meta.as_ref().and_then(|m| m.banner.as_ref()) {
            for line in banner.lines() {
                println!("{}", c!(line, bold));
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
            println!("{}", c!(info, dimmed));
        }
        println!();
    }

    // Apply runtime vars from meta.toml before anything else.
    // Use effective selectors (explicit or defaults) for scoped vars.
    if let Some(ref vars) = meta.as_ref().and_then(|m| m.vars.as_ref()) {
        let defaults = meta.as_ref().map(|m| &m.defaults[..]).unwrap_or(&[]);
        let effective: &[String] = if configs.is_empty() { defaults } else { &configs };
        config::apply_vars(vars, effective);
    }

    let config = config::load_for_apply(&resolved_path, &configs, meta.as_ref())?;

    // Resolve artifacts (build outputs) before running.
    // Skip when --prepared (rsync remote deploy) or tarball (bake).
    let working_path = if prepared || util::is_tar_gz(&path) {
        resolved_path.clone()
    } else {
        prepare_config(&resolved_path, &config)?
    };

    let runner = runner::Runner::new(mode);
    runner.run(&config, &working_path)
}

/// Pre-built config dir and binary info for remote deployment
struct RemotePayload {
    prepared_dir: PathBuf,
    bin_hash: String,
    dek_binary: PathBuf,
}

impl RemotePayload {
    fn prepare(prepared_dir: &std::path::Path) -> Result<Self> {
        let dek_binary = std::env::current_exe()?;
        let bin_data = std::fs::read(&dek_binary)?;
        let bin_hash = format!("{:x}", md5::compute(&bin_data));

        Ok(Self { prepared_dir: prepared_dir.to_path_buf(), bin_hash, dek_binary })
    }
}

fn run_remote(target: &str, cmd: &str, config_path: Option<PathBuf>, configs: &[String]) -> Result<()> {
    let config_path = resolve_config(config_path)?;
    let config_abs = std::fs::canonicalize(&config_path)?;
    let meta = config::load_meta(&config_path);
    let remote_install = meta.as_ref().map(|m| m.remote_install).unwrap_or(false);

    output::print_header(&format!("{} on {}", cmd, target));
    println!();

    // Prepare config (artifacts + includes)
    let dek_config = config::load(&config_path)?;
    let prepared_config = prepare_config(&config_abs, &dek_config)?;
    let prepared_abs = std::fs::canonicalize(&prepared_config)?;

    let payload = RemotePayload::prepare(&prepared_abs)?;

    // Show payload sizes
    let config_size = dir_size(&payload.prepared_dir);
    let bin_size = std::fs::metadata(&payload.dek_binary).map(|m| m.len()).unwrap_or(0);
    println!("  {} payload — config: {}, binary: {}",
        c!("→", yellow),
        output::format_bytes(config_size),
        output::format_bytes(bin_size),
    );
    println!();

    let result = deploy_to_host(target, cmd, configs, &payload, None, remote_install)?;

    // Print full remote output for single-host
    for line in result.output.lines() {
        println!("  {}", line);
    }

    let timing = format!("({})", output::format_duration(result.duration));
    println!();
    if result.success {
        println!("{} done {}", c!("✓", green), c!(timing, dimmed));
    } else {
        println!("{} failed {}", c!("✗", red), c!(timing, dimmed));
        bail!("Remote command failed on {}", target);
    }

    Ok(())
}

/// Result of deploying to a single host
struct DeployResult {
    host: String,
    output: String,
    success: bool,
    duration: std::time::Duration,
}

fn deploy_to_host(
    target: &str, cmd: &str, configs: &[String], payload: &RemotePayload,
    pb: Option<&indicatif::ProgressBar>, remote_install: bool,
) -> Result<DeployResult> {
    let start = std::time::Instant::now();
    let remote_dir = "/tmp/dek-remote";
    let remote_bin = format!("{}/dek", remote_dir);
    let remote_config = format!("{}/config/", remote_dir);
    let mut log = String::new();

    let update = |msg: &str| {
        if let Some(pb) = pb {
            pb.set_message(msg.to_string());
        } else {
            println!("  {} {}", c!("→", yellow), msg);
        }
    };

    // Setup remote dir + check if binary already exists with same hash
    update("connecting...");
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
        update("uploading binary...");
        let scp_bin = Command::new("scp")
            .args(["-q", &payload.dek_binary.to_string_lossy(), &format!("{}:{}", target, remote_bin)])
            .status()?;
        if !scp_bin.success() {
            bail!("Failed to copy dek binary to {}", target);
        }
    } else {
        update("binary cached");
    }

    // Rsync config
    update("syncing config...");
    let local_src = format!("{}/", payload.prepared_dir.display());
    let remote_dest = format!("{}:{}", target, remote_config);
    let rsync = Command::new("rsync")
        .args(["-az", "--delete", &local_src, &remote_dest])
        .output()?;
    if !rsync.status.success() {
        let err = String::from_utf8_lossy(&rsync.stderr);
        bail!("Failed to rsync config to {}: {}", target, err.trim());
    }

    // Symlink config + binary so `dek` works standalone on remote
    if remote_install {
        let link_cmd = format!(
            "mkdir -p ~/.config ~/.local/bin && ln -sfn {} ~/.config/dek && ln -sf {} ~/.local/bin/dek",
            remote_config.trim_end_matches('/'), remote_bin
        );
        let _ = Command::new("ssh").args([target, &link_cmd]).output();
    }

    // Run dek on remote
    update(&format!("running {}...", cmd));
    let configs_arg = configs.join(" ");
    let remote_cmd = format!("{} -q --prepared {} -C {} {}", remote_bin, cmd, remote_config, configs_arg);

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
        duration: start.elapsed(),
    })
}

fn run_remotes(pattern: &str, cmd: &str, config_path: Option<PathBuf>, configs: &[String]) -> Result<()> {
    use std::io::{self, Write};

    let config_path = resolve_config(config_path.clone())?;
    let config_abs = std::fs::canonicalize(&config_path)?;
    let meta = config::load_meta(&config_path);
    let remote_install = meta.as_ref().map(|m| m.remote_install).unwrap_or(false);
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
    let host_list: Vec<&str> = matched.iter().map(|h| h.as_str()).collect();
    println!("{} {} on {} host(s): {}", c!("::", blue), cmd, matched.len(), host_list.join(", "));
    if !local_cmds.is_empty() {
        println!();
        println!("{} Local commands to run first:", c!("::", blue));
        for (name, _) in &local_cmds {
            println!("  {}", name);
        }
    }
    if !dek_config.artifact.is_empty() {
        println!();
        println!("{} Artifacts to build:", c!("::", blue));
        for a in &dek_config.artifact {
            println!("  {} → {}", a.name.as_deref().unwrap_or(&a.src), a.dest);
        }
    }
    if let Some(ref includes) = dek_config.include {
        if !includes.is_empty() {
            println!();
            println!("{} Files to include:", c!("::", blue));
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
        println!("{} Running local commands...", c!("::", blue));
        for (name, run_cfg) in &local_cmds {
            println!("  {} {}", c!("→", yellow), name);
            run_local_command(name, run_cfg, &config_abs)?;
        }
        println!();
    }

    // Prepare config (artifacts + includes)
    let prepared_config = prepare_config(&config_abs, &dek_config)?;
    let prepared_abs = std::fs::canonicalize(&prepared_config)?;

    // Compute binary hash once
    let payload = RemotePayload::prepare(&prepared_abs)?;

    // Show payload sizes
    let config_size = dir_size(&payload.prepared_dir);
    let bin_size = std::fs::metadata(&payload.dek_binary).map(|m| m.len()).unwrap_or(0);
    println!("{} Payload ready — config: {}, binary: {}",
        c!("::", blue),
        output::format_bytes(config_size),
        output::format_bytes(bin_size),
    );

    // Deploy to all hosts in parallel
    let total = matched.len();
    println!("{} Deploying to {} hosts...\n", c!("::", blue), total);
    let start = std::time::Instant::now();

    let mp = indicatif::MultiProgress::new();
    let spinners: Vec<_> = matched.iter()
        .map(|host| output::start_deploy_spinner(&mp, host))
        .collect();

    let (tx, rx) = std::sync::mpsc::channel::<(usize, Result<DeployResult>)>();

    std::thread::scope(|s| {
        for (i, host) in matched.iter().enumerate() {
            let tx = tx.clone();
            let payload = &payload;
            let configs = configs;
            let pb = &spinners[i];
            s.spawn(move || {
                let result = deploy_to_host(host, cmd, configs, payload, Some(pb), remote_install);
                let _ = tx.send((i, result));
            });
        }
        drop(tx);

        let mut failed_hosts: Vec<String> = Vec::new();
        for (i, result) in rx {
            let pb = &spinners[i];
            match result {
                Ok(r) => {
                    let summary = output::extract_summary_line(&r.output)
                        .unwrap_or_default();
                    if r.success {
                        output::finish_deploy_ok(pb, &r.host, &summary, r.duration);
                    } else {
                        let err = output::extract_summary_line(&r.output)
                            .unwrap_or_else(|| "failed".to_string());
                        output::finish_deploy_fail(pb, &r.host, &err, r.duration);
                        failed_hosts.push(r.host);
                    }
                }
                Err(e) => {
                    output::finish_deploy_fail(pb, matched[i], &e.to_string(), start.elapsed());
                    failed_hosts.push(matched[i].clone());
                }
            }
        }

        // Summary
        let elapsed = start.elapsed();
        let timing = format!("({})", output::format_duration(elapsed));
        let succeeded = total - failed_hosts.len();
        println!();
        if failed_hosts.is_empty() {
            println!("{} {}/{} hosts completed {}", c!("✓", green), succeeded, total, c!(timing, dimmed));
        } else {
            println!("{} {}/{} hosts completed, {} failed {}", c!("!", yellow), succeeded, total, failed_hosts.len(), c!(timing, dimmed));
            for h in &failed_hosts {
                println!("  {} {}", c!("✗", red), h);
            }
        }

        if !failed_hosts.is_empty() {
            std::process::exit(1);
        }
    });

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

/// Prepare config: resolve artifacts + includes into a temp copy.
/// Returns original path if nothing to prepare.
pub(crate) fn prepare_config(config_path: &std::path::Path, dek_config: &config::Config) -> Result<PathBuf> {
    use std::fs;

    let has_artifacts = !dek_config.artifact.is_empty();
    let has_includes = dek_config.include.as_ref().map(|i| !i.is_empty()).unwrap_or(false);

    if !has_artifacts && !has_includes {
        return Ok(config_path.to_path_buf());
    }

    let base_dir = if config_path.is_dir() {
        config_path
    } else {
        config_path.parent().unwrap()
    };

    // Create temp copy of config
    let temp_dir = tempfile::tempdir()?;
    let temp_path = temp_dir.into_path();
    copy_dir_recursive(base_dir, &temp_path)?;

    // Resolve artifacts
    if has_artifacts {
        println!("{} Resolving artifacts...", c!("::", blue));
        for artifact in &dek_config.artifact {
            let label = artifact.name.as_deref().unwrap_or(&artifact.dest);

            // Skip if dest already exists in config (pre-resolved, e.g. shipped via remote deploy)
            let dest_in_config = base_dir.join(&artifact.dest);
            if dest_in_config.exists() {
                let dst_path = temp_path.join(&artifact.dest);
                if let Some(parent) = dst_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&dest_in_config, &dst_path)?;
                println!("  {} {} {}", c!("•", dimmed), c!(label, dimmed), c!("(pre-resolved)", dimmed));
                continue;
            }

            let src_path = if artifact.src.starts_with('/') {
                PathBuf::from(&artifact.src)
            } else {
                base_dir.join(&artifact.src)
            };

            // Determine if build is needed
            let should_build = if !artifact.watch.is_empty() {
                // watch mode: hash watched paths, compare with cache
                !artifact_watch_fresh(base_dir, artifact, &src_path)
            } else if let Some(ref cmd) = artifact.check {
                // check mode: run shell command
                !Command::new("sh")
                    .arg("-c").arg(cmd).current_dir(base_dir)
                    .stdout(Stdio::null()).stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            } else {
                true // no check, no watch → always build
            };

            if should_build {
                resolve_artifact_deps(&artifact.deps)?;
                let pb = output::start_artifact_spinner(label);
                let result = util::run_cmd_live_dir("sh", &["-c", &artifact.build], &pb, base_dir)?;
                if !result.status.success() {
                    output::finish_artifact_fail(&pb, label, "build failed");
                    bail!("Artifact build failed: {}", label);
                }
                output::finish_artifact_ok(&pb, label);
                // Update watch cache after successful build
                if !artifact.watch.is_empty() {
                    artifact_watch_save(base_dir, artifact);
                }
            } else {
                println!("  {} {} {}", c!("•", dimmed), c!(label, dimmed), c!("(fresh)", dimmed));
            }

            if !src_path.exists() {
                bail!("Artifact not found after build: {} (expected at {})", label, src_path.display());
            }

            // Copy to dest in temp
            let dst_path = temp_path.join(&artifact.dest);
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src_path, &dst_path)?;
        }
    }

    // Resolve includes
    if let Some(ref includes) = dek_config.include {
        for (src, dst) in includes {
            let src_path = if src.starts_with('/') {
                PathBuf::from(src)
            } else {
                base_dir.join(src)
            };
            let dst_path = temp_path.join(dst);

            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }

            if src_path.is_dir() {
                copy_dir_recursive(&src_path, &dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path)
                    .map_err(|e| anyhow::anyhow!("Failed to include '{}': {}", src_path.display(), e))?;
            }
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

/// Resolve local dependencies for artifact builds.
/// Specs: "pkg:bin" (auto-detect PM), "apt.pkg:bin", "pacman.pkg:bin", etc.
fn resolve_artifact_deps(deps: &[String]) -> Result<()> {
    use owo_colors::OwoColorize;
    for dep in deps {
        let (provider, spec) = if let Some((p, s)) = dep.split_once('.') {
            (Some(p), s)
        } else {
            (None, dep.as_str())
        };
        let (pkg, bin) = util::parse_spec(spec);
        if util::command_exists(&bin) {
            continue;
        }
        println!("    {} installing {} (for {})...", c!("→", yellow), pkg, bin);
        match provider {
            Some("apt") => util::SysPkgManager::Apt.install(&pkg)?,
            Some("pacman") => util::SysPkgManager::Pacman.install(&pkg)?,
            Some("brew") => util::SysPkgManager::Brew.install(&pkg)?,
            None | Some("os") => {
                let pm = util::SysPkgManager::detect()
                    .ok_or_else(|| anyhow::anyhow!("No package manager found to install '{}'", dep))?;
                pm.install(&pkg)?;
            }
            Some(p) => anyhow::bail!("Unknown package manager '{}' in dep '{}'", p, dep),
        }
        if !util::command_exists(&bin) {
            anyhow::bail!("Installed '{}' but '{}' not found in PATH", pkg, bin);
        }
    }
    Ok(())
}

/// Compute a hash of all files under the watch paths (path + size + mtime).
fn artifact_watch_hash(base_dir: &std::path::Path, artifact: &config::ArtifactConfig) -> String {
    let mut entries: Vec<(String, u64, u64)> = Vec::new();

    for watch in &artifact.watch {
        let path = if watch.starts_with('/') {
            PathBuf::from(watch)
        } else {
            base_dir.join(watch)
        };
        collect_file_meta(&path, &path, &mut entries);
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut buf = String::new();
    for (path, size, mtime) in &entries {
        buf.push_str(&format!("{}\0{}\0{}\n", path, size, mtime));
    }
    format!("{:x}", md5::compute(buf.as_bytes()))
}

fn collect_file_meta(path: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, u64, u64)>) {
    use std::fs;

    if path.is_file() {
        if let Ok(meta) = fs::metadata(path) {
            let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().to_string();
            let mtime = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push((rel, meta.len(), mtime));
        }
    } else if path.is_dir() {
        if let Ok(rd) = fs::read_dir(path) {
            for entry in rd.flatten() {
                collect_file_meta(&entry.path(), root, out);
            }
        }
    }
}

fn artifact_cache_path(base_dir: &std::path::Path, artifact: &config::ArtifactConfig) -> PathBuf {
    let key = format!("{}\0{}", base_dir.display(), artifact.dest);
    let hash = format!("{:x}", md5::compute(key.as_bytes()));
    PathBuf::from(format!("/tmp/dek-watch-{}.hash", &hash[..16]))
}

/// Check if watched files are unchanged since last build.
fn artifact_watch_fresh(base_dir: &std::path::Path, artifact: &config::ArtifactConfig, src_path: &std::path::Path) -> bool {
    if !src_path.exists() {
        return false; // artifact doesn't exist, must build
    }
    let cache = artifact_cache_path(base_dir, artifact);
    let cached = std::fs::read_to_string(&cache).unwrap_or_default();
    let current = artifact_watch_hash(base_dir, artifact);
    cached.trim() == current
}

/// Save current watch hash to cache.
fn artifact_watch_save(base_dir: &std::path::Path, artifact: &config::ArtifactConfig) {
    let cache = artifact_cache_path(base_dir, artifact);
    let hash = artifact_watch_hash(base_dir, artifact);
    let _ = std::fs::write(&cache, &hash);
}

/// Recursively sum file sizes in a directory.
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = p.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn collect_var_exports(meta: Option<&config::Meta>) -> String {
    let vars = match meta.and_then(|m| m.vars.as_ref()).and_then(|v| v.as_table()) {
        Some(t) => t,
        None => return String::new(),
    };
    let mut exports = Vec::new();
    for (k, v) in vars {
        if let Some(s) = v.as_str() {
            exports.push(format!("export {}={}", k, shell_escape(s)));
        }
    }
    if exports.is_empty() { String::new() } else { format!("{}; ", exports.join("; ")) }
}

fn run_command_remote(
    config_path: Option<PathBuf>, name: Option<String>, args: Vec<String>,
    target: Option<String>, remotes: Option<String>,
) -> Result<()> {
    use std::io::{self, Write};

    let path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&path)?;

    // Apply runtime vars from meta.toml
    let meta = config::load_meta(&resolved_path);
    check_min_version(meta.as_ref())?;
    if let Some(ref vars) = meta.as_ref().and_then(|m| m.vars.as_ref()) {
        config::apply_vars(vars, &[]);
    }

    let cfg = config::load_all(&resolved_path)?;

    // If no name, list available commands
    let name = match name {
        Some(n) => n,
        None => {
            let commands = cfg.run.as_ref();
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
                    println!("  {} - {}", c!(cmd_name, bold), c!(desc, dimmed));
                } else {
                    println!("  {}", c!(cmd_name, bold));
                }
            }
            return Ok(());
        }
    };

    let run_config = cfg.run.as_ref()
        .and_then(|r| r.get(&name))
        .ok_or_else(|| anyhow::anyhow!("Command '{}' not found in config", name))?;

    // Resolve the shell command
    let base_dir = if resolved_path.is_file() {
        resolved_path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf()
    } else {
        resolved_path.clone()
    };

    let shell_cmd = if let Some(ref cmd) = run_config.cmd {
        cmd.clone()
    } else if let Some(ref script_path) = run_config.script {
        let full_path = base_dir.join(script_path);
        std::fs::read_to_string(&full_path)
            .map_err(|e| anyhow::anyhow!("Failed to read script '{}': {}", full_path.display(), e))?
    } else {
        bail!("Command '{}' has no cmd or script for remote execution", name);
    };

    // Append extra args
    let export_prefix = collect_var_exports(meta.as_ref());
    let full_cmd = if args.is_empty() {
        format!("{}{}", export_prefix, shell_cmd)
    } else {
        format!("{}{} {}", export_prefix, shell_cmd, args.join(" "))
    };

    // Resolve hosts
    let hosts: Vec<String> = if let Some(ref t) = target {
        vec![t.clone()]
    } else if let Some(ref pattern) = remotes {
        let inventory = config::load_inventory(&path)
            .ok_or_else(|| anyhow::anyhow!("No inventory.ini found in config directory"))?;
        if inventory.hosts.is_empty() {
            bail!("No hosts defined in inventory");
        }
        let regex_pattern = format!("^{}$", pattern.replace("*", ".*"));
        let re = regex::Regex::new(&regex_pattern)
            .map_err(|e| anyhow::anyhow!("Invalid pattern '{}': {}", pattern, e))?;
        let matched: Vec<String> = inventory.hosts.iter().filter(|h| re.is_match(h)).cloned().collect();
        if matched.is_empty() {
            bail!("No hosts match pattern '{}'", pattern);
        }
        matched
    } else {
        unreachable!()
    };

    // tty + -r → bail
    if run_config.tty && remotes.is_some() {
        bail!("Command '{}' requires tty (ssh -t) and cannot be used with --remotes", name);
    }

    // Confirm
    if run_config.confirm {
        let target_desc = if hosts.len() == 1 {
            hosts[0].clone()
        } else {
            format!("{} hosts ({})", hosts.len(), hosts.join(", "))
        };
        print!("Run {} on {}? [y/N] ", c!(&name, bold), target_desc);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted");
            return Ok(());
        }
    }

    // Single host (-t)
    if target.is_some() {
        let host = &hosts[0];
        if run_config.tty {
            // ssh -t with inherited stdio
            let status = Command::new("ssh")
                .args(["-t", host, &full_cmd])
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()?;
            if !status.success() {
                bail!("Command '{}' failed on {}", name, host);
            }
        } else {
            let output = Command::new("ssh")
                .args([host.as_str(), &full_cmd])
                .output()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.is_empty() {
                print!("{}", stdout);
            }
            if !stderr.is_empty() {
                eprint!("{}", stderr);
            }
            if !output.status.success() {
                bail!("Command '{}' failed on {}", name, host);
            }
        }
        return Ok(());
    }

    // Multi-host (-r) — parallel with spinners
    let total = hosts.len();
    println!("{} Running '{}' on {} host(s)...\n", c!("::", blue), name, total);
    let start = std::time::Instant::now();

    let mp = indicatif::MultiProgress::new();
    let spinners: Vec<_> = hosts.iter()
        .map(|host| output::start_deploy_spinner(&mp, host))
        .collect();

    let (tx, rx) = std::sync::mpsc::channel::<(usize, String, bool, std::time::Duration)>();

    std::thread::scope(|s| {
        for (i, host) in hosts.iter().enumerate() {
            let tx = tx.clone();
            let cmd = &full_cmd;
            let pb = &spinners[i];
            s.spawn(move || {
                let t = std::time::Instant::now();
                pb.set_message("running...");
                let result = Command::new("ssh")
                    .args([host.as_str(), cmd])
                    .output();
                let elapsed = t.elapsed();
                match result {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                        let combined = if stderr.is_empty() { stdout } else { format!("{}{}", stdout, stderr) };
                        let _ = tx.send((i, combined, out.status.success(), elapsed));
                    }
                    Err(e) => {
                        let _ = tx.send((i, e.to_string(), false, elapsed));
                    }
                }
            });
        }
        drop(tx);

        let mut failed_hosts: Vec<String> = Vec::new();
        for (i, output_text, success, elapsed) in rx {
            let pb = &spinners[i];
            let host = &hosts[i];
            let summary = output_text.lines().last().unwrap_or("").trim().to_string();
            if success {
                output::finish_deploy_ok(pb, host, &summary, elapsed);
            } else {
                let err = if summary.is_empty() { "failed".to_string() } else { summary };
                output::finish_deploy_fail(pb, host, &err, elapsed);
                failed_hosts.push(host.clone());
            }
        }

        // Summary
        let elapsed = start.elapsed();
        let timing = format!("({})", output::format_duration(elapsed));
        let succeeded = total - failed_hosts.len();
        println!();
        if failed_hosts.is_empty() {
            println!("{} {}/{} hosts completed {}", c!("✓", green), succeeded, total, c!(timing, dimmed));
        } else {
            println!("{} {}/{} hosts completed, {} failed {}", c!("!", yellow), succeeded, total, failed_hosts.len(), c!(timing, dimmed));
            for h in &failed_hosts {
                println!("  {} {}", c!("✗", red), h);
            }
        }

        if !failed_hosts.is_empty() {
            std::process::exit(1);
        }
    });

    Ok(())
}

fn run_command(config_path: Option<PathBuf>, name: Option<String>, args: Vec<String>) -> Result<()> {
    use std::process::Command;

    let path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&path)?;

    // Apply runtime vars from meta.toml
    let meta = config::load_meta(&resolved_path);
    check_min_version(meta.as_ref())?;
    if let Some(ref vars) = meta.as_ref().and_then(|m| m.vars.as_ref()) {
        config::apply_vars(vars, &[]);
    }

    let config = config::load_all(&resolved_path)?;

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
                    println!("  {} - {}", c!(cmd_name, bold), c!(desc, dimmed));
                } else {
                    println!("  {}", c!(cmd_name, bold));
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

    // Confirm
    if run_config.confirm {
        use std::io::{self, Write};
        print!("Run {}? [y/N] ", c!(&name, bold));
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted");
            return Ok(());
        }
    }

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

/// Derive the test container name from config metadata.
fn test_container_name(config_path: Option<PathBuf>) -> Result<String> {
    let config_path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&config_path)?;
    let meta = config::load_meta(&resolved_path);
    let config_name = meta.as_ref().and_then(|m| m.name.as_deref())
        .unwrap_or_else(|| {
            resolved_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("dek")
        });
    let sanitized: String = config_name.to_lowercase().chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    Ok(format!("dek-test-{}", sanitized.trim_matches('-')))
}

fn run_exec(config_path: Option<PathBuf>, cmd: Vec<String>) -> Result<()> {
    if which::which("docker").is_err() {
        bail!("docker not found in PATH");
    }

    let container_name = test_container_name(config_path)?;

    if get_container_state(&container_name).as_deref() != Some("running") {
        bail!("Container '{}' is not running. Start it with: dek test", container_name);
    }

    let mut args = vec!["exec".to_string()];
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        args.extend(["-it".to_string()]);
    }
    args.push(container_name);
    args.extend(cmd);

    let status = Command::new("docker")
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn run_test(
    config_path: Option<PathBuf>, image: Option<String>, rm: bool,
    fresh: bool, attach: bool, selectors: Vec<String>,
) -> Result<()> {
    if which::which("docker").is_err() {
        bail!("docker not found in PATH");
    }

    let config_path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&config_path)?;
    let meta = config::load_meta(&resolved_path);
    check_min_version(meta.as_ref())?;
    let test_config = meta.as_ref().and_then(|m| m.test.as_ref());

    // Derive image: CLI > meta.toml > "archlinux"
    let image = image
        .or_else(|| test_config.and_then(|t| t.image.clone()))
        .unwrap_or_else(|| "archlinux".to_string());

    // Container name from config identity
    let container_name = test_container_name(Some(resolved_path.clone()))?;

    // Check existing container state
    let container_state = get_container_state(&container_name);

    // --attach: just attach to existing (no rebuild)
    if attach {
        match container_state.as_deref() {
            Some("running") => return docker_shell(&container_name),
            Some(_) => {
                docker_start(&container_name)?;
                return docker_shell(&container_name);
            }
            None => bail!("No container '{}' to attach to", container_name),
        }
    }

    // Handle --fresh: remove old container
    if fresh {
        if container_state.is_some() {
            println!("  {} Removing old container...", c!("→", yellow));
            let _ = Command::new("docker").args(["rm", "-f", &container_name])
                .stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }

    let is_new = fresh || container_state.is_none();

    if is_new {
        output::print_header(&format!("Testing in {}", image));
    } else {
        output::print_header(&format!("Updating {}", container_name));
    }
    println!();

    // Build dek binary (musl for portability across containers/distros)
    let cwd = std::env::current_dir()?;
    let musl_target = "x86_64-unknown-linux-musl";
    let dek_binary = if cwd.join("Cargo.toml").exists() {
        println!("  {} Building dek (musl)...", c!("→", yellow));
        let build_status = Command::new("cargo")
            .args(["build", "--release", "--quiet", "--target", musl_target])
            .status()?;
        if !build_status.success() {
            bail!("cargo build failed (is the musl target installed? rustup target add {})", musl_target);
        }
        cwd.join(format!("target/{}/release/dek", musl_target))
    } else {
        std::env::current_exe()?
    };

    // Prepare config (artifacts + includes)
    let dek_config = config::load_all(&resolved_path)?;
    let prepared_path = prepare_config(&resolved_path, &dek_config)?;

    // Bake into standalone binary
    let baked_path = PathBuf::from(format!("/tmp/{}", container_name));
    println!("  {} Baking config into binary...", c!("→", yellow));
    bake::create_baked_binary(&prepared_path, &dek_binary, &baked_path)?;

    if is_new {
        // Create new container with keep-alive process
        println!("  {} Creating container...", c!("→", yellow));
        let mut create_args = vec!["create", "--name", &container_name, "-w", "/root"];
        let config_dir = if resolved_path.is_file() {
            resolved_path.parent().unwrap_or(std::path::Path::new("."))
        } else {
            resolved_path.as_path()
        };
        let mounts: Vec<String> = test_config
            .map(|t| t.mount.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                // Resolve relative host paths against config dir
                if let Some((host, rest)) = m.split_once(':') {
                    if !host.starts_with('/') && !host.starts_with('~') {
                        let joined = config_dir.join(host);
                        // Create dir so canonicalize works (Docker needs absolute paths)
                        let _ = std::fs::create_dir_all(&joined);
                        let abs = joined.canonicalize().unwrap_or(joined);
                        return format!("{}:{}", abs.display(), rest);
                    }
                }
                m
            })
            .collect();
        for m in &mounts {
            create_args.push("-v");
            create_args.push(m);
        }
        create_args.extend_from_slice(&[&image, "tail", "-f", "/dev/null"]);
        let create_status = Command::new("docker")
            .args(&create_args)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()?;
        if !create_status.success() {
            bail!("Failed to create container");
        }
    }

    // Copy baked binary into container
    println!("  {} Copying dek into container...", c!("→", yellow));
    let cp_status = Command::new("docker")
        .args(["cp", &baked_path.to_string_lossy(), &format!("{}:/usr/local/bin/dek", container_name)])
        .status()?;
    if !cp_status.success() {
        bail!("Failed to copy binary into container");
    }

    // Ensure container is running
    if get_container_state(&container_name).as_deref() != Some("running") {
        docker_start(&container_name)?;
    }

    // Apply config inside container
    println!("  {} Applying config...", c!("→", yellow));
    println!();

    let mut apply_args = vec!["exec".to_string(), container_name.clone(),
                              "dek".to_string(), "apply".to_string()];
    apply_args.extend(selectors);

    let apply_status = Command::new("docker")
        .args(&apply_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !apply_status.success() {
        println!();
        println!("  {} Apply had errors, dropping into shell anyway", c!("!", yellow));
    }

    // Drop into shell
    println!();
    println!("Dropping into shell...");
    docker_shell(&container_name)?;

    if rm {
        let _ = Command::new("docker").args(["rm", "-f", &container_name])
            .stdout(Stdio::null()).stderr(Stdio::null()).status();
        println!("Container removed: {}", container_name);
    } else {
        println!();
        println!("Container kept: {}", c!(container_name, bold));
        println!("  Rerun:     {}", c!("dek test", dimmed));
        println!("  Attach:    {}", c!("dek test --attach", dimmed));
        println!("  Fresh:     {}", c!("dek test --fresh", dimmed));
        println!("  Remove:    {}", c!(format!("docker rm {}", container_name), dimmed));
    }

    Ok(())
}

fn get_container_state(name: &str) -> Option<String> {
    let output = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Status}}", name])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if state.is_empty() { None } else { Some(state) }
}

fn docker_start(name: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["start", name])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        bail!("Failed to start container '{}'", name);
    }
    Ok(())
}

fn docker_shell(name: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["exec", "-it", name, "bash", "-l"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .or_else(|_| {
            Command::new("docker")
                .args(["exec", "-it", name, "sh"])
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
        })?;
    if !status.success() {
        bail!("docker exec exited with status {}", status);
    }
    Ok(())
}

fn eval_probe(probe: &config::StateConfig) -> String {
    let output = Command::new("sh")
        .args(["-c", &probe.cmd])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok();
    let raw = output
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    for rule in &probe.rewrite {
        if let Ok(re) = regex::Regex::new(&rule.pattern) {
            if re.is_match(&raw) {
                return rule.value.clone();
            }
        }
    }
    raw
}

fn run_state(config_path: Option<PathBuf>, name: Option<String>, json: bool, args: Vec<String>) -> Result<()> {
    let path = resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&path)?;
    let cfg = config::load_all(&resolved_path)?;

    if cfg.state.is_empty() {
        bail!("No state probes defined in config");
    }

    // --json may end up in args due to trailing_var_arg
    let json = json || args.iter().any(|a| a == "--json");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--json").collect();

    // Collect all requested names
    let mut names: Vec<String> = Vec::new();
    if let Some(ref n) = name {
        names.push(n.clone());
    }

    // Detect operator mode (is/isnot/get)
    let has_op = name.is_some()
        && !args.is_empty()
        && matches!(args[0].as_str(), "is" | "isnot" | "get");

    if !has_op {
        names.extend(args.iter().cloned());
    }

    // Operator mode: single probe
    if has_op {
        let probe_name = name.as_ref().unwrap();
        let probe = cfg.state.iter()
            .find(|p| p.name == *probe_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown state probe: {}", probe_name))?;
        let value = eval_probe(probe);
        let op = &args[0];
        match op.as_str() {
            "is" => {
                let expected = args.get(1)
                    .ok_or_else(|| anyhow::anyhow!("Missing value after 'is'"))?;
                if value != *expected { std::process::exit(1); }
            }
            "isnot" => {
                let expected = args.get(1)
                    .ok_or_else(|| anyhow::anyhow!("Missing value after 'isnot'"))?;
                if value == *expected { std::process::exit(1); }
            }
            "get" => {
                if args.len() < 3 {
                    bail!("Usage: dek state <name> get <val>... <default>");
                }
                let allowed = &args[1..args.len() - 1];
                let fallback = &args[args.len() - 1];
                if allowed.iter().any(|a| a == &value) {
                    print!("{}", value);
                } else {
                    print!("{}", fallback);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Filter probes
    let probes: Vec<&config::StateConfig> = if names.is_empty() {
        cfg.state.iter().collect()
    } else {
        let mut selected = Vec::new();
        for n in &names {
            let probe = cfg.state.iter()
                .find(|p| p.name == *n)
                .ok_or_else(|| anyhow::anyhow!("Unknown state probe: {}", n))?;
            selected.push(probe);
        }
        selected
    };

    // Single probe, no json → plain value
    if probes.len() == 1 && !json && names.len() == 1 {
        println!("{}", eval_probe(probes[0]));
        return Ok(());
    }

    // Parallel eval, config order
    let results: Vec<(String, String)> = std::thread::scope(|s| {
        let handles: Vec<_> = probes.iter().map(|probe| {
            s.spawn(|| (probe.name.clone(), eval_probe(probe)))
        }).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    if json {
        let mut map = serde_json::Map::new();
        for (k, v) in results {
            map.insert(k, serde_json::Value::String(v));
        }
        println!("{}", serde_json::Value::Object(map));
    } else {
        let max_name = results.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        for (name, value) in &results {
            println!("  {:>width$}  {}",
                c!(name, cyan), c!(value, bold),
                width = max_name);
        }
    }
    Ok(())
}

fn run_setup() -> Result<()> {
    use std::fs;

    output::print_header("Setting up dek");
    println!();

    let shell = util::Shell::detect();
    println!("  {} Detected shell: {}", c!("•", blue), shell.name());

    // Generate completions (custom scripts with dynamic completion support)
    let completions_str = match shell {
        util::Shell::Zsh => zsh_completions(),
        util::Shell::Bash => bash_completions(),
        util::Shell::Fish => fish_completions(),
    };

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
    println!("  {} Wrote completions to {}", c!("✓", green), comp_path);

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
            println!("  {} Added completions to .zshrc", c!("✓", green));
        } else {
            println!("  {} Completions already configured in .zshrc", c!("•", dimmed));
        }
    }

    println!();
    println!("  {} Restart your shell or run: exec {}", c!("✓", green), shell.name());

    Ok(())
}

fn print_rich_help(meta: Option<&config::Meta>, config_path: &PathBuf) -> Result<()> {
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "dek".to_string());

    let name = meta.and_then(|m| m.name.as_deref()).unwrap_or(&exe_name);
    let cfg = config::load_all(config_path)?;
    let configs = config::list_configs(config_path, meta)?;

    // Banner or header
    if let Some(banner) = meta.and_then(|m| m.banner.as_ref()) {
        println!();
        for line in banner.lines() {
            println!("  {}", c!(line, bold));
        }
    } else {
        println!();
        println!("  {}", c!(name, bold));
    }
    let desc = meta.and_then(|m| m.description.as_ref());
    let version = meta.and_then(|m| m.version.as_ref());
    match (desc, version) {
        (Some(d), Some(v)) => println!("  {} {}", c!(d, dimmed), c!(format!("v{}", v), dimmed)),
        (Some(d), None) => println!("  {}", c!(d, dimmed)),
        (None, Some(v)) => println!("  {}", c!(format!("v{}", v), dimmed)),
        (None, None) => {}
    }
    if let Some(info) = bake::get_bake_info() {
        println!("  {}", c!("Powered by dek (https://github.com/zcag/dek)", dimmed));
        println!("  {}", c!(info, dimmed));
    }
    println!();

    // Usage
    println!("  {}", c!("USAGE", dimmed));
    println!("    {} {} {}", c!(exe_name, cyan), c!("[OPTIONS]", dimmed), c!("<COMMAND>", white));
    println!();

    // Commands
    println!("  {}", c!("COMMANDS", dimmed));
    println!("    {} {}  {}", c!("apply", white), c!("a", dimmed), c!("Apply configuration", dimmed));
    println!("    {} {}  {}", c!("check", white), c!("c", dimmed), c!("Check what would change (dry-run)", dimmed));
    println!("    {}  {}  {}", c!("plan", white), c!("p", dimmed), c!("List items from config", dimmed));
    println!("    {}   {}  {}", c!("run", white), c!("r", dimmed), c!("Run a command from config", dimmed));
    println!("    {}  {}  {}", c!("test", white), c!("t", dimmed), c!("Test in container", dimmed));
    println!("    {} {}  {}", c!("exec", white), c!("dx", dimmed), c!("Run command in test container", dimmed));
    println!("    {} {}  {}", c!("state", white), c!("s", dimmed), c!("Query system state probes", dimmed));
    println!("    {}  {}  {}", c!("bake", white), c!(" ", dimmed), c!("Bake into standalone binary", dimmed));
    println!();

    // Options
    println!("  {}", c!("OPTIONS", dimmed));
    println!("    {}  {}", c!("-C, --config <PATH>", white), c!("Config path", dimmed));
    println!("    {}  {}", c!("-t, --target <HOST>", white), c!("Remote target (user@host)", dimmed));
    println!("    {} {}", c!("-r, --remotes <PATTERN>", white), c!("Remote targets from inventory (glob)", dimmed));
    println!("    {}              {}", c!("-h, --help", white), c!("Print help", dimmed));
    println!("    {}           {}", c!("-V, --version", white), c!("Print version", dimmed));
    println!();

    // Available configs
    if !configs.is_empty() {
        let defaults = meta.map(|m| &m.defaults[..]).unwrap_or(&[]);
        if defaults.is_empty() {
            println!("  {}", c!("CONFIGS", dimmed));
        } else {
            let defaults_str = defaults.join(", ");
            println!("  {}  {}", c!("CONFIGS", dimmed), c!(format!("defaults: {}", defaults_str), dimmed));
        }
        for cfg_info in &configs {
            let label = if cfg_info.name != cfg_info.key {
                format!("{} ({})", cfg_info.key, cfg_info.name)
            } else {
                cfg_info.key.clone()
            };
            if cfg_info.is_default {
                print!("    {} {}", c!("•", green), c!(label, green));
            } else {
                print!("      {}", c!(label, white));
            }
            for l in &cfg_info.labels {
                print!(" {}", c!(format!("@{}", l), cyan));
            }
            if let Some(ref d) = cfg_info.description {
                print!("  {}", c!(d, dimmed));
            }
            println!();
        }
        println!();
    }

    // Run commands
    if let Some(run) = &cfg.run {
        if !run.is_empty() {
            println!("  {}", c!("RUN", dimmed));
            let mut cmds: Vec<_> = run.iter().collect();
            cmds.sort_by_key(|(k, _)| *k);
            for (cmd_name, rc) in cmds {
                if let Some(d) = &rc.description {
                    println!("    {}  {}", c!(cmd_name, yellow), c!(d, dimmed));
                } else {
                    println!("    {}", c!(cmd_name, yellow));
                }
            }
            println!();
        }
    }

    Ok(())
}

fn run_complete(config_path: Option<PathBuf>, what: &str) -> Result<()> {
    // Shell-agnostic check if completions are installed (for use in [[command]].check)
    if what == "check" {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = match util::Shell::detect() {
            util::Shell::Zsh => format!("{}/.zsh/completions/_dek", home),
            util::Shell::Bash => format!("{}/.local/share/bash-completion/completions/dek", home),
            util::Shell::Fish => format!("{}/.config/fish/completions/dek.fish", home),
        };
        if !std::path::Path::new(&path).exists() {
            std::process::exit(1);
        }
        return Ok(());
    }

    let path = match config_path
        .or_else(bake::check_embedded)
        .or_else(config::find_default_config)
    {
        Some(p) => p,
        None => return Ok(()),
    };
    let resolved = config::resolve_path(&path).unwrap_or(path);
    let meta = config::load_meta(&resolved);

    match what {
        "configs" => {
            let configs = config::list_configs(&resolved, meta.as_ref()).unwrap_or_default();
            for cfg in &configs {
                println!("{}", cfg.key);
            }
            let mut seen = std::collections::HashSet::new();
            for cfg in &configs {
                for l in &cfg.labels {
                    if seen.insert(l.clone()) {
                        println!("@{}", l);
                    }
                }
            }
        }
        "run" => {
            let config = config::load_all(&resolved).unwrap_or_default();
            if let Some(run) = &config.run {
                let mut cmds: Vec<_> = run.keys().collect();
                cmds.sort();
                for cmd in cmds {
                    println!("{}", cmd);
                }
            }
        }
        "state" => {
            let config = config::load_all(&resolved).unwrap_or_default();
            for probe in &config.state {
                println!("{}", probe.name);
            }
        }
        _ => {}
    }
    Ok(())
}

fn zsh_completions() -> String {
    r#"#compdef dek

_dek_configs() {
    local -a items
    items=(${(f)"$(dek _complete configs 2>/dev/null)"})
    [[ -n "$items" ]] && compadd -- $items
}

_dek_run_cmds() {
    local -a items
    items=(${(f)"$(dek _complete run 2>/dev/null)"})
    [[ -n "$items" ]] && compadd -- $items
}

_dek_state_probes() {
    local -a items
    items=(${(f)"$(dek _complete state 2>/dev/null)"})
    [[ -n "$items" ]] && compadd -- $items
}

_dek() {
    local curcontext="$curcontext" state
    local -a commands=(
        'apply:Apply configuration'
        'a:Apply configuration'
        'check:Check what would change'
        'c:Check what would change'
        'plan:List items from config'
        'p:List items from config'
        'run:Run a command'
        'r:Run a command'
        'test:Test in container'
        't:Test in container'
        'exec:Run in test container'
        'dx:Run in test container'
        'bake:Bake into standalone binary'
        'state:Query system state'
        's:Query system state'
        'setup:Install completions'
        'completions:Generate raw completions'
    )

    _arguments -C \
        '(-C --config)'{-C,--config}'[Config path]:path:_files' \
        '(-t --target)'{-t,--target}'[Remote target]:target:' \
        '(-r --remotes)'{-r,--remotes}'[Remote pattern]:pattern:' \
        '(-q --quiet)'{-q,--quiet}'[Suppress output]' \
        '--color[Color mode]:mode:(auto always never)' \
        '1:command:->cmd' \
        '*::arg:->args'

    case $state in
        cmd)
            _describe 'command' commands
            ;;
        args)
            case ${words[1]} in
                apply|a|check|c|plan|p)
                    _dek_configs
                    ;;
                run|r)
                    (( CURRENT == 2 )) && _dek_run_cmds
                    ;;
                state|s)
                    (( CURRENT == 2 )) && _dek_state_probes
                    ;;
                test|t)
                    _arguments \
                        '(-i --image)'{-i,--image}'[Base image]:image:' \
                        '(-r --rm)'{-r,--rm}'[Remove after exit]' \
                        '(-f --fresh)'{-f,--fresh}'[Force new container]' \
                        '(-a --attach)'{-a,--attach}'[Attach to existing]' \
                        '*:selector:_dek_configs'
                    ;;
                exec|dx)
                    _normal
                    ;;
                bake)
                    _arguments \
                        '(-o --output)'{-o,--output}'[Output path]:path:_files' \
                        '*:config:_files'
                    ;;
                completions)
                    _arguments '1:shell:(bash zsh fish)'
                    ;;
            esac
            ;;
    esac
}

_dek "$@"
"#.to_string()
}

fn bash_completions() -> String {
    r#"_dek() {
    local cur prev words cword
    _init_completion || return

    local commands="apply a check c plan p run r state s test t exec dx bake setup completions"

    # Find the subcommand
    local cmd="" cmd_idx=0
    for ((i=1; i<cword; i++)); do
        case "${words[i]}" in
            -C|--config|-t|--target|-r|--remotes|--color) ((i++)); continue ;;
            -*) continue ;;
            *) cmd="${words[i]}"; cmd_idx=$i; break ;;
        esac
    done

    # Complete subcommand
    if [[ -z "$cmd" ]]; then
        COMPREPLY=($(compgen -W "$commands" -- "$cur"))
        return
    fi

    case $cmd in
        apply|a|check|c|plan|p)
            COMPREPLY=($(compgen -W "$(dek _complete configs 2>/dev/null)" -- "$cur"))
            ;;
        run|r)
            if [[ $cword -eq $((cmd_idx+1)) ]]; then
                COMPREPLY=($(compgen -W "$(dek _complete run 2>/dev/null)" -- "$cur"))
            fi
            ;;
        state|s)
            if [[ $cword -eq $((cmd_idx+1)) ]]; then
                COMPREPLY=($(compgen -W "$(dek _complete state 2>/dev/null)" -- "$cur"))
            fi
            ;;
        test|t)
            case $prev in
                -i|--image) return ;;
            esac
            if [[ $cur == -* ]]; then
                COMPREPLY=($(compgen -W "-i --image -r --rm -f --fresh -a --attach" -- "$cur"))
            else
                COMPREPLY=($(compgen -W "$(dek _complete configs 2>/dev/null)" -- "$cur"))
            fi
            ;;
        completions)
            COMPREPLY=($(compgen -W "bash zsh fish" -- "$cur"))
            ;;
    esac
}

complete -F _dek dek
"#.to_string()
}

fn fish_completions() -> String {
    r#"# Subcommands
set -l commands apply a check c plan p run r state s test t exec dx bake setup completions

complete -c dek -n "not __fish_seen_subcommand_from $commands" -a apply -d 'Apply configuration'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a a -d 'Apply configuration'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a check -d 'Check what would change'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a c -d 'Check what would change'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a plan -d 'List items from config'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a p -d 'List items from config'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a run -d 'Run a command'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a r -d 'Run a command'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a test -d 'Test in container'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a t -d 'Test in container'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a exec -d 'Run in test container'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a dx -d 'Run in test container'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a state -d 'Query system state'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a s -d 'Query system state'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a bake -d 'Bake into standalone binary'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a setup -d 'Install completions'
complete -c dek -n "not __fish_seen_subcommand_from $commands" -a completions -d 'Generate raw completions'

# Global options
complete -c dek -s C -l config -d 'Config path' -r -F
complete -c dek -s t -l target -d 'Remote target' -r
complete -c dek -s r -l remotes -d 'Remote pattern' -r
complete -c dek -s q -l quiet -d 'Suppress output'
complete -c dek -l color -d 'Color mode' -r -a 'auto always never'

# Dynamic completions for apply/check/plan and aliases
for cmd in apply a check c plan p
    complete -c dek -n "__fish_seen_subcommand_from $cmd" -a "(dek _complete configs 2>/dev/null)" -f
end

# Dynamic completions for run and alias
for cmd in run r
    complete -c dek -n "__fish_seen_subcommand_from $cmd" -a "(dek _complete run 2>/dev/null)" -f
end

# Dynamic completions for state and alias
for cmd in state s
    complete -c dek -n "__fish_seen_subcommand_from $cmd" -a "(dek _complete state 2>/dev/null)" -f
end

# Test flags and dynamic completions
for cmd in test t
    complete -c dek -n "__fish_seen_subcommand_from $cmd" -s i -l image -d 'Base image' -r
    complete -c dek -n "__fish_seen_subcommand_from $cmd" -s r -l rm -d 'Remove after exit'
    complete -c dek -n "__fish_seen_subcommand_from $cmd" -s f -l fresh -d 'Force new container'
    complete -c dek -n "__fish_seen_subcommand_from $cmd" -s a -l attach -d 'Attach to existing'
    complete -c dek -n "__fish_seen_subcommand_from $cmd" -a "(dek _complete configs 2>/dev/null)" -f
end

# Completions subcommand
complete -c dek -n "__fish_seen_subcommand_from completions" -a "bash zsh fish" -f
"#.to_string()
}
