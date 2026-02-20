use anyhow::{Context, Result};
use indicatif::ProgressBar;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Parse package spec: "pkg:bin" or "pkg" (bin defaults to pkg)
pub fn parse_spec(spec: &str) -> (String, String) {
    if let Some((pkg, bin)) = spec.split_once(':') {
        (pkg.to_string(), bin.to_string())
    } else {
        (spec.to_string(), spec.to_string())
    }
}

/// Expand ~ to home directory
pub fn expand_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    let path_str = path.to_string_lossy();

    if path_str.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(&path_str[2..]);
        }
    } else if path_str == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }

    path.to_path_buf()
}

/// Run a command and return output
pub fn run_cmd(cmd: &str, args: &[&str]) -> Result<Output> {
    Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run: {} {}", cmd, args.join(" ")))
}

/// Run a command and check if it succeeded
pub fn run_cmd_ok(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run a command with sudo (or directly if already root)
pub fn run_sudo(cmd: &str, args: &[&str]) -> Result<Output> {
    // Skip sudo if running as root
    if unsafe { libc::geteuid() } == 0 {
        return run_cmd(cmd, args);
    }
    let mut sudo_args = vec![cmd];
    sudo_args.extend(args);
    run_cmd("sudo", &sudo_args)
}

/// Run a command with piped output, updating a spinner with each line
pub fn run_cmd_live(cmd: &str, args: &[&str], pb: &ProgressBar) -> Result<Output> {
    let child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to run: {} {}", cmd, args.join(" ")))?;
    run_cmd_live_inner(child, pb)
}

/// Run a command with piped output and custom working directory
pub fn run_cmd_live_dir(cmd: &str, args: &[&str], pb: &ProgressBar, dir: &Path) -> Result<Output> {
    let child = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to run: {} {}", cmd, args.join(" ")))?;
    run_cmd_live_inner(child, pb)
}

fn run_cmd_live_inner(mut child: std::process::Child, pb: &ProgressBar) -> Result<Output> {
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let pb2 = pb.clone();
    let stderr_thread = std::thread::spawn(move || {
        let mut collected = Vec::new();
        for line in BufReader::new(stderr).lines() {
            if let Ok(line) = line {
                crate::output::update_spinner(&pb2, &line);
                collected.extend(line.as_bytes().iter().copied());
                collected.push(b'\n');
            }
        }
        collected
    });

    let mut stdout_bytes = Vec::new();
    for line in BufReader::new(stdout).lines() {
        if let Ok(line) = line {
            crate::output::update_spinner(pb, &line);
            stdout_bytes.extend(line.as_bytes().iter().copied());
            stdout_bytes.push(b'\n');
        }
    }

    let status = child.wait()?;
    let stderr_bytes = stderr_thread.join().unwrap_or_default();

    Ok(Output {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
    })
}

/// Run a command with sudo and piped output, updating a spinner with each line.
/// Assumes sudo credentials are already cached (via pre-auth in runner).
pub fn run_sudo_live(cmd: &str, args: &[&str], pb: &ProgressBar) -> Result<Output> {
    if unsafe { libc::geteuid() } == 0 {
        return run_cmd_live(cmd, args, pb);
    }
    let mut sudo_args = vec![cmd];
    sudo_args.extend(args);
    run_cmd_live("sudo", &sudo_args, pb)
}

/// Run a command and return stdout as string
#[allow(dead_code)]
pub fn run_cmd_stdout(cmd: &str, args: &[&str]) -> Result<String> {
    let output = run_cmd(cmd, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Check if a command exists
pub fn command_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

/// User's shell type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
}

impl Shell {
    /// Detect the user's shell from $SHELL
    pub fn detect() -> Self {
        if let Ok(shell) = std::env::var("SHELL") {
            if shell.contains("zsh") {
                return Self::Zsh;
            } else if shell.contains("fish") {
                return Self::Fish;
            }
        }
        Self::Bash
    }

    /// Shell name for display
    pub fn name(&self) -> &'static str {
        match self {
            Self::Zsh => "zsh",
            Self::Bash => "bash",
            Self::Fish => "fish",
        }
    }

    /// Path to shell rc file
    pub fn rc_file(&self) -> &'static str {
        match self {
            Self::Zsh => "~/.zshrc",
            Self::Bash => "~/.bashrc",
            Self::Fish => "~/.config/fish/config.fish",
        }
    }
}

/// Detected system package manager
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SysPkgManager {
    Apt,
    Pacman,
    Brew,
}

