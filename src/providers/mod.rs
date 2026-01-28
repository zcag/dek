pub mod file;
pub mod package;
pub mod service;
pub mod shell;

use anyhow::Result;
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
    /// Provider kind (e.g., "package.apt", "service", "file.copy")
    pub kind: String,
    /// Item identifier
    pub key: String,
    /// Provider-specific data (serialized)
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

/// Provider trait for checking and applying state
pub trait Provider {
    /// Check if the state is already satisfied
    fn check(&self, state: &StateItem) -> Result<CheckResult>;

    /// Apply the state
    fn apply(&self, state: &StateItem) -> Result<()>;

    /// Provider name for display
    fn name(&self) -> &'static str;
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
            Box::new(package::AptProvider),
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
