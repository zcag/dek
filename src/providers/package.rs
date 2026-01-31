use super::{CheckResult, InstallMethod, Provider, Requirement, StateItem};
use crate::util::{command_exists, run_cmd, run_cmd_ok, run_sudo, SysPkgManager};
use anyhow::{bail, Result};

// =============================================================================
// OS (auto-detect system package manager)
// =============================================================================

pub struct OsProvider;

impl Provider for OsProvider {
    fn name(&self) -> &'static str {
        "package.os"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let pm = SysPkgManager::detect()
            .ok_or_else(|| anyhow::anyhow!("No supported package manager found"))?;

        let installed = match pm {
            SysPkgManager::Pacman => run_cmd_ok("pacman", &["-Q", &state.key]),
            SysPkgManager::Apt => {
                let output = run_cmd("dpkg-query", &["-W", "-f=${Status}", &state.key])?;
                String::from_utf8_lossy(&output.stdout).contains("install ok installed")
            }
            SysPkgManager::Dnf | SysPkgManager::Yum => run_cmd_ok("rpm", &["-q", &state.key]),
            SysPkgManager::Brew => run_cmd_ok("brew", &["list", &state.key]),
        };

        if installed {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("package '{}' not installed", state.key),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let pm = SysPkgManager::detect()
            .ok_or_else(|| anyhow::anyhow!("No supported package manager found"))?;
        pm.install(&state.key)
    }
}

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
// PACMAN
// =============================================================================

pub struct PacmanProvider;

impl Provider for PacmanProvider {
    fn name(&self) -> &'static str {
        "package.pacman"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let ok = run_cmd_ok("pacman", &["-Q", &state.key]);
        if ok {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("package '{}' not installed", state.key),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let output = run_sudo("pacman", &["-S", "--noconfirm", &state.key])?;
        if !output.status.success() {
            bail!("pacman install failed: {}", String::from_utf8_lossy(&output.stderr));
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
            Requirement::binary("cargo-binstall", InstallMethod::CargoBinstall),
        ]
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let (_, bin_name) = parse_cargo_spec(&state.key);
        if command_exists(&bin_name) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("'{}' not in PATH", bin_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = parse_cargo_spec(&state.key);

        // Try binstall first (pre-compiled), fall back to install (compile)
        let output = run_cmd("cargo", &["binstall", "-y", &pkg_name])?;
        if output.status.success() {
            return Ok(());
        }

        let output = run_cmd("cargo", &["install", &pkg_name])?;
        if !output.status.success() {
            bail!("cargo install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}

/// Parse cargo spec: "pkg:bin" or "pkg" (bin defaults to pkg or known mapping)
fn parse_cargo_spec(spec: &str) -> (String, String) {
    if let Some((pkg, bin)) = spec.split_once(':') {
        (pkg.to_string(), bin.to_string())
    } else {
        let bin = cargo_bin_name(spec);
        (spec.to_string(), bin)
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
        vec![Requirement::binary("go", InstallMethod::Webi("golang"))]
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
// WEBI
// =============================================================================

pub struct WebiProvider;

impl Provider for WebiProvider {
    fn name(&self) -> &'static str {
        "package.webi"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let (_, bin_name) = parse_webi_spec(&state.key);
        if command_exists(&bin_name) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("'{}' not in PATH", bin_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = parse_webi_spec(&state.key);
        let url = format!("https://webi.sh/{}", pkg_name);
        crate::util::run_install_script(&url, &[])?;

        // Webi installs to various paths, ensure they're in PATH
        if let Ok(home) = std::env::var("HOME") {
            let webi_paths = [
                format!("{}/.local/bin", home),
                format!("{}/.local/opt/go/bin", home),
                format!("{}/go/bin", home),
            ];
            if let Ok(path) = std::env::var("PATH") {
                let mut new_path = path.clone();
                for p in &webi_paths {
                    if !new_path.contains(p) {
                        new_path = format!("{}:{}", p, new_path);
                    }
                }
                std::env::set_var("PATH", new_path);
            }
        }
        Ok(())
    }
}

/// Parse webi spec: "pkg:bin" or "pkg" (bin defaults to pkg)
fn parse_webi_spec(spec: &str) -> (String, String) {
    if let Some((pkg, bin)) = spec.split_once(':') {
        (pkg.to_string(), bin.to_string())
    } else {
        (spec.to_string(), spec.to_string())
    }
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
