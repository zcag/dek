mod types;

pub use types::*;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Load configs from main directory only (no optional/)
pub fn load<P: AsRef<Path>>(path: P) -> Result<Config> {
    load_filtered(path, None)
}

/// Load all configs from main + optional/ directories (no filtering)
pub fn load_all<P: AsRef<Path>>(path: P) -> Result<Config> {
    let path = path.as_ref();

    if crate::util::is_tar_gz(path) {
        let extracted = crate::util::extract_tar_gz(path)?;
        return load_all_from_dir(&extracted);
    }

    if path.is_dir() {
        load_all_from_dir(path)
    } else {
        load_file(path)
    }
}

/// Smart config loader for apply: resolves selectors (@labels + keys) and defaults
pub fn load_for_apply<P: AsRef<Path>>(path: P, selectors: &[String], meta: Option<&Meta>) -> Result<Config> {
    let path = path.as_ref();

    // No selectors and no defaults → main dir only (backward compat)
    let defaults = meta.map(|m| &m.defaults[..]).unwrap_or(&[]);
    if selectors.is_empty() && defaults.is_empty() {
        return load(path);
    }

    // Determine effective selectors
    let effective: &[String] = if selectors.is_empty() { defaults } else { selectors };

    let dir = if crate::util::is_tar_gz(path) {
        crate::util::extract_tar_gz(path)?
    } else if path.is_dir() {
        path.to_path_buf()
    } else {
        return load_file(path);
    };

    // Scan all entries (main + optional/)
    let entries = scan_config_entries(&dir)?;

    // Resolve selectors to keys
    let resolved_keys = resolve_selectors(effective, &entries);

    // Load only resolved keys from all dirs
    let keys: Vec<String> = resolved_keys.into_iter().collect();
    load_directory(&dir, Some(&keys))
}

/// Internal entry representing a scanned config file
struct ConfigEntry {
    key: String,
    labels: Vec<String>,
}

/// Scan main + optional/ dirs for config entries with their labels
fn scan_config_entries(dir: &Path) -> Result<Vec<ConfigEntry>> {
    let mut entries = Vec::new();
    scan_entries_from_dir(dir, &mut entries)?;
    let optional_dir = dir.join("optional");
    if optional_dir.is_dir() {
        scan_entries_from_dir(&optional_dir, &mut entries)?;
    }
    Ok(entries)
}

fn scan_entries_from_dir(dir: &Path, entries: &mut Vec<ConfigEntry>) -> Result<()> {
    for entry in get_config_entries(dir)? {
        let key = file_key(&entry.path());
        if key == "meta" {
            continue;
        }
        let config = load_file(&entry.path())?;
        let labels = config.meta.as_ref()
            .map(|m| m.labels.clone())
            .unwrap_or_default();
        entries.push(ConfigEntry { key, labels });
    }
    Ok(())
}

/// Resolve selectors (@label refs and plain keys) to a set of config keys
fn resolve_selectors(selectors: &[String], entries: &[ConfigEntry]) -> Vec<String> {
    let mut keys = Vec::new();
    for sel in selectors {
        if let Some(label) = sel.strip_prefix('@') {
            for entry in entries {
                if entry.labels.iter().any(|l| l == label) && !keys.contains(&entry.key) {
                    keys.push(entry.key.clone());
                }
            }
        } else if !keys.contains(sel) {
            keys.push(sel.clone());
        }
    }
    keys
}

/// Check if a config is a default based on meta.defaults
fn compute_is_default(key: &str, labels: &[String], optional: bool, meta: Option<&Meta>) -> bool {
    let defaults = match meta {
        Some(m) if !m.defaults.is_empty() => &m.defaults,
        _ => return !optional,
    };
    for sel in defaults {
        if let Some(label) = sel.strip_prefix('@') {
            if labels.iter().any(|l| l == label) {
                return true;
            }
        } else if sel == key {
            return true;
        }
    }
    false
}

fn load_filtered<P: AsRef<Path>>(path: P, filter_keys: Option<&[String]>) -> Result<Config> {
    let path = path.as_ref();

    if crate::util::is_tar_gz(path) {
        let extracted = crate::util::extract_tar_gz(path)?;
        return load_directory(&extracted, filter_keys);
    }

    if path.is_dir() {
        load_directory(path, filter_keys)
    } else {
        load_file(path)
    }
}

