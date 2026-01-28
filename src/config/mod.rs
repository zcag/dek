mod types;

pub use types::*;

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn load<P: AsRef<Path>>(path: P) -> Result<Config> {
    let path = path.as_ref();

    if path.is_dir() {
        load_directory(path)
    } else {
        load_file(path)
    }
}

fn load_file(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    Ok(config)
}

fn load_directory(dir: &Path) -> Result<Config> {
    let mut merged = Config::default();

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

    for entry in entries {
        let config = load_file(&entry.path())?;
        merge_config(&mut merged, config);
    }

    Ok(merged)
}

fn merge_config(base: &mut Config, other: Config) {
    // Merge packages
    if let Some(pkg) = other.package {
        let base_pkg = base.package.get_or_insert_with(PackageConfig::default);
        if pkg.apt.is_some() {
            base_pkg.apt = pkg.apt;
        }
        if pkg.cargo.is_some() {
            base_pkg.cargo = pkg.cargo;
        }
        if pkg.go.is_some() {
            base_pkg.go = pkg.go;
        }
        if pkg.npm.is_some() {
            base_pkg.npm = pkg.npm;
        }
        if pkg.pip.is_some() {
            base_pkg.pip = pkg.pip;
        }
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
