use serde::Deserialize;
use std::collections::HashMap;

/// Metadata for baked binaries (loaded from meta.toml)
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Meta {
    /// Name shown in help/banner (defaults to binary name)
    pub name: Option<String>,
    /// Description for --help
    pub description: Option<String>,
    /// Version string
    pub version: Option<String>,
    /// Banner text shown on apply
    pub banner: Option<String>,
    /// Custom inventory path (absolute or relative to meta.toml)
    pub inventory: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Config {
    /// Per-file metadata (name, description)
    pub meta: Option<ConfigMeta>,
    pub package: Option<PackageConfig>,
    #[serde(default)]
    pub service: Vec<ServiceConfig>,
    pub file: Option<FileConfig>,
    #[serde(rename = "alias")]
    pub aliases: Option<HashMap<String, String>>,
    pub env: Option<HashMap<String, String>>,
    pub timezone: Option<String>,
    pub hostname: Option<String>,
    /// Custom commands with check/apply
    #[serde(default)]
    pub command: Vec<CommandConfig>,
    /// Scripts to install to ~/.local/bin
    pub script: Option<HashMap<String, String>>,
    /// Runnable commands (dek run <name>)
    pub run: Option<HashMap<String, RunConfig>>,
    /// External files to include (source path → config-relative dest)
    pub include: Option<HashMap<String, String>>,
    /// Assertions to check before apply
    #[serde(default)]
    pub assert: Vec<AssertConfig>,
}

/// Per-file metadata
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ConfigMeta {
    /// Display name for this config
    pub name: Option<String>,
    /// Description shown in help
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct PackageConfig {
    pub os: Option<PackageList>,
    pub apt: Option<PackageList>,
    pub pacman: Option<PackageList>,
    pub cargo: Option<PackageList>,
    pub go: Option<PackageList>,
    pub npm: Option<PackageList>,
    pub pip: Option<PackageList>,
    pub webi: Option<PackageList>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PackageList {
    pub items: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    pub name: String,
    #[serde(default = "default_service_state")]
    pub state: String,
    #[serde(default)]
    pub enabled: bool,
}

fn default_service_state() -> String {
    "active".to_string()
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct FileConfig {
    pub copy: Option<HashMap<String, String>>,
    pub symlink: Option<HashMap<String, String>>,
    pub ensure_line: Option<HashMap<String, Vec<String>>>,
    /// Structured line entries with original pattern matching
    #[serde(default)]
    pub line: Vec<FileLineConfig>,
}

/// Structured ensure_line with original pattern support
#[derive(Debug, Deserialize, Clone)]
pub struct FileLineConfig {
    pub path: String,
    pub line: String,
    /// Regex pattern to match an existing line
    pub original: Option<String>,
    /// "replace" (default) or "below"
    #[serde(default)]
    pub mode: FileLineMode,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum FileLineMode {
    #[default]
    Replace,
    Below,
}

/// Custom command with check/apply scripts
#[derive(Debug, Deserialize, Clone)]
pub struct CommandConfig {
    pub name: String,
    /// Shell command that returns 0 if satisfied
    pub check: String,
    /// Shell command to apply the state
    pub apply: String,
}

/// Runnable command (dek run <name>)
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct RunConfig {
    /// Description for completions/help
    pub description: Option<String>,
    /// Dependencies in provider.package format (e.g., "os.fzf")
    #[serde(default)]
    pub deps: Vec<String>,
    /// Inline shell command
    pub cmd: Option<String>,
    /// Script file path (relative to config dir)
    pub script: Option<String>,
    /// Inline provider config to apply
    pub file: Option<FileConfig>,
    /// Run locally before remote deployment (with --remotes)
    #[serde(default)]
    pub local: bool,
}

/// Info about a config file (for listing)
#[derive(Debug, Clone)]
pub struct ConfigInfo {
    /// File stem (e.g., "tools" from "10-tools.toml")
    pub key: String,
    /// Display name from meta or derived from filename
    pub name: String,
    /// Description from meta
    pub description: Option<String>,
    /// Whether this is in optional/ (not applied by default)
    pub optional: bool,
}

/// Inventory of remote hosts (loaded from inventory.ini)
#[derive(Debug, Default, Clone)]
pub struct Inventory {
    pub hosts: Vec<String>,
}

/// Assertion to check before apply
#[derive(Debug, Deserialize, Clone)]
pub struct AssertConfig {
    /// Shell command to run
    pub check: String,
    /// Optional regex to match against stdout
    pub stdout: Option<String>,
    /// Optional regex to match against stderr
    pub stderr: Option<String>,
}