fn load_all_from_dir(dir: &Path) -> Result<Config> {
    let mut merged = Config::default();
    load_from_dir_skip_run_if(dir, &mut merged)?;
    let optional_dir = dir.join("optional");
    if optional_dir.is_dir() {
        load_from_dir_skip_run_if(&optional_dir, &mut merged)?;
    }
    Ok(merged)
}

/// List available config files with their metadata
pub fn list_configs<P: AsRef<Path>>(path: P, meta: Option<&Meta>) -> Result<Vec<ConfigInfo>> {
    let path = path.as_ref();

    if crate::util::is_tar_gz(path) {
        let extracted = crate::util::extract_tar_gz(path)?;
        return list_configs(&extracted, meta);
    }

    if !path.is_dir() {
        return Ok(vec![]);
    }

    let mut configs = Vec::new();
    list_configs_from_dir(path, false, meta, &mut configs)?;

    let optional_dir = path.join("optional");
    if optional_dir.is_dir() {
        list_configs_from_dir(&optional_dir, true, meta, &mut configs)?;
    }

    Ok(configs)
}

fn list_configs_from_dir(dir: &Path, optional: bool, meta: Option<&Meta>, configs: &mut Vec<ConfigInfo>) -> Result<()> {
    for entry in get_config_entries(dir)? {
        let key = file_key(&entry.path());
        if key == "meta" {
            continue;
        }
        let config = load_file(&entry.path())?;
        let cm = config.meta.as_ref();
        let name = cm
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| key.clone());
        let description = cm.and_then(|m| m.description.clone());
        let run_if = cm.and_then(|m| m.run_if.clone());
        let labels = cm.map(|m| m.labels.clone()).unwrap_or_default();
        let is_default = compute_is_default(&key, &labels, optional, meta);
        configs.push(ConfigInfo { key, name, description, labels, optional, is_default, run_if });
    }
    Ok(())
}

fn load_file(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    Ok(config)
}

fn load_directory(dir: &Path, filter_keys: Option<&[String]>) -> Result<Config> {
    let mut merged = Config::default();

    // Load from main directory
    load_from_dir(dir, filter_keys, &mut merged)?;

    // Also check optional/ when specific keys requested
    if filter_keys.is_some() {
        let optional_dir = dir.join("optional");
        if optional_dir.is_dir() {
            load_from_dir(&optional_dir, filter_keys, &mut merged)?;
        }
    }

    Ok(merged)
}

fn load_from_dir(dir: &Path, filter_keys: Option<&[String]>, merged: &mut Config) -> Result<()> {
    load_from_dir_inner(dir, filter_keys, merged, true)
}

fn load_from_dir_skip_run_if(dir: &Path, merged: &mut Config) -> Result<()> {
    load_from_dir_inner(dir, None, merged, false)
}

fn load_from_dir_inner(dir: &Path, filter_keys: Option<&[String]>, merged: &mut Config, eval_conditions: bool) -> Result<()> {
    for entry in get_config_entries(dir)? {
        let key = file_key(&entry.path());
        if key == "meta" {
            continue;
        }

        if let Some(keys) = filter_keys {
            if !keys.iter().any(|k| k == &key) {
                continue;
            }
        }

        let config = load_file(&entry.path())?;

        // Skip config if run_if condition fails
        if eval_conditions {
            if let Some(ref run_if) = config.meta.as_ref().and_then(|m| m.run_if.clone()) {
                if !eval_run_if(run_if) {
                    continue;
                }
            }
        }

        merge_config(merged, config);
    }
    Ok(())
}

