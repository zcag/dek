use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
            Self::Pacman => run_sudo("pacman", &["-Sy", "--noconfirm", pkg])?,
            Self::Apt => run_sudo("apt-get", &["install", "-y", pkg])?,
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
