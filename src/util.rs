use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// Detected system package manager
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SysPkgManager {
    Apt,
    Pacman,
    Dnf,
    Yum,
    Brew,
}

impl SysPkgManager {
    /// Detect the system package manager
    pub fn detect() -> Option<Self> {
        if command_exists("pacman") {
            Some(Self::Pacman)
        } else if command_exists("apt-get") {
            Some(Self::Apt)
        } else if command_exists("dnf") {
            Some(Self::Dnf)
        } else if command_exists("yum") {
            Some(Self::Yum)
        } else if command_exists("brew") {
            Some(Self::Brew)
        } else {
            None
        }
    }

    /// Install a package using this package manager
    pub fn install(&self, pkg: &str) -> Result<()> {
        let output = match self {
            Self::Pacman => run_sudo("pacman", &["-Sy", "--noconfirm", pkg])?,
            Self::Apt => run_sudo("apt-get", &["install", "-y", pkg])?,
            Self::Dnf => run_sudo("dnf", &["install", "-y", pkg])?,
            Self::Yum => run_sudo("yum", &["install", "-y", pkg])?,
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

    /// Get the package name for a tool (may differ across package managers)
    #[allow(dead_code)]
    pub fn package_name<'a>(&self, tool: &'a str) -> &'a str {
        match (self, tool) {
            // Go
            (Self::Pacman, "go") => "go",
            (Self::Apt, "go") => "golang",
            (Self::Dnf | Self::Yum, "go") => "golang",
            (Self::Brew, "go") => "go",
            // Node/npm
            (Self::Pacman, "npm") => "npm",
            (Self::Apt, "npm") => "npm",
            (Self::Dnf | Self::Yum, "npm") => "npm",
            (Self::Brew, "npm") => "node",
            // Python/pip
            (Self::Pacman, "pip") => "python-pip",
            (Self::Apt, "pip") => "python3-pip",
            (Self::Dnf | Self::Yum, "pip") => "python3-pip",
            (Self::Brew, "pip") => "python",
            // Default: use the tool name
            _ => tool,
        }
    }
}

/// Run a script from a URL via curl | sh
pub fn run_install_script(url: &str, args: &[&str]) -> Result<()> {
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
