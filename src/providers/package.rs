use super::{CheckResult, Provider, StateItem};
use crate::util::{command_exists, run_cmd, run_cmd_ok, run_install_script, run_sudo, SysPkgManager};
use anyhow::{bail, Result};

// =============================================================================
// APT
// =============================================================================

pub struct AptProvider;

impl Provider for AptProvider {
    fn name(&self) -> &'static str {
        "package.apt"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let output = run_cmd("dpkg-query", &["-W", "-f=${Status}", &state.key])?;
        let status = String::from_utf8_lossy(&output.stdout);

        if status.contains("install ok installed") {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("package '{}' not installed", state.key),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let output = run_sudo("apt-get", &["install", "-y", &state.key])?;
        if !output.status.success() {
            bail!(
                "apt-get install failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
}

// =============================================================================
// CARGO
// =============================================================================

pub struct CargoProvider;

impl Provider for CargoProvider {
    fn name(&self) -> &'static str {
        "package.cargo"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let bin_name = cargo_bin_name(&state.key);
        if command_exists(&bin_name) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("'{}' not in PATH", bin_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        ensure_cargo()?;
        let output = run_cmd("cargo", &["install", &state.key])?;
        if !output.status.success() {
            bail!("cargo install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}

fn ensure_cargo() -> Result<()> {
    if command_exists("cargo") {
        return Ok(());
    }
    println!("  → Installing rustup/cargo...");
    run_install_script("https://sh.rustup.rs", &["-y"])?;
    // Add to PATH for current process
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(path) = std::env::var("PATH") {
            std::env::set_var("PATH", format!("{}/.cargo/bin:{}", home, path));
        }
    }
    Ok(())
}

fn cargo_bin_name(pkg: &str) -> String {
    match pkg {
        "ripgrep" => "rg",
        "fd-find" => "fd",
        "du-dust" => "dust",
        "bottom" => "btm",
        _ => pkg,
    }.to_string()
}

// =============================================================================
// GO
// =============================================================================

pub struct GoProvider;

impl Provider for GoProvider {
    fn name(&self) -> &'static str {
        "package.go"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let bin_name = go_bin_name(&state.key);
        if command_exists(&bin_name) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("'{}' not in PATH", bin_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        ensure_go()?;
        let output = run_cmd("go", &["install", &state.key])?;
        if !output.status.success() {
            bail!("go install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}

fn ensure_go() -> Result<()> {
    if command_exists("go") {
        return Ok(());
    }
    println!("  → Installing go...");
    let pm = SysPkgManager::detect()
        .ok_or_else(|| anyhow::anyhow!("No supported package manager found to install go"))?;
    pm.install(pm.package_name("go"))?;
    Ok(())
}

fn go_bin_name(pkg: &str) -> String {
    let pkg = pkg.split('@').next().unwrap_or(pkg);
    pkg.rsplit('/').next().unwrap_or(pkg).to_string()
}

// =============================================================================
// NPM
// =============================================================================

pub struct NpmProvider;

impl Provider for NpmProvider {
    fn name(&self) -> &'static str {
        "package.npm"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let ok = run_cmd_ok("npm", &["list", "-g", &state.key, "--depth=0"]);
        if ok {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("npm package '{}' not installed globally", state.key),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        ensure_npm()?;

        let output = run_cmd("npm", &["install", "-g", &state.key])?;
        if !output.status.success() {
            bail!(
                "npm install failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
}

fn ensure_npm() -> Result<()> {
    if command_exists("npm") {
        return Ok(());
    }

    println!("  → Installing npm...");
    let pm = SysPkgManager::detect()
        .ok_or_else(|| anyhow::anyhow!("No supported package manager found to install npm"))?;

    pm.install(pm.package_name("npm"))?;
    Ok(())
}

// =============================================================================
// PIP
// =============================================================================

pub struct PipProvider;

impl Provider for PipProvider {
    fn name(&self) -> &'static str {
        "package.pip"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        // Try pip3 first, then pip
        let ok = run_cmd_ok("pip3", &["show", &state.key])
            || run_cmd_ok("pip", &["show", &state.key]);
        if ok {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("pip package '{}' not installed", state.key),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let pip = ensure_pip()?;

        let output = run_cmd(&pip, &["install", "--user", &state.key])?;
        if !output.status.success() {
            bail!(
                "pip install failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
}

fn ensure_pip() -> Result<String> {
    if command_exists("pip3") {
        return Ok("pip3".to_string());
    }
    if command_exists("pip") {
        return Ok("pip".to_string());
    }

    println!("  → Installing pip...");

    // Try ensurepip first
    if command_exists("python3") {
        let output = run_cmd("python3", &["-m", "ensurepip", "--user"])?;
        if output.status.success() {
            return Ok("pip3".to_string());
        }
    }

    // Fall back to system package manager
    let pm = SysPkgManager::detect()
        .ok_or_else(|| anyhow::anyhow!("No supported package manager found to install pip"))?;

    pm.install(pm.package_name("pip"))?;

    if command_exists("pip3") {
        Ok("pip3".to_string())
    } else if command_exists("pip") {
        Ok("pip".to_string())
    } else {
        bail!("pip installation failed")
    }
}
