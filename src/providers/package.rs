use super::{CheckResult, InstallMethod, Provider, Requirement, StateItem};
use crate::util::{command_exists, run_cmd, run_cmd_ok, run_sudo};
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
            bail!("apt-get install failed: {}", String::from_utf8_lossy(&output.stderr));
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

    fn requires(&self) -> Vec<Requirement> {
        vec![
            Requirement::binary("cargo", InstallMethod::Rustup),
            Requirement::binary("cargo-binstall", InstallMethod::Cargo("cargo-binstall")),
        ]
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
        // Try binstall first (pre-compiled), fall back to install (compile)
        let output = run_cmd("cargo", &["binstall", "-y", &state.key])?;
        if output.status.success() {
            return Ok(());
        }

        let output = run_cmd("cargo", &["install", &state.key])?;
        if !output.status.success() {
            bail!("cargo install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}

fn cargo_bin_name(pkg: &str) -> String {
    match pkg {
        "ripgrep" => "rg",
        "fd-find" => "fd",
        "du-dust" => "dust",
        "bottom" => "btm",
        _ => pkg,
    }
    .to_string()
}

// =============================================================================
// GO
// =============================================================================

pub struct GoProvider;

impl Provider for GoProvider {
    fn name(&self) -> &'static str {
        "package.go"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::binary("go", InstallMethod::System("go"))]
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
        let output = run_cmd("go", &["install", &state.key])?;
        if !output.status.success() {
            bail!("go install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
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

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::binary("npm", InstallMethod::System("npm"))]
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
        let output = run_cmd("npm", &["install", "-g", &state.key])?;
        if !output.status.success() {
            bail!("npm install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}

// =============================================================================
// PIP
// =============================================================================

pub struct PipProvider;

impl Provider for PipProvider {
    fn name(&self) -> &'static str {
        "package.pip"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::binary("pip3", InstallMethod::System("python3-pip"))]
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
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
        let pip = if command_exists("pip3") { "pip3" } else { "pip" };
        let output = run_cmd(pip, &["install", "--user", &state.key])?;
        if !output.status.success() {
            bail!("pip install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}
