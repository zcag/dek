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
    /// Default selectors for `dek apply` — keys and @label refs
    #[serde(default)]
    pub defaults: Vec<String>,
    /// Test container settings
    #[serde(default)]
    pub test: Option<TestConfig>,
    /// Runtime variables set via std::env::set_var before any items run.
    /// Base vars are plain key=value, scoped vars are sub-tables keyed by
    /// selector (@label or config key).
    #[serde(default)]
    pub vars: Option<toml::Value>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct TestConfig {
    pub image: Option<String>,
    pub keep: Option<bool>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Config {
    /// Per-file metadata (name, description)
    pub meta: Option<ConfigMeta>,
    /// Proxy settings (applied to current process for all commands)
    pub proxy: Option<ProxyConfig>,
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
    /// Build artifacts (resolved before bake/deploy)
    #[serde(default)]
    pub artifact: Vec<ArtifactConfig>,
}

/// Proxy configuration
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ProxyConfig {
    /// HTTP proxy URL (sets http_proxy and HTTP_PROXY)
    pub http: Option<String>,
    /// HTTPS proxy URL (sets https_proxy and HTTPS_PROXY)
    pub https: Option<String>,
    /// No-proxy list (comma-separated, sets no_proxy and NO_PROXY)
    pub no_proxy: Option<String>,
    /// Persist to ~/.dek_env for future shell sessions (default: false)
    #[serde(default)]
    pub persist: bool,
}

/// Per-file metadata
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct ConfigMeta {
    /// Display name for this config
    pub name: Option<String>,
    /// Description shown in help
    pub description: Option<String>,
    /// Shell command — skip this config when it exits non-zero
    pub run_if: Option<String>,
    /// Labels for grouping (selectable via @label)
    #[serde(default)]
    pub labels: Vec<String>,
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
    pub pipx: Option<PackageList>,
    pub webi: Option<PackageList>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PackageList {
    pub items: Vec<String>,
    #[serde(default)]
    pub run_if: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    pub name: String,
    #[serde(default = "default_service_state")]
    pub state: String,
    #[serde(default)]
    pub enabled: bool,
    /// "system" (default) or "user"
    #[serde(default = "default_service_scope")]
    pub scope: String,
    #[serde(default)]
    pub run_if: Option<String>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub cache_key_cmd: Option<String>,
}

fn default_service_scope() -> String {
    "system".to_string()
}

fn default_service_state() -> String {
    "active".to_string()
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct FileConfig {
    pub copy: Option<HashMap<String, String>>,
    pub fetch: Option<HashMap<String, FetchTarget>>,
    pub symlink: Option<HashMap<String, String>>,
    pub ensure_line: Option<HashMap<String, Vec<String>>>,
    /// Structured line entries with original pattern matching
    #[serde(default)]
    pub line: Vec<FileLineConfig>,
}

/// Fetch target: either a plain path string or { path, ttl }
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum FetchTarget {
    Path(String),
    WithOptions { path: String, ttl: Option<String> },
}

impl FetchTarget {
    pub fn path(&self) -> &str {
        match self {
            Self::Path(p) => p,
            Self::WithOptions { path, .. } => path,
        }
    }

    pub fn ttl(&self) -> Option<&str> {
        match self {
            Self::Path(_) => None,
            Self::WithOptions { ttl, .. } => ttl.as_deref(),
        }
    }
}

/// Structured ensure_line with original pattern support
#[derive(Debug, Deserialize, Clone)]
pub struct FileLineConfig {
    pub path: String,
    pub line: String,
    /// Literal string to match an existing line
    pub original: Option<String>,
    /// Regex pattern to match an existing line
    pub original_regex: Option<String>,
    /// "replace" (default) or "below"
    #[serde(default)]
    pub mode: FileLineMode,
    #[serde(default)]
    pub run_if: Option<String>,
    #[serde(default)]
    pub cache_key: Option<String>,
    #[serde(default)]
    pub cache_key_cmd: Option<String>,
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
    /// Shell command to apply the state (accepts "cmd" as alias)
    #[serde(alias = "cmd")]
    pub apply: String,
    #[serde(default)]
    pub run_if: Option<String>,
    /// Skip if this value (supports $VAR) hasn't changed since last apply
    #[serde(default)]
    pub cache_key: Option<String>,
    /// Skip if this command's output hasn't changed since last apply
    #[serde(default)]
    pub cache_key_cmd: Option<String>,
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
    /// Allocate TTY for ssh (ssh -t), only with -t, rejects -r
    #[serde(default)]
    pub tty: bool,
    /// Prompt before running
    #[serde(default)]
    pub confirm: bool,
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
    /// Labels from [meta] labels
    pub labels: Vec<String>,
    /// Whether this is in optional/ (not applied by default)
    pub optional: bool,
    /// Whether this config runs by default (computed from meta.defaults or !optional)
    pub is_default: bool,
    /// Shell command condition from [meta] run_if
    pub run_if: Option<String>,
}

/// Inventory of remote hosts (loaded from inventory.ini)
#[derive(Debug, Default, Clone)]
pub struct Inventory {
    pub hosts: Vec<String>,
}

/// Build artifact (resolved before bake/deploy)
#[derive(Debug, Deserialize, Clone)]
pub struct ArtifactConfig {
    /// Display label
    pub name: Option<String>,
    /// Shell command to build the artifact
    pub build: String,
    /// Shell command — skip build if exits 0 (artifact is fresh)
    pub check: Option<String>,
    /// Paths to watch — skip build if hash unchanged (files or directories)
    #[serde(default)]
    pub watch: Vec<String>,
    /// Source path after build (relative to config dir)
    pub src: String,
    /// Destination path within config (included in tarball/bake)
    pub dest: String,
    /// Local dependencies needed before build (e.g. "maven:mvn", "apt.default-jdk:java")
    #[serde(default)]
    pub deps: Vec<String>,
}

/// Assertion to check before apply
#[derive(Debug, Deserialize, Clone)]
pub struct AssertConfig {
    /// Display label
    pub name: Option<String>,
    /// Shell command to run (exit 0 = pass)
    pub check: Option<String>,
    /// Shell command whose stdout lines are findings (0 lines = pass)
    pub foreach: Option<String>,
    /// Optional regex to match against stdout (check mode only)
    pub stdout: Option<String>,
    /// Optional regex to match against stderr (check mode only)
    pub stderr: Option<String>,
    /// Custom failure message
    pub message: Option<String>,
    #[serde(default)]
    pub run_if: Option<String>,
}
