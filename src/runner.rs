use crate::config::Config;
use crate::output;
use crate::providers::{resolve_requirements, ProviderRegistry, Requirement, StateItem};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
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
        // Apply proxy settings early so all commands inherit them
        if let Some(ref proxy) = config.proxy {
            crate::config::apply_proxy(proxy);
        }

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
            if !should_run(item) {
                output::print_skip_run_if(item);
                continue;
            }
            output::print_plan_item(item);
        }
        output::print_plan_summary(items.len());
        Ok(())
    }

    fn check_all(&self, items: &[StateItem]) -> Result<()> {
        let start = Instant::now();
        let mut satisfied = 0;
        let mut missing = 0;

        let mut skipped = 0;

        for item in items {
            if !should_run(item) {
                output::print_skip_run_if(item);
                skipped += 1;
                continue;
            }

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

        output::print_check_summary(
            items.len() - skipped,
            satisfied,
            missing,
            start.elapsed(),
        );
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

        // Pre-authenticate sudo once if any provider will need it
        if self.any_needs_sudo(items) {
            Command::new("sudo")
                .arg("-v")
                .status()
                .context("Failed to authenticate sudo")?;
        }

        let mut changed = 0;
        let mut failed = 0;
        let mut skipped = 0;
        let mut issues = 0;

        for item in items {
            if !should_run(item) {
                output::print_skip_run_if(item);
                skipped += 1;
                continue;
            }

            let provider = self
                .registry
                .get(&item.kind)
                .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", item.kind))?;

            let check = provider.check(item)?;

            if check.is_satisfied() {
                // Cache key present and stale → re-apply (config changed).
                // No cache key, or cache fresh → skip.
                if !item.cache_key.is_some() || is_cache_fresh(item) {
                    output::print_apply_skip(item);
                    continue;
                }
                // fall through to apply
            }

            // Check failed — if cache is fresh, something was removed/changed
            // externally. Apply will run and cache updates on success.

            if provider.is_check_only() {
                output::print_check_result(item, &check);
                issues += 1;
                continue;
            }

            let pb = output::start_spinner(item);

            match provider.apply_live(item, &pb) {
                Ok(()) => {
                    update_cache(item);
                    output::finish_spinner_done(&pb, item);
                    changed += 1;
                }
                Err(e) => {
                    output::finish_spinner_fail(&pb, item, &e.to_string());
                    failed += 1;
                }
            }
        }

        output::print_summary(items.len() - skipped, changed, failed, issues, start.elapsed());

        if failed > 0 {
            bail!("{} items failed to apply", failed);
        }

        Ok(())
    }

    fn any_needs_sudo(&self, items: &[StateItem]) -> bool {
        if unsafe { libc::geteuid() } == 0 {
            return false;
        }
        items.iter().any(|item| {
            self.registry
                .get(&item.kind)
                .map(|p| p.needs_sudo())
                .unwrap_or(false)
        })
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

/// Returns the cache state item ID for a given item
fn cache_item_id(item: &StateItem) -> String {
    format!("{}:{}", item.kind, item.key)
}

/// Check if cache_key is fresh (value unchanged since last apply).
/// Returns true if the item should be skipped.
fn is_cache_fresh(item: &StateItem) -> bool {
    let key = match &item.cache_key {
        Some(k) => k,
        None => return false,
    };
    let id = cache_item_id(item);
    crate::cache::get_state(&id).as_deref() == Some(key.as_str())
}

/// Store cache_key value after successful apply
fn update_cache(item: &StateItem) {
    if let Some(ref key) = item.cache_key {
        crate::cache::set_state(&cache_item_id(item), key);
    }
}

fn should_run(item: &StateItem) -> bool {
    match &item.run_if {
        None => true,
        Some(cmd) => Command::new("sh")
            .args(["-c", cmd])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
    }
}

fn ev(s: &str) -> String {
    crate::util::expand_vars(s)
}

fn resolve_source_path(src: &str, base_dir: &Path) -> String {
    let expanded = ev(src);
    if expanded.starts_with('/') || expanded.starts_with('~') {
        expanded
    } else {
        base_dir.join(&expanded).to_string_lossy().to_string()
    }
}

/// Load vars files (YAML or TOML) and return merged key→Value map.
/// Later files override earlier ones.
fn load_vars_files(paths: &[String], base_dir: &Path) -> HashMap<String, minijinja::Value> {
    let mut merged = HashMap::new();
    for path in paths {
        let resolved = resolve_source_path(path, base_dir);
        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let val: Option<serde_json::Value> = if resolved.ends_with(".yaml")
            || resolved.ends_with(".yml")
        {
            serde_yml::from_str(&content).ok()
        } else {
            // TOML
            toml::from_str::<toml::Value>(&content)
                .ok()
                .and_then(|v| serde_json::to_value(v).ok())
        };
        if let Some(serde_json::Value::Object(map)) = val {
            for (k, v) in map {
                merged.insert(k, minijinja::Value::from_serialize(&v));
            }
        }
    }
    merged
}

fn collect_state_items(config: &Config, base_dir: &Path) -> Vec<StateItem> {
    let mut items = Vec::new();

    // Packages
    if let Some(ref pkg) = config.package {
        if let Some(ref os) = pkg.os {
            for item in &os.items {
                items.push(StateItem::new("package.os", item).with_run_if(os.run_if.clone()));
            }
        }
        if let Some(ref apt) = pkg.apt {
            for item in &apt.items {
                items.push(StateItem::new("package.apt", item).with_run_if(apt.run_if.clone()));
            }
        }
        if let Some(ref pacman) = pkg.pacman {
            for item in &pacman.items {
                items.push(
                    StateItem::new("package.pacman", item).with_run_if(pacman.run_if.clone()),
                );
            }
        }
        if let Some(ref cargo) = pkg.cargo {
            for item in &cargo.items {
                items.push(
                    StateItem::new("package.cargo", item).with_run_if(cargo.run_if.clone()),
                );
            }
        }
        if let Some(ref go) = pkg.go {
            for item in &go.items {
                items.push(StateItem::new("package.go", item).with_run_if(go.run_if.clone()));
            }
        }
        if let Some(ref npm) = pkg.npm {
            for item in &npm.items {
                items.push(StateItem::new("package.npm", item).with_run_if(npm.run_if.clone()));
            }
        }
        if let Some(ref pip) = pkg.pip {
            for item in &pip.items {
                items.push(StateItem::new("package.pip", item).with_run_if(pip.run_if.clone()));
            }
        }
        if let Some(ref pipx) = pkg.pipx {
            for item in &pipx.items {
                items.push(
                    StateItem::new("package.pipx", item).with_run_if(pipx.run_if.clone()),
                );
            }
        }
        if let Some(ref webi) = pkg.webi {
            for item in &webi.items {
                items.push(StateItem::new("package.webi", item).with_run_if(webi.run_if.clone()));
            }
        }
    }

    // Services
    for svc in &config.service {
        let value = format!("state={},enabled={},scope={}", svc.state, svc.enabled, svc.scope);
        items.push(
            StateItem::new("service", &svc.name)
                .with_value(value)
                .with_run_if(svc.run_if.clone())
                .with_cache_key(svc.cache_key.clone(), svc.cache_key_cmd.clone()),
        );
    }

    // Files
    if let Some(ref file) = config.file {
        if let Some(ref copy) = file.copy {
            for (src, dst) in copy {
                let src_resolved = resolve_source_path(src, base_dir);
                items.push(StateItem::new("file.copy", &src_resolved).with_value(ev(dst)));
            }
        }
        if let Some(ref fetch) = file.fetch {
            for (url, target) in fetch {
                let value = format!("{}\x00{}", ev(target.path()), target.ttl().unwrap_or(""));
                items.push(StateItem::new("file.fetch", ev(url)).with_value(value));
            }
        }
        if let Some(ref symlink) = file.symlink {
            for (src, dst) in symlink {
                let src_resolved = resolve_source_path(src, base_dir);
                items.push(StateItem::new("file.symlink", &src_resolved).with_value(ev(dst)));
            }
        }
        if let Some(ref ensure_line) = file.ensure_line {
            for (file, lines) in ensure_line {
                let value = lines.join("\n");
                items.push(StateItem::new("file.ensure_line", ev(file)).with_value(value));
            }
        }
        for entry in &file.line {
            use crate::config::FileLineMode;
            let mode = match entry.mode {
                FileLineMode::Replace => "replace",
                FileLineMode::Below => "below",
            };
            let (original, match_type) = if let Some(ref re) = entry.original_regex {
                (re.as_str(), "regex")
            } else {
                (entry.original.as_deref().unwrap_or(""), "literal")
            };
            let value = format!("{}\x01{}\x01{}\x01{}", entry.line, original, mode, match_type);
            items.push(
                StateItem::new("file.line", ev(&entry.path))
                    .with_value(value)
                    .with_run_if(entry.run_if.clone())
                    .with_cache_key(entry.cache_key.clone(), entry.cache_key_cmd.clone()),
            );
        }

        // Templates
        if !file.template.is_empty() {
            // Collect all needed state names
            let needed: Vec<String> = file
                .template
                .iter()
                .flat_map(|t| t.states.iter().cloned())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();

            // Evaluate states
            let state_results = if needed.is_empty() {
                HashMap::new()
            } else {
                crate::state::eval_states(&config.state, &needed).unwrap_or_default()
            };

            // Load shared vars files
            let shared_vars = load_vars_files(&file.vars, base_dir);

            // Build built-in context values
            let hostname = hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_default();
            let user = std::env::var("USER").unwrap_or_default();
            let os = std::env::consts::OS.to_string();
            let arch = std::env::consts::ARCH.to_string();

            for tmpl in &file.template {
                let src_path = resolve_source_path(&tmpl.src, base_dir);
                let src_content = match std::fs::read_to_string(&src_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                // Build context: built-ins first
                let mut ctx = HashMap::new();
                ctx.insert(
                    "hostname".to_string(),
                    minijinja::Value::from(hostname.clone()),
                );
                ctx.insert("user".to_string(), minijinja::Value::from(user.clone()));
                ctx.insert("os".to_string(), minijinja::Value::from(os.clone()));
                ctx.insert("arch".to_string(), minijinja::Value::from(arch.clone()));

                // Layer shared vars
                for (k, v) in &shared_vars {
                    ctx.insert(k.clone(), v.clone());
                }

                // Layer per-template vars (overrides shared)
                if !tmpl.vars.is_empty() {
                    let tmpl_vars = load_vars_files(&tmpl.vars, base_dir);
                    for (k, v) in tmpl_vars {
                        ctx.insert(k, v);
                    }
                }

                // Add state results
                for (name, result) in &state_results {
                    if !tmpl.states.contains(name) {
                        continue;
                    }
                    let mut map = HashMap::new();
                    map.insert("raw".to_string(), result.raw.clone());
                    if let Some(ref orig) = result.original {
                        map.insert("original".to_string(), orig.clone());
                    }
                    for (k, v) in &result.templates {
                        map.insert(k.clone(), v.clone());
                    }
                    ctx.insert(
                        name.clone(),
                        minijinja::Value::from_serialize(&map),
                    );
                }

                // Render
                let mut env = minijinja::Environment::new();
                env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
                env.add_template("_tmpl", &src_content).ok();
                let rendered = env
                    .get_template("_tmpl")
                    .and_then(|t| t.render(&ctx))
                    .unwrap_or_default();

                let dest = ev(&tmpl.dest);
                items.push(
                    StateItem::new("file.template", &dest).with_value(rendered),
                );
            }
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
            items.push(StateItem::new("env", name).with_value(ev(value)));
        }
    }

    // Proxy persistence (adds to env items if persist: true)
    if let Some(ref proxy) = config.proxy {
        if proxy.persist {
            if let Some(ref url) = proxy.http {
                items.push(StateItem::new("env", "http_proxy").with_value(url));
                items.push(StateItem::new("env", "HTTP_PROXY").with_value(url));
            }
            if let Some(ref url) = proxy.https {
                items.push(StateItem::new("env", "https_proxy").with_value(url));
                items.push(StateItem::new("env", "HTTPS_PROXY").with_value(url));
            }
            if let Some(ref no_proxy) = proxy.no_proxy {
                items.push(StateItem::new("env", "no_proxy").with_value(no_proxy));
                items.push(StateItem::new("env", "NO_PROXY").with_value(no_proxy));
            }
        }
    }

    // Commands (check/apply)
    for cmd in &config.command {
        // Encode check and apply with null separator
        let value = format!("{}\x00{}", cmd.check, cmd.apply);
        items.push(
            StateItem::new("command", &cmd.name)
                .with_value(value)
                .with_run_if(cmd.run_if.clone())
                .with_cache_key(cmd.cache_key.clone(), cmd.cache_key_cmd.clone()),
        );
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
        let (cmd, mode) = if let Some(ref foreach) = assertion.foreach {
            (foreach.as_str(), "foreach")
        } else if let Some(ref check) = assertion.check {
            (check.as_str(), "check")
        } else {
            continue; // skip invalid: neither check nor foreach
        };
        let key = assertion.name.as_deref().unwrap_or(cmd);
        let stdout = assertion.stdout.as_deref().unwrap_or("");
        let stderr = assertion.stderr.as_deref().unwrap_or("");
        let message = assertion.message.as_deref().unwrap_or("");
        let value = format!("{}\x00{}\x00{}\x00{}\x00{}", cmd, mode, stdout, stderr, message);
        items.push(
            StateItem::new("assert", key)
                .with_value(value)
                .with_run_if(assertion.run_if.clone()),
        );
    }

    items
}

