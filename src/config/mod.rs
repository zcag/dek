mod types;

pub use types::*;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Load all configs from path (merges all .toml files if directory)
pub fn load<P: AsRef<Path>>(path: P) -> Result<Config> {
    let path = path.as_ref();

    // Handle tar.gz - extract to cache first
    if is_tar_gz(path) {
        let extracted = extract_tar_gz(path)?;
        return load_directory(&extracted, None);
    }

    if path.is_dir() {
        load_directory(path, None)
    } else {
        load_file(path)
    }
}

/// Load specific configs by key (e.g., "tools", "config")
/// Key is derived from filename: "10-tools.toml" -> "tools"
/// Also searches optional/ subdirectory
pub fn load_selected<P: AsRef<Path>>(path: P, keys: &[String]) -> Result<Config> {
    let path = path.as_ref();

    // Handle tar.gz - extract to cache first
    if is_tar_gz(path) {
        let extracted = extract_tar_gz(path)?;
        return load_directory_with_optional(&extracted, Some(keys));
    }

    if path.is_dir() {
        load_directory_with_optional(path, Some(keys))
    } else {
        // Single file - just load it
        load_file(path)
    }
}

/// List available config files with their metadata
pub fn list_configs<P: AsRef<Path>>(path: P) -> Result<Vec<ConfigInfo>> {
    let path = path.as_ref();

    // Handle tar.gz - extract to cache first
    if is_tar_gz(path) {
        let extracted = extract_tar_gz(path)?;
        return list_configs(&extracted);
    }

    if !path.is_dir() {
        return Ok(vec![]);
    }

    let mut configs = Vec::new();

    // Main configs
    for entry in get_config_entries(path)? {
        let key = file_key(&entry.path());
        if key == "meta" {
            continue;
        }
        let config = load_file(&entry.path())?;
        let name = config
            .meta
            .as_ref()
            .and_then(|m| m.name.clone())
            .unwrap_or_else(|| key.clone());
        let description = config.meta.as_ref().and_then(|m| m.description.clone());
        configs.push(ConfigInfo { key, name, description, optional: false });
    }

    // Optional configs
    let optional_dir = path.join("optional");
    if optional_dir.is_dir() {
        for entry in get_config_entries(&optional_dir)? {
            let key = file_key(&entry.path());
            if key == "meta" {
                continue;
            }
            let config = load_file(&entry.path())?;
            let name = config
                .meta
                .as_ref()
                .and_then(|m| m.name.clone())
                .unwrap_or_else(|| key.clone());
            let description = config.meta.as_ref().and_then(|m| m.description.clone());
            configs.push(ConfigInfo { key, name, description, optional: true });
        }
    }

    Ok(configs)
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

    for entry in get_config_entries(dir)? {
        let key = file_key(&entry.path());
        if key == "meta" {
            continue; // Skip meta.toml
        }

        // Filter by keys if specified
        if let Some(keys) = filter_keys {
            if !keys.iter().any(|k| k == &key) {
                continue;
            }
        }

        let config = load_file(&entry.path())?;
        merge_config(&mut merged, config);
    }

    Ok(merged)
}

/// Load configs, including optional/ when keys are specified
fn load_directory_with_optional(dir: &Path, filter_keys: Option<&[String]>) -> Result<Config> {
    let mut merged = Config::default();

    // Load from main directory
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
        merge_config(&mut merged, config);
    }

    // Also check optional/ when specific keys requested
    if let Some(keys) = filter_keys {
        let optional_dir = dir.join("optional");
        if optional_dir.is_dir() {
            for entry in get_config_entries(&optional_dir)? {
                let key = file_key(&entry.path());
                if key == "meta" {
                    continue;
                }

                if !keys.iter().any(|k| k == &key) {
                    continue;
                }

                let config = load_file(&entry.path())?;
                merge_config(&mut merged, config);
            }
        }
    }

    Ok(merged)
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

/// Extract key from filename: "10-tools.toml" -> "tools", "config.toml" -> "config"
fn file_key(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    // Strip numeric prefix like "10-", "00-"
    if let Some(pos) = stem.find('-') {
        if stem[..pos].chars().all(|c| c.is_ascii_digit()) {
            return stem[pos + 1..].to_string();
        }
    }
    stem.to_string()
}

fn merge_config(base: &mut Config, other: Config) {
    // Merge packages
    if let Some(pkg) = other.package {
        let base_pkg = base.package.get_or_insert_with(PackageConfig::default);
        merge_package_list(&mut base_pkg.os, pkg.os);
        merge_package_list(&mut base_pkg.apt, pkg.apt);
        merge_package_list(&mut base_pkg.pacman, pkg.pacman);
        merge_package_list(&mut base_pkg.yum, pkg.yum);
        merge_package_list(&mut base_pkg.cargo, pkg.cargo);
        merge_package_list(&mut base_pkg.go, pkg.go);
        merge_package_list(&mut base_pkg.npm, pkg.npm);
        merge_package_list(&mut base_pkg.pip, pkg.pip);
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
}

fn merge_package_list(base: &mut Option<PackageList>, other: Option<PackageList>) {
    if let Some(other_list) = other {
        base.get_or_insert_with(|| PackageList { items: vec![] })
            .items
            .extend(other_list.items);
    }
}

fn is_tar_gz(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".tar.gz") || name.ends_with(".tgz")
}

fn extract_tar_gz(path: &Path) -> Result<PathBuf> {
    let data = fs::read(path).with_context(|| format!("Failed to read: {}", path.display()))?;

    // Hash for cache key
    let hash = format!("{:x}", md5::compute(&data));
    let cache_dir = PathBuf::from(format!("/tmp/dek-{}", hash));

    // Already extracted?
    if cache_dir.exists() {
        return Ok(cache_dir);
    }

    // Extract
    let decoder = flate2::read::GzDecoder::new(&data[..]);
    let mut archive = tar::Archive::new(decoder);
    fs::create_dir_all(&cache_dir)
        .with_context(|| format!("Failed to create cache dir: {}", cache_dir.display()))?;
    archive
        .unpack(&cache_dir)
        .with_context(|| format!("Failed to extract: {}", path.display()))?;

    Ok(cache_dir)
}

pub fn find_default_config() -> Option<std::path::PathBuf> {
    let file = Path::new("dek.toml");
    if file.exists() {
        return Some(file.to_path_buf());
    }

    let dir = Path::new("dek");
    if dir.is_dir() {
        return Some(dir.to_path_buf());
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
    toml::from_str(&content).ok()
}
