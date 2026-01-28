pub mod file;
pub mod package;
pub mod service;
pub mod shell;

use crate::util::{command_exists, run_cmd, run_install_script, SysPkgManager};
use anyhow::{bail, Result};
use std::collections::HashSet;
use std::fmt;

/// Result of checking if a state is already satisfied
#[derive(Debug)]
pub enum CheckResult {
    Satisfied,
    Missing { detail: String },
}

impl CheckResult {
    pub fn is_satisfied(&self) -> bool {
        matches!(self, CheckResult::Satisfied)
    }
}

impl fmt::Display for CheckResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckResult::Satisfied => write!(f, "satisfied"),
            CheckResult::Missing { detail } => write!(f, "missing: {}", detail),
        }
    }
}

/// A single item of state to be checked/applied
#[derive(Debug, Clone)]
pub struct StateItem {
    pub kind: String,
    pub key: String,
    pub value: Option<String>,
}

impl StateItem {
    pub fn new(kind: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            key: key.into(),
            value: None,
        }
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
}

impl fmt::Display for StateItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.kind, self.key)
    }
}

// =============================================================================
// REQUIREMENTS
// =============================================================================

/// How to install a requirement
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum InstallMethod {
    /// Install via rustup (curl script)
    Rustup,
    /// Install via cargo install
    Cargo(&'static str),
    /// Install via system package manager
    System(&'static str),
    /// Install via go install
    Go(&'static str),
    /// Install via npm install -g
    Npm(&'static str),
    /// Install via pip install
    Pip(&'static str),
}

/// A requirement that must be satisfied before a provider can run
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Requirement {
    /// Binary that must exist in PATH
    pub binary: &'static str,
    /// How to install if missing
    pub install: InstallMethod,
}

impl Requirement {
    pub const fn binary(cmd: &'static str, install: InstallMethod) -> Self {
        Self { binary: cmd, install }
    }

    pub fn is_satisfied(&self) -> bool {
        command_exists(self.binary)
    }

    pub fn satisfy(&self) -> Result<()> {
        if self.is_satisfied() {
            return Ok(());
        }

        use owo_colors::OwoColorize;
        println!("    {} installing {}...", "→".yellow(), self.binary);

        match &self.install {
            InstallMethod::Rustup => {
                run_install_script("https://sh.rustup.rs", &["-y"])?;
                // Add to PATH
                if let Ok(home) = std::env::var("HOME") {
                    if let Ok(path) = std::env::var("PATH") {
                        std::env::set_var("PATH", format!("{}/.cargo/bin:{}", home, path));
                    }
                }
            }
            InstallMethod::Cargo(pkg) => {
                let output = run_cmd("cargo", &["install", pkg])?;
                if !output.status.success() {
                    bail!("cargo install {} failed", pkg);
                }
            }
            InstallMethod::System(pkg) => {
                let pm = SysPkgManager::detect()
                    .ok_or_else(|| anyhow::anyhow!("No supported package manager"))?;
                pm.install(pkg)?;
            }
            InstallMethod::Go(pkg) => {
                let output = run_cmd("go", &["install", pkg])?;
                if !output.status.success() {
                    bail!("go install {} failed", pkg);
                }
            }
            InstallMethod::Npm(pkg) => {
                let output = run_cmd("npm", &["install", "-g", pkg])?;
                if !output.status.success() {
                    bail!("npm install -g {} failed", pkg);
                }
            }
            InstallMethod::Pip(pkg) => {
                let pip = if command_exists("pip3") { "pip3" } else { "pip" };
                let output = run_cmd(pip, &["install", "--user", pkg])?;
                if !output.status.success() {
                    bail!("pip install {} failed", pkg);
                }
            }
        }

        if !self.is_satisfied() {
            bail!("Failed to install {}", self.binary);
        }

        Ok(())
    }
}

/// Resolve all requirements, installing missing ones
pub fn resolve_requirements(reqs: &[Requirement]) -> Result<()> {
    // Dedupe and preserve order
    let mut seen = HashSet::new();
    let unique: Vec<_> = reqs.iter().filter(|r| seen.insert((*r).clone())).collect();

    for req in unique {
        req.satisfy()?;
    }
    Ok(())
}

// =============================================================================
// PROVIDER TRAIT
// =============================================================================

/// Provider trait for checking and applying state
pub trait Provider {
    fn check(&self, state: &StateItem) -> Result<CheckResult>;
    fn apply(&self, state: &StateItem) -> Result<()>;
    fn name(&self) -> &'static str;

    /// Requirements that must be satisfied before this provider can run
    fn requires(&self) -> Vec<Requirement> {
        vec![]
    }
}

/// Registry of all providers
pub struct ProviderRegistry {
    providers: Vec<Box<dyn Provider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        let providers: Vec<Box<dyn Provider>> = vec![
            Box::new(package::OsProvider),
            Box::new(package::AptProvider),
            Box::new(package::PacmanProvider),
            Box::new(package::CargoProvider),
            Box::new(package::GoProvider),
            Box::new(package::NpmProvider),
            Box::new(package::PipProvider),
            Box::new(service::SystemdProvider),
            Box::new(file::CopyProvider),
            Box::new(file::SymlinkProvider),
            Box::new(file::EnsureLineProvider),
            Box::new(shell::AliasProvider),
            Box::new(shell::EnvProvider),
        ];

        Self { providers }
    }

    pub fn get(&self, kind: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| p.name() == kind)
            .map(|p| p.as_ref())
    }
}
