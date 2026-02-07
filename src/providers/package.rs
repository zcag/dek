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
        let Some(pm) = SysPkgManager::detect() else {
            return WebiProvider.check(state);
        };

        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let installed = match pm {
            SysPkgManager::Pacman => run_cmd_ok("pacman", &["-Q", &pkg_name]),
            SysPkgManager::Apt => {
                let output = run_cmd("dpkg-query", &["-W", "-f=${Status}", &pkg_name])?;
                String::from_utf8_lossy(&output.stdout).contains("install ok installed")
            }
            SysPkgManager::Brew => run_cmd_ok("brew", &["list", &pkg_name]),
        };

        if installed {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("package '{}' not installed", pkg_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let Some(pm) = SysPkgManager::detect() else {
            return WebiProvider.apply(state);
        };

        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        pm.install(&pkg_name)
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
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let output = run_cmd("dpkg-query", &["-W", "-f=${Status}", &pkg_name])?;
        let status = String::from_utf8_lossy(&output.stdout);

        if status.contains("install ok installed") {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("package '{}' not installed", pkg_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let output = run_sudo("apt-get", &["install", "-y", &pkg_name])?;
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
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let ok = run_cmd_ok("pacman", &["-Q", &pkg_name]);
        if ok {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("package '{}' not installed", pkg_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let output = run_sudo("pacman", &["-S", "--noconfirm", &pkg_name])?;
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
        let (_, bin_name) = crate::util::parse_spec(&state.key);
        if command_exists(&bin_name) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("'{}' not in PATH", bin_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);

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
        let (pkg_name, _) = go_parse_spec(&state.key);
        let output = run_cmd("go", &["install", &pkg_name])?;
        if !output.status.success() {
            bail!("go install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}

/// Parse go spec: supports explicit "pkg:bin" or derives binary from path
fn go_parse_spec(spec: &str) -> (String, String) {
    if let Some((pkg, bin)) = spec.split_once(':') {
        (pkg.to_string(), bin.to_string())
    } else {
        let bin = go_bin_from_path(spec);
        (spec.to_string(), bin)
    }
}

/// Get binary name from go package path (last segment, stripping @version)
fn go_bin_name(spec: &str) -> String {
    // Check for explicit :bin first
    if let Some((_, bin)) = spec.split_once(':') {
        return bin.to_string();
    }
    go_bin_from_path(spec)
}

fn go_bin_from_path(pkg: &str) -> String {
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
        let (_, bin_name) = crate::util::parse_spec(&state.key);
        if command_exists(&bin_name) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("'{}' not in PATH", bin_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
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

// =============================================================================
// NPM
// =============================================================================

pub struct NpmProvider;

impl Provider for NpmProvider {
    fn name(&self) -> &'static str {
        "package.npm"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::binary("npm", InstallMethod::Webi("node"))]
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let ok = run_cmd_ok("npm", &["list", "-g", &pkg_name, "--depth=0"]);
        if ok {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("npm package '{}' not installed globally", pkg_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let output = run_cmd("npm", &["install", "-g", &pkg_name])?;
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
        vec![Requirement::binary("pip3", InstallMethod::Webi("python"))]
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let ok = run_cmd_ok("pip3", &["show", &pkg_name])
            || run_cmd_ok("pip", &["show", &pkg_name]);
        if ok {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("pip package '{}' not installed", pkg_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let pip = if command_exists("pip3") { "pip3" } else { "pip" };
        let output = run_cmd(pip, &["install", "--user", &pkg_name])?;
        if !output.status.success() {
            bail!("pip install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}

// =============================================================================
// PIPX
// =============================================================================

pub struct PipxProvider;

impl Provider for PipxProvider {
    fn name(&self) -> &'static str {
        "package.pipx"
    }

    fn requires(&self) -> Vec<Requirement> {
        vec![Requirement::binary("pipx", InstallMethod::Pip("pipx"))]
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let (_, bin_name) = crate::util::parse_spec(&state.key);
        if command_exists(&bin_name) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("'{}' not in PATH", bin_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let (pkg_name, _) = crate::util::parse_spec(&state.key);
        let output = run_cmd("pipx", &["install", &pkg_name])?;
        if !output.status.success() {
            bail!("pipx install failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }
}
