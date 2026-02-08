use crate::config::Config;
use crate::output;
use crate::providers::{resolve_requirements, ProviderRegistry, Requirement, StateItem};
use anyhow::{bail, Result};
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Apply,
    Check,
    Plan,
}

pub struct Runner {
    registry: ProviderRegistry,
    mode: Mode,
}

impl Runner {
    pub fn new(mode: Mode) -> Self {
        Self {
            registry: ProviderRegistry::new(),
            mode,
        }
    }

    pub fn run(&self, config: &Config, config_path: &Path) -> Result<()> {
        let base_dir = if config_path.is_file() {
            config_path.parent().unwrap_or(Path::new("."))
        } else {
            config_path
        };
        let items = collect_state_items(config, base_dir);
        self.run_items(&items)
    }

    pub fn run_items(&self, items: &[StateItem]) -> Result<()> {
        if items.is_empty() {
            println!("  No items");
            return Ok(());
        }

        match self.mode {
            Mode::Plan => self.plan_all(items),
            Mode::Check => self.check_all(items),
            Mode::Apply => self.apply_all(items),
        }
    }

    fn plan_all(&self, items: &[StateItem]) -> Result<()> {
        for item in items {
            output::print_plan_item(item);
        }
        output::print_plan_summary(items.len());
        Ok(())
    }

    fn check_all(&self, items: &[StateItem]) -> Result<()> {
        let start = Instant::now();
        let mut satisfied = 0;
        let mut missing = 0;

        for item in items {
            let provider = self
                .registry
                .get(&item.kind)
                .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", item.kind))?;

            let result = provider.check(item)?;
            output::print_check_result(item, &result);

            if result.is_satisfied() {
                satisfied += 1;
            } else {
                missing += 1;
            }
        }

        output::print_check_summary(items.len(), satisfied, missing, start.elapsed());
        Ok(())
    }

    fn apply_all(&self, items: &[StateItem]) -> Result<()> {
        let start = Instant::now();

        // Collect and resolve requirements from all providers
        let requirements = self.collect_requirements(items)?;
        if !requirements.is_empty() {
            output::print_resolving_requirements(requirements.len());
            resolve_requirements(&requirements)?;
        }

        let mut changed = 0;
        let mut failed = 0;

        for item in items {
            let provider = self
                .registry
                .get(&item.kind)
                .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", item.kind))?;

            let check = provider.check(item)?;

            if check.is_satisfied() {
                output::print_apply_skip(item);
                continue;
            }

            let pb = output::start_spinner(item);

            match provider.apply_live(item, &pb) {
                Ok(()) => {
                    output::finish_spinner_done(&pb, item);
                    changed += 1;
                }
                Err(e) => {
                    output::finish_spinner_fail(&pb, item, &e.to_string());
                    failed += 1;
                }
            }
        }

        output::print_summary(items.len(), changed, failed, start.elapsed());

        if failed > 0 {
            bail!("{} items failed to apply", failed);
        }

        Ok(())
    }

    fn collect_requirements(&self, items: &[StateItem]) -> Result<Vec<Requirement>> {
        let mut seen_kinds = HashSet::new();
        let mut requirements = Vec::new();

        for item in items {
            if seen_kinds.contains(&item.kind) {
                continue;
            }
            seen_kinds.insert(item.kind.clone());

            if let Some(provider) = self.registry.get(&item.kind) {
                requirements.extend(provider.requires());
            }
        }

        Ok(requirements)
    }
}

fn resolve_source_path(src: &str, base_dir: &Path) -> String {
    if src.starts_with('/') || src.starts_with('~') {
        src.to_string()
    } else {
        base_dir.join(src).to_string_lossy().to_string()
    }
}