pub fn eval_run_if(cmd: &str) -> bool {
    crate::util::shell_cmd(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn get_config_entries(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("Failed to read config directory: {}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "toml")
                .unwrap_or(false)
        })
        .collect();
    entries.sort_by_key(|e| e.path());
    Ok(entries)
}

/// Extract key from filename: "10-tools.toml" -> "tools", "tools.dek.toml" -> "tools"
fn file_key(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    // Strip .dek suffix (for *.dek.toml files used with nvim-dek)
    let stem = stem.strip_suffix(".dek").unwrap_or(&stem);
    // Strip numeric prefix like "10-", "00-"
    if let Some(pos) = stem.find('-') {
        if stem[..pos].chars().all(|c| c.is_ascii_digit()) {
            return stem[pos + 1..].to_string();
        }
    }
    stem.to_string()
}

fn merge_config(base: &mut Config, other: Config) {
    // Merge proxy (later config wins for each field)
    if let Some(proxy) = other.proxy {
        let base_proxy = base.proxy.get_or_insert_with(ProxyConfig::default);
        if proxy.http.is_some() {
            base_proxy.http = proxy.http;
        }
        if proxy.https.is_some() {
            base_proxy.https = proxy.https;
        }
        if proxy.no_proxy.is_some() {
            base_proxy.no_proxy = proxy.no_proxy;
        }
        if proxy.persist {
            base_proxy.persist = true;
        }
    }

    // Merge packages
    if let Some(pkg) = other.package {
        let base_pkg = base.package.get_or_insert_with(PackageConfig::default);
        merge_package_list(&mut base_pkg.os, pkg.os);
        merge_package_list(&mut base_pkg.apt, pkg.apt);
        merge_package_list(&mut base_pkg.pacman, pkg.pacman);
        merge_package_list(&mut base_pkg.cargo, pkg.cargo);
        merge_package_list(&mut base_pkg.go, pkg.go);
        merge_package_list(&mut base_pkg.npm, pkg.npm);
        merge_package_list(&mut base_pkg.pip, pkg.pip);
        merge_package_list(&mut base_pkg.pipx, pkg.pipx);
        merge_package_list(&mut base_pkg.webi, pkg.webi);
    }

    // Merge services
    base.service.extend(other.service);

    // Merge files
    if let Some(file) = other.file {
        let base_file = base.file.get_or_insert_with(FileConfig::default);
        if let Some(copy) = file.copy {
            base_file.copy.get_or_insert_with(Default::default).extend(copy);
        }
        if let Some(symlink) = file.symlink {
            base_file.symlink.get_or_insert_with(Default::default).extend(symlink);
        }
        if let Some(ensure_line) = file.ensure_line {
            base_file.ensure_line.get_or_insert_with(Default::default).extend(ensure_line);
        }
        base_file.line.extend(file.line);
        base_file.template.extend(file.template);
        base_file.vars.extend(file.vars);
    }

    // Merge aliases
    if let Some(aliases) = other.aliases {
        base.aliases.get_or_insert_with(Default::default).extend(aliases);
    }

    // Merge env
    if let Some(env) = other.env {
        base.env.get_or_insert_with(Default::default).extend(env);
    }

    // Override scalars
    if other.timezone.is_some() {
        base.timezone = other.timezone;
    }
    if other.hostname.is_some() {
        base.hostname = other.hostname;
    }

    // Merge commands
    base.command.extend(other.command);

    // Merge scripts
    if let Some(script) = other.script {
        base.script.get_or_insert_with(Default::default).extend(script);
    }

    // Merge run commands
    if let Some(run) = other.run {
        base.run.get_or_insert_with(Default::default).extend(run);
    }

    // Merge includes
    if let Some(include) = other.include {
        base.include.get_or_insert_with(Default::default).extend(include);
    }

    // Merge assertions
    base.assert.extend(other.assert);

    // Merge artifacts
    base.artifact.extend(other.artifact);

    // Merge state probes
    base.state.extend(other.state);
}

fn merge_package_list(base: &mut Option<PackageList>, other: Option<PackageList>) {
    if let Some(other_list) = other {
        base.get_or_insert_with(|| PackageList {
            items: vec![],
            run_if: None,
        })
        .items
        .extend(other_list.items);
    }
}

/// Apply proxy settings to current process environment
/// Call this early so all child commands inherit the proxy
pub fn apply_proxy(proxy: &ProxyConfig) {
    if let Some(ref url) = proxy.http {
        std::env::set_var("http_proxy", url);
        std::env::set_var("HTTP_PROXY", url);
    }
    if let Some(ref url) = proxy.https {
        std::env::set_var("https_proxy", url);
        std::env::set_var("HTTPS_PROXY", url);
    }
    if let Some(ref no_proxy) = proxy.no_proxy {
        std::env::set_var("no_proxy", no_proxy);
        std::env::set_var("NO_PROXY", no_proxy);
    }
}

/// Apply runtime vars from meta.toml to the current process environment.
/// Sets base vars first, then overlays vars matching active selectors.
pub fn apply_vars(vars: &toml::Value, selectors: &[String]) {
    let table = match vars.as_table() {
        Some(t) => t,
        None => return,
    };

    // Collect labels from selectors (strip @ prefix) and config keys
    let active_labels: Vec<&str> = selectors.iter()
        .filter(|s| s.starts_with('@'))
        .map(|s| &s[1..])
        .collect();
    let active_keys: Vec<&str> = selectors.iter()
        .filter(|s| !s.starts_with('@'))
        .map(|s| s.as_str())
        .collect();

    // Pass 1: set base vars (string values at top level)
    // Values are expanded so vars can reference earlier vars.
    for (key, val) in table {
        if let Some(s) = val.as_str() {
            std::env::set_var(key, crate::util::expand_vars(s));
        }
    }

    // Pass 2: overlay scoped vars from matching selectors
    for (key, val) in table {
        if !val.is_table() {
            continue;
        }
        let matches = if key.starts_with('@') {
            active_labels.contains(&&key[1..])
        } else {
            active_keys.contains(&key.as_str())
        };
        if !matches {
            continue;
        }
        if let Some(sub) = val.as_table() {
            for (k, v) in sub {
                if let Some(s) = v.as_str() {
                    std::env::set_var(k, crate::util::expand_vars(s));
                }
            }
        }
    }
}

/// Resolve config path - extracts tar.gz if needed, returns actual path for runner
pub fn resolve_path<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let path = path.as_ref();
    if crate::util::is_tar_gz(path) {
        crate::util::extract_tar_gz(path)
    } else {
        Ok(path.to_path_buf())
    }
}