impl SysPkgManager {
    /// Detect the system package manager
    pub fn detect() -> Option<Self> {
        if command_exists("pacman") {
            Some(Self::Pacman)
        } else if command_exists("apt-get") {
            Some(Self::Apt)
        } else if command_exists("brew") {
            Some(Self::Brew)
        } else {
            None
        }
    }

    /// Install a package using this package manager
    pub fn install(&self, pkg: &str) -> Result<()> {
        let output = match self {
            Self::Pacman => {
                let out = run_sudo("pacman", &["-S", "--noconfirm", pkg])?;
                if !out.status.success() {
                    // Pacman failed - try yay for AUR packages
                    return install_with_yay(pkg);
                }
                return Ok(());
            }
            Self::Apt => {
                // Run apt-get update if package lists are missing (fresh containers)
                let lists = std::path::Path::new("/var/lib/apt/lists");
                let has_lists = lists.is_dir() && std::fs::read_dir(lists)
                    .map(|mut d| d.any(|e| e.ok().map(|e| e.file_name().to_string_lossy().contains("_Packages")).unwrap_or(false)))
                    .unwrap_or(false);
                if !has_lists {
                    let update = run_sudo("apt-get", &["update", "-qq"])?;
                    if !update.status.success() {
                        anyhow::bail!("apt-get update failed");
                    }
                }
                run_sudo("apt-get", &["install", "-y", pkg])?
            }
            Self::Brew => run_cmd("brew", &["install", pkg])?,
        };

        if !output.status.success() {
            anyhow::bail!(
                "Failed to install '{}': {}",
                pkg,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
}

/// Install a package via yay (AUR helper), installing yay first if needed
pub fn install_with_yay(pkg: &str) -> Result<()> {
    if !command_exists("yay") {
        install_yay()?;
    }
    let output = run_cmd("yay", &["-S", "--noconfirm", pkg])?;
    if !output.status.success() {
        anyhow::bail!(
            "Failed to install '{}' via yay: {}",
            pkg,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Install a package via yay with live progress
pub fn install_with_yay_live(pkg: &str, pb: &ProgressBar) -> Result<()> {
    if !command_exists("yay") {
        install_yay()?;
    }
    let output = run_cmd_live("yay", &["-S", "--noconfirm", pkg], pb)?;
    if !output.status.success() {
        anyhow::bail!(
            "Failed to install '{}' via yay: {}",
            pkg,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Install yay from AUR
fn install_yay() -> Result<()> {
    use owo_colors::OwoColorize;
    println!("    {} installing yay...", c!("→", yellow));

    // Ensure base-devel and git
    let _ = run_sudo("pacman", &["-S", "--needed", "--noconfirm", "git", "base-devel"]);

    let tmp = "/tmp/dek-yay-install";
    let _ = std::fs::remove_dir_all(tmp);

    let clone = Command::new("git")
        .args(["clone", "https://aur.archlinux.org/yay.git", tmp])
        .output()
        .context("Failed to clone yay")?;
    if !clone.status.success() {
        anyhow::bail!("Failed to clone yay from AUR");
    }

    let build = Command::new("makepkg")
        .args(["-si", "--noconfirm"])
        .current_dir(tmp)
        .status()
        .context("Failed to build yay")?;
    if !build.success() {
        anyhow::bail!("Failed to build/install yay");
    }

    let _ = std::fs::remove_dir_all(tmp);
    Ok(())
}

/// Run a script from a URL via curl | sh
pub fn run_install_script(url: &str, args: &[&str]) -> Result<()> {
    // Ensure curl is available — install via system package manager if missing
    if !command_exists("curl") {
        if let Some(pm) = SysPkgManager::detect() {
            pm.install("curl")?;
        } else {
            anyhow::bail!("curl not found and no package manager available to install it");
        }
    }

    let curl = Command::new("curl")
        .args(["-fsSL", url])
        .output()
        .context("Failed to download install script")?;

    if !curl.status.success() {
        anyhow::bail!("Failed to download: {}", url);
    }

    let mut sh_args = vec!["-s", "--"];
    sh_args.extend(args);

    let sh = Command::new("sh")
        .args(&sh_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("Failed to spawn shell")?;

    let mut child = sh;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(&curl.stdout)?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("Install script failed");
    }

    Ok(())
}

/// Expand environment variables in a string: $VAR and ${VAR}.
/// Only expands $NAME and ${NAME} patterns. Other $ uses ($(...), $$, etc.) are preserved.
pub fn expand_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'{') {
                chars.next(); // skip {
                let name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                match std::env::var(&name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        result.push('$');
                        result.push('{');
                        result.push_str(&name);
                        result.push('}');
                    }
                }
            } else if chars.peek().map(|c| c.is_ascii_alphabetic() || *c == '_').unwrap_or(false) {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match std::env::var(&name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        result.push('$');
                        result.push_str(&name);
                    }
                }
            } else {
                // Not a var reference ($(...), $$, $!, etc.) — preserve as-is
                result.push('$');
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse a human-readable duration string (e.g. "1h", "30m", "1d", "1h30m")
pub fn parse_duration(s: &str) -> Result<std::time::Duration> {
    let mut total_secs: u64 = 0;
    let mut num_buf = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num_buf.push(c);
        } else {
            let n: u64 = num_buf.parse().with_context(|| format!("invalid duration: {}", s))?;
            num_buf.clear();
            total_secs += match c {
                's' => n,
                'm' => n * 60,
                'h' => n * 3600,
                'd' => n * 86400,
                _ => anyhow::bail!("unknown duration unit '{}' in: {}", c, s),
            };
        }
    }
    if !num_buf.is_empty() {
        // bare number = seconds
        let n: u64 = num_buf.parse().with_context(|| format!("invalid duration: {}", s))?;
        total_secs += n;
    }
    Ok(std::time::Duration::from_secs(total_secs))
}

/// Download a URL to bytes using curl, with file-based caching.
/// `max_age`: `None` = cache indefinitely, `Some(d)` = expire after duration.
pub fn fetch_url(url: &str, max_age: Option<std::time::Duration>) -> Result<Vec<u8>> {
    if let Some(data) = crate::cache::get(url, max_age) {
        return Ok(data);
    }
    if !command_exists("curl") {
        if let Some(pm) = SysPkgManager::detect() {
            pm.install("curl")?;
        } else {
            anyhow::bail!("curl not found and no package manager available");
        }
    }
    let output = Command::new("curl")
        .args(["-fsSL", url])
        .output()
        .with_context(|| format!("Failed to fetch: {}", url))?;
    if !output.status.success() {
        anyhow::bail!("Failed to download: {}", url);
    }
    crate::cache::set(url, &output.stdout);
    Ok(output.stdout)
}

/// Check if path is a tar.gz file
pub fn is_tar_gz(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".tar.gz") || name.ends_with(".tgz")
}

/// Extract tar.gz to cache directory, returns extracted path
pub fn extract_tar_gz(path: &Path) -> Result<PathBuf> {
    let data = fs::read(path).with_context(|| format!("Failed to read: {}", path.display()))?;

    let hash = format!("{:x}", md5::compute(&data));
    let cache_dir = PathBuf::from(format!("/tmp/dek-{}", hash));

    if cache_dir.exists() {
        return Ok(cache_dir);
    }

    let decoder = flate2::read::GzDecoder::new(&data[..]);
    let mut archive = tar::Archive::new(decoder);
    fs::create_dir_all(&cache_dir)
        .with_context(|| format!("Failed to create cache dir: {}", cache_dir.display()))?;
    archive
        .unpack(&cache_dir)
        .with_context(|| format!("Failed to extract: {}", path.display()))?;

    Ok(cache_dir)
}

/// Set DEK_LIB to data/functions.sh if it exists under the config directory.
pub fn init_lib(config_path: &Path) {
    let base = if config_path.is_dir() {
        config_path.to_path_buf()
    } else {
        config_path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    let lib = base.join("data/functions.sh");
    if lib.exists() {
        std::env::set_var("DEK_LIB", lib);
    }
}

/// Create a `sh -c` command, sourcing DEK_LIB first if set.
/// Uses bash when DEK_LIB is set, since functions.sh likely uses bash syntax.
pub fn shell_cmd(script: &str) -> Command {
    if let Ok(lib) = std::env::var("DEK_LIB") {
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(format!(". {lib}\n{script}"));
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }
}

/// Create tar.gz from a path (file or directory)
pub fn create_tar_gz(path: &Path) -> Result<Vec<u8>> {
    let mut tar_data = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut tar_data, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);

        if path.is_file() {
            let name = path.file_name().unwrap_or_default();
            tar.append_path_with_name(path, name)?;
        } else if path.is_dir() {
            tar.append_dir_all(".", path)?;
        } else {
            anyhow::bail!("Path does not exist: {}", path.display());
        }

        tar.into_inner()?.finish()?;
    }
    Ok(tar_data)
}
