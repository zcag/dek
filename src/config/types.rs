use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Default)]
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
}

#[derive(Debug, Deserialize, Default)]
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

#[derive(Debug, Deserialize)]
pub struct PackageList {
    pub items: Vec<String>,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct FileConfig {
    pub copy: Option<HashMap<String, String>>,
    pub symlink: Option<HashMap<String, String>>,
    pub ensure_line: Option<HashMap<String, Vec<String>>>,
}