pub fn find_default_config() -> Option<std::path::PathBuf> {
    // Current directory: dek.toml or dek/
    let file = Path::new("dek.toml");
    if file.exists() {
        return Some(file.to_path_buf());
    }

    let dir = Path::new("dek");
    if dir.is_dir() {
        return Some(dir.to_path_buf());
    }

    // User config: $XDG_CONFIG_HOME/dek or ~/.config/dek
    let config_home = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")));

    if let Some(config_dir) = config_home {
        let global = config_dir.join("dek");
        if global.is_dir() {
            return Some(global);
        }
    }

    None
}

/// Load meta.toml from config path (file's parent dir or directory itself)
pub fn load_meta<P: AsRef<Path>>(config_path: P) -> Option<Meta> {
    let path = config_path.as_ref();
    let dir = if path.is_dir() { path } else { path.parent()? };
    let meta_path = dir.join("meta.toml");

    if !meta_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&meta_path).ok()?;
    let mut meta: Meta = toml::from_str(&content).ok()?;

    // Load banner from banner.txt if present
    let banner_path = dir.join("banner.txt");
    if banner_path.exists() {
        if let Ok(banner) = fs::read_to_string(&banner_path) {
            meta.banner = Some(banner.trim_end().to_string());
        }
    }

    Some(meta)
}

/// Load inventory from config path
/// Checks meta.toml for custom inventory path, falls back to inventory.ini in config dir
pub fn load_inventory<P: AsRef<Path>>(config_path: P) -> Option<Inventory> {
    let path = config_path.as_ref();
    let dir = if path.is_dir() { path } else { path.parent()? };

    // Check meta.toml for custom inventory path
    let inventory_path = if let Some(meta) = load_meta(path) {
        if let Some(ref custom) = meta.inventory {
            let p = Path::new(custom);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                dir.join(custom)
            }
        } else {
            dir.join("inventory.ini")
        }
    } else {
        dir.join("inventory.ini")
    };

    if !inventory_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&inventory_path).ok()?;
    Some(parse_inventory_ini(&content))
}

/// Parse ansible-style inventory.ini
/// Ignores [group] headers, comments (;/#), and blank lines
fn parse_inventory_ini(content: &str) -> Inventory {
    let hosts: Vec<String> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with('[') && !l.starts_with(';') && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();
    Inventory { hosts }
}