fn collect_state_items(config: &Config, base_dir: &Path) -> Vec<StateItem> {
    let mut items = Vec::new();

    // Packages
    if let Some(ref pkg) = config.package {
        if let Some(ref os) = pkg.os {
            for item in &os.items {
                items.push(StateItem::new("package.os", item));
            }
        }
        if let Some(ref apt) = pkg.apt {
            for item in &apt.items {
                items.push(StateItem::new("package.apt", item));
            }
        }
        if let Some(ref pacman) = pkg.pacman {
            for item in &pacman.items {
                items.push(StateItem::new("package.pacman", item));
            }
        }
        if let Some(ref cargo) = pkg.cargo {
            for item in &cargo.items {
                items.push(StateItem::new("package.cargo", item));
            }
        }
        if let Some(ref go) = pkg.go {
            for item in &go.items {
                items.push(StateItem::new("package.go", item));
            }
        }
        if let Some(ref npm) = pkg.npm {
            for item in &npm.items {
                items.push(StateItem::new("package.npm", item));
            }
        }
        if let Some(ref pip) = pkg.pip {
            for item in &pip.items {
                items.push(StateItem::new("package.pip", item));
            }
        }
        if let Some(ref pipx) = pkg.pipx {
            for item in &pipx.items {
                items.push(StateItem::new("package.pipx", item));
            }
        }
        if let Some(ref webi) = pkg.webi {
            for item in &webi.items {
                items.push(StateItem::new("package.webi", item));
            }
        }
    }

    // Services
    for svc in &config.service {
        let value = format!("state={},enabled={},scope={}", svc.state, svc.enabled, svc.scope);
        items.push(StateItem::new("service", &svc.name).with_value(value));
    }

    // Files
    if let Some(ref file) = config.file {
        if let Some(ref copy) = file.copy {
            for (src, dst) in copy {
                let src_resolved = resolve_source_path(src, base_dir);
                items.push(StateItem::new("file.copy", &src_resolved).with_value(dst));
            }
        }
        if let Some(ref symlink) = file.symlink {
            for (src, dst) in symlink {
                let src_resolved = resolve_source_path(src, base_dir);
                items.push(StateItem::new("file.symlink", &src_resolved).with_value(dst));
            }
        }
        if let Some(ref ensure_line) = file.ensure_line {
            for (file, lines) in ensure_line {
                let value = lines.join("\n");
                items.push(StateItem::new("file.ensure_line", file).with_value(value));
            }
        }
        for entry in &file.line {
            use crate::config::FileLineMode;
            let mode = match entry.mode {
                FileLineMode::Replace => "replace",
                FileLineMode::Below => "below",
            };
            // Encode: line\x01original\x01mode\x01match_type
            let (original, match_type) = if let Some(ref re) = entry.original_regex {
                (re.as_str(), "regex")
            } else {
                (entry.original.as_deref().unwrap_or(""), "literal")
            };
            let value = format!("{}\x01{}\x01{}\x01{}", entry.line, original, mode, match_type);
            items.push(StateItem::new("file.line", &entry.path).with_value(value));
        }
    }

    // Aliases
    if let Some(ref aliases) = config.aliases {
        for (name, cmd) in aliases {
            items.push(StateItem::new("alias", name).with_value(cmd));
        }
    }

    // Env
    if let Some(ref env) = config.env {
        for (name, value) in env {
            items.push(StateItem::new("env", name).with_value(value));
        }
    }

    // Commands (check/apply)
    for cmd in &config.command {
        // Encode check and apply with null separator
        let value = format!("{}\x00{}", cmd.check, cmd.apply);
        items.push(StateItem::new("command", &cmd.name).with_value(value));
    }

    // Scripts
    if let Some(ref scripts) = config.script {
        for (name, path) in scripts {
            let script_path = base_dir.join(path);
            if let Ok(content) = std::fs::read_to_string(&script_path) {
                items.push(StateItem::new("script", name).with_value(content));
            }
        }
    }

    // Assertions
    for assertion in &config.assert {
        // Encode: check\0stdout_pattern\0stderr_pattern
        let stdout = assertion.stdout.as_deref().unwrap_or("");
        let stderr = assertion.stderr.as_deref().unwrap_or("");
        let value = format!("{}\x00{}", stdout, stderr);
        items.push(StateItem::new("assert", &assertion.check).with_value(value));
    }

    items
}

