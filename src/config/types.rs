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
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Config {
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
    /// Sections for grouping (dek apply <section>)
    pub sections: Option<HashMap<String, SectionConfig>>,
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
}

/// Section containing config items (mirrors main config structure)
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct SectionConfig {
    /// Description for completions/help
    pub description: Option<String>,
    pub package: Option<PackageConfig>,
    #[serde(default)]
    pub service: Vec<ServiceConfig>,
    pub file: Option<FileConfig>,
    #[serde(rename = "alias")]
    pub aliases: Option<HashMap<String, String>>,
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub command: Vec<CommandConfig>,
    pub script: Option<HashMap<String, String>>,
}
