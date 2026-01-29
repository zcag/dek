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
        if pkg.os.is_some() {
            base_pkg.os = pkg.os;
        }
        if pkg.apt.is_some() {
            base_pkg.apt = pkg.apt;
        }
        if pkg.pacman.is_some() {
            base_pkg.pacman = pkg.pacman;
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

    // Merge sections
    if let Some(sections) = other.sections {
        let base_sections = base.sections.get_or_insert_with(Default::default);
        for (name, section) in sections {
            // If section exists, merge; otherwise insert
            if let Some(base_section) = base_sections.get_mut(&name) {
                merge_section(base_section, section);
            } else {
                base_sections.insert(name, section);
            }
        }
    }
}

fn merge_section(base: &mut SectionConfig, other: SectionConfig) {
    // Description: override if set
    if other.description.is_some() {
        base.description = other.description;
    }

    // Merge packages
    if let Some(pkg) = other.package {
        let base_pkg = base.package.get_or_insert_with(PackageConfig::default);
        if pkg.os.is_some() { base_pkg.os = pkg.os; }
        if pkg.apt.is_some() { base_pkg.apt = pkg.apt; }
        if pkg.pacman.is_some() { base_pkg.pacman = pkg.pacman; }
        if pkg.cargo.is_some() { base_pkg.cargo = pkg.cargo; }
        if pkg.go.is_some() { base_pkg.go = pkg.go; }
        if pkg.npm.is_some() { base_pkg.npm = pkg.npm; }
        if pkg.pip.is_some() { base_pkg.pip = pkg.pip; }
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

    // Merge commands
    base.command.extend(other.command);

    // Merge scripts
    if let Some(script) = other.script {
        base.script.get_or_insert_with(Default::default).extend(script);
    }
}

/// Get list of available sections with descriptions
pub fn list_sections(config: &Config) -> Vec<(&str, Option<&str>)> {
    config.sections.as_ref().map_or(vec![], |sections| {
        let mut list: Vec<_> = sections
            .iter()
            .map(|(name, sec)| (name.as_str(), sec.description.as_deref()))
            .collect();
        list.sort_by_key(|(name, _)| *name);
        list
    })
}

/// Apply sections to a base config, returning merged config
pub fn apply_sections(config: &Config, section_names: &[String]) -> Config {
    let mut result = Config {
        package: config.package.clone(),
        service: config.service.clone(),
        file: config.file.clone(),
        aliases: config.aliases.clone(),
        env: config.env.clone(),
        timezone: config.timezone.clone(),
        hostname: config.hostname.clone(),
        command: config.command.clone(),
        script: config.script.clone(),
        run: config.run.clone(),
        sections: None, // Don't include sections in result
    };

    if let Some(sections) = &config.sections {
        for name in section_names {
            if let Some(section) = sections.get(name) {
                apply_section_to_config(&mut result, section);
            }
        }
    }

    result
}

fn apply_section_to_config(config: &mut Config, section: &SectionConfig) {
    // Merge packages
    if let Some(pkg) = &section.package {
        let base_pkg = config.package.get_or_insert_with(PackageConfig::default);
        if let Some(ref os) = pkg.os {
            base_pkg.os.get_or_insert_with(|| PackageList { items: vec![] }).items.extend(os.items.clone());
        }
        if let Some(ref apt) = pkg.apt {
            base_pkg.apt.get_or_insert_with(|| PackageList { items: vec![] }).items.extend(apt.items.clone());
        }
        if let Some(ref pacman) = pkg.pacman {
            base_pkg.pacman.get_or_insert_with(|| PackageList { items: vec![] }).items.extend(pacman.items.clone());
        }
        if let Some(ref cargo) = pkg.cargo {
            base_pkg.cargo.get_or_insert_with(|| PackageList { items: vec![] }).items.extend(cargo.items.clone());
        }
        if let Some(ref go) = pkg.go {
            base_pkg.go.get_or_insert_with(|| PackageList { items: vec![] }).items.extend(go.items.clone());
        }
        if let Some(ref npm) = pkg.npm {
            base_pkg.npm.get_or_insert_with(|| PackageList { items: vec![] }).items.extend(npm.items.clone());
        }
        if let Some(ref pip) = pkg.pip {
            base_pkg.pip.get_or_insert_with(|| PackageList { items: vec![] }).items.extend(pip.items.clone());
        }
    }

    // Merge services
    config.service.extend(section.service.clone());

    // Merge files
    if let Some(file) = &section.file {
        let base_file = config.file.get_or_insert_with(FileConfig::default);
        if let Some(ref copy) = file.copy {
            base_file.copy.get_or_insert_with(Default::default).extend(copy.clone());
        }
        if let Some(ref symlink) = file.symlink {
            base_file.symlink.get_or_insert_with(Default::default).extend(symlink.clone());
        }
        if let Some(ref ensure_line) = file.ensure_line {
            base_file.ensure_line.get_or_insert_with(Default::default).extend(ensure_line.clone());
        }
    }

    // Merge aliases
    if let Some(ref aliases) = section.aliases {
        config.aliases.get_or_insert_with(Default::default).extend(aliases.clone());
    }

    // Merge env
    if let Some(ref env) = section.env {
        config.env.get_or_insert_with(Default::default).extend(env.clone());
    }

    // Merge commands
    config.command.extend(section.command.clone());

    // Merge scripts
    if let Some(ref script) = section.script {
        config.script.get_or_insert_with(Default::default).extend(script.clone());
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
