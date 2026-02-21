use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};

use crate::config;
use crate::config::StateConfig;

pub struct StateResult {
    pub name: String,
    pub original: Option<String>,
    pub raw: String,
    /// Parsed JSON when `json: true` on the state config
    pub raw_parsed: Option<serde_json::Value>,
    pub templates: HashMap<String, String>,
}

impl StateResult {
    fn get_variant(&self, variant: Option<&str>) -> Option<&str> {
        match variant {
            None | Some("raw") => Some(&self.raw),
            Some("original") => self.original.as_deref().or(Some(&self.raw)),
            Some(v) => self.templates.get(v).map(|s| s.as_str()),
        }
    }

    /// Return raw as minijinja Value — parsed object if json, string otherwise
    pub fn raw_value(&self) -> minijinja::Value {
        if let Some(ref v) = self.raw_parsed {
            minijinja::Value::from_serialize(v)
        } else {
            minijinja::Value::from(self.raw.clone())
        }
    }

    /// Return raw as serde_json Value — object if json, string otherwise
    pub fn raw_json(&self) -> serde_json::Value {
        if let Some(ref v) = self.raw_parsed {
            v.clone()
        } else {
            serde_json::Value::String(self.raw.clone())
        }
    }
}

struct StateQuery {
    name: String,
    variant: Option<String>,
}

fn parse_query(s: &str) -> StateQuery {
    match s.split_once('.') {
        Some((name, variant)) => StateQuery {
            name: name.to_string(),
            variant: Some(variant.to_string()),
        },
        None => StateQuery {
            name: s.to_string(),
            variant: None,
        },
    }
}

// Kahn's algorithm — returns layers of indices for parallel eval
fn topo_sort(states: &[StateConfig]) -> Result<Vec<Vec<usize>>> {
    let name_to_idx: HashMap<&str, usize> = states
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();

    // Validate deps exist
    for s in states {
        for dep in &s.deps {
            if !name_to_idx.contains_key(dep.as_str()) {
                bail!("State '{}' depends on unknown state '{}'", s.name, dep);
            }
        }
    }

    let n = states.len();
    let mut in_degree = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, s) in states.iter().enumerate() {
        in_degree[i] = s.deps.len();
        for dep in &s.deps {
            let dep_idx = name_to_idx[dep.as_str()];
            dependents[dep_idx].push(i);
        }
    }

    let mut layers = Vec::new();
    let mut ready: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut processed = 0;

    while !ready.is_empty() {
        layers.push(ready.clone());
        processed += ready.len();
        let mut next_ready = Vec::new();
        for &idx in &ready {
            for &dep_idx in &dependents[idx] {
                in_degree[dep_idx] -= 1;
                if in_degree[dep_idx] == 0 {
                    next_ready.push(dep_idx);
                }
            }
        }
        ready = next_ready;
    }

    if processed != n {
        bail!("Cycle detected in state dependencies");
    }
    Ok(layers)
}

fn add_filters(env: &mut minijinja::Environment) {
    env.add_filter(
        "fromjson",
        |s: String| -> Result<minijinja::Value, minijinja::Error> {
            serde_json::from_str::<serde_json::Value>(&s)
                .map(|v| minijinja::Value::from_serialize(&v))
                .map_err(|e| {
                    minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                })
        },
    );
}

fn eval_single(state: &StateConfig, dep_results: &HashMap<String, &StateResult>) -> StateResult {
    // Run cmd if present, with optional TTL cache
    let ttl = state
        .ttl
        .as_deref()
        .and_then(|s| crate::util::parse_duration(s).ok());
    let cache_key = format!("state-probe:{}", state.name);

    let cmd_output = state.cmd.as_ref().map(|cmd| {
        // Check cache first
        if let Some(max_age) = ttl {
            if let Some(cached) = crate::cache::get(&cache_key, Some(max_age)) {
                return String::from_utf8_lossy(&cached).to_string();
            }
        }

        let output = crate::util::shell_cmd(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok();
        let result = output
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        // Store in cache if TTL configured
        if ttl.is_some() {
            crate::cache::set(&cache_key, result.as_bytes());
        }

        result
    });

    // Evaluate expr — post-processes cmd output, or standalone with dep context
    let raw_before_rewrite = match &state.expr {
        Some(expr) => {
            let cmd_raw = cmd_output.unwrap_or_default();
            let mut env = minijinja::Environment::new();
            env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
            add_filters(&mut env);
            let mut ctx = HashMap::new();
            // cmd output available as `raw` in expr context
            if state.json {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&cmd_raw) {
                    ctx.insert("raw".to_string(), minijinja::Value::from_serialize(&v));
                } else {
                    ctx.insert("raw".to_string(), minijinja::Value::from(cmd_raw));
                }
            } else {
                ctx.insert("raw".to_string(), minijinja::Value::from(cmd_raw));
            }
            for (dep_name, dep_result) in dep_results {
                let mut dep_map: HashMap<String, serde_json::Value> = HashMap::new();
                dep_map.insert("raw".to_string(), dep_result.raw_json());
                if let Some(ref orig) = dep_result.original {
                    dep_map.insert("original".to_string(), serde_json::Value::String(orig.clone()));
                }
                for (tmpl_name, tmpl_val) in &dep_result.templates {
                    dep_map.insert(tmpl_name.clone(), serde_json::Value::String(tmpl_val.clone()));
                }
                ctx.insert(dep_name.replace('-', "_"), minijinja::Value::from_serialize(&dep_map));
            }
            env.add_template("_expr", expr).ok();
            env.get_template("_expr")
                .and_then(|t| t.render(&ctx))
                .unwrap_or_default()
        }
        None => cmd_output.unwrap_or_default(),
    };

    // Apply rewrites
    let mut original = None;
    let mut raw = raw_before_rewrite.clone();
    for rule in &state.rewrite {
        if let Ok(re) = regex::Regex::new(&rule.pattern) {
            if re.is_match(&raw) {
                original = Some(raw_before_rewrite.clone());
                raw = rule.value.clone();
                break;
            }
        }
    }

    // Parse JSON if flagged
    let raw_parsed = if state.json {
        serde_json::from_str::<serde_json::Value>(&raw).ok()
    } else {
        None
    };

    // Render templates
    let mut rendered = HashMap::new();
    if !state.templates.is_empty() {
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        add_filters(&mut env);

        let mut ctx = HashMap::new();
        // Use parsed JSON for raw if available
        if let Some(ref v) = raw_parsed {
            ctx.insert("raw".to_string(), minijinja::Value::from_serialize(v));
        } else {
            ctx.insert("raw".to_string(), minijinja::Value::from(raw.clone()));
        }
        if let Some(ref orig) = original {
            ctx.insert(
                "original".to_string(),
                minijinja::Value::from(orig.clone()),
            );
        }

        // Add dep values to context
        for (dep_name, dep_result) in dep_results {
            let mut dep_map: HashMap<String, serde_json::Value> = HashMap::new();
            dep_map.insert("raw".to_string(), dep_result.raw_json());
            if let Some(ref orig) = dep_result.original {
                dep_map.insert("original".to_string(), serde_json::Value::String(orig.clone()));
            }
            for (tmpl_name, tmpl_val) in &dep_result.templates {
                dep_map.insert(tmpl_name.clone(), serde_json::Value::String(tmpl_val.clone()));
            }
            ctx.insert(dep_name.replace('-', "_"), minijinja::Value::from_serialize(&dep_map));
        }

        for (tmpl_name, tmpl_src) in &state.templates {
            env.add_template(tmpl_name, tmpl_src).ok();
            if let Ok(tmpl) = env.get_template(tmpl_name) {
                if let Ok(val) = tmpl.render(&ctx) {
                    rendered.insert(tmpl_name.clone(), val);
                }
            }
        }
    }

    StateResult {
        name: state.name.clone(),
        original,
        raw,
        raw_parsed,
        templates: rendered,
    }
}

fn eval_all(states: &[StateConfig]) -> Result<Vec<StateResult>> {
    let layers = topo_sort(states)?;
    let mut results: HashMap<String, StateResult> = HashMap::new();

    for layer in &layers {
        if layer.len() == 1 {
            // Single state — no need for threading
            let idx = layer[0];
            let state = &states[idx];
            let dep_results: HashMap<String, &StateResult> = state
                .deps
                .iter()
                .filter_map(|d| results.get(d).map(|r| (d.clone(), r)))
                .collect();
            let result = eval_single(state, &dep_results);
            results.insert(result.name.clone(), result);
        } else {
            // Parallel eval within layer
            let layer_results: Vec<StateResult> = std::thread::scope(|s| {
                let handles: Vec<_> = layer
                    .iter()
                    .map(|&idx| {
                        let state = &states[idx];
                        let dep_results: HashMap<String, &StateResult> = state
                            .deps
                            .iter()
                            .filter_map(|d| results.get(d).map(|r| (d.clone(), r)))
                            .collect();
                        s.spawn(move || eval_single(state, &dep_results))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            for result in layer_results {
                results.insert(result.name.clone(), result);
            }
        }
    }

    // Return in original config order
    Ok(states
        .iter()
        .filter_map(|s| results.remove(&s.name))
        .collect())
}

pub fn run(
    config_path: Option<std::path::PathBuf>,
    name: Option<String>,
    json: bool,
    args: Vec<String>,
) -> Result<()> {
    let path = crate::resolve_config(config_path)?;
    let resolved_path = config::resolve_path(&path)?;
    crate::util::init_lib(&resolved_path);
    let meta = config::load_meta(&resolved_path);
    if let Some(ref vars) = meta.as_ref().and_then(|m| m.vars.as_ref()) {
        config::apply_vars(vars, &[]);
    }
    let cfg = config::load_all(&resolved_path)?;

    if cfg.state.is_empty() {
        bail!("No state probes defined in config");
    }

    // --json may end up in args due to trailing_var_arg
    let json = json || args.iter().any(|a| a == "--json");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--json").collect();

    // Parse the first name for dot notation
    let query = name.as_ref().map(|n| parse_query(n));

    // Collect additional names from args (non-operator mode)
    let has_op = query.is_some()
        && !args.is_empty()
        && matches!(args[0].as_str(), "is" | "isnot" | "get");

    let mut queries: Vec<StateQuery> = Vec::new();
    if let Some(q) = query {
        queries.push(q);
    }
    if !has_op {
        for a in &args {
            queries.push(parse_query(a));
        }
    }

    // Determine which states need evaluation
    let needed_names: Vec<&str> = if queries.is_empty() {
        cfg.state.iter().map(|s| s.name.as_str()).collect()
    } else {
        // Need to eval all states since deps may require it
        cfg.state.iter().map(|s| s.name.as_str()).collect()
    };
    let _ = needed_names; // We always eval all for simplicity with deps

    let results = eval_all(&cfg.state)?;
    let result_map: HashMap<&str, &StateResult> =
        results.iter().map(|r| (r.name.as_str(), r)).collect();

    // Operator mode
    if has_op {
        let q = &queries[0];
        let result = result_map
            .get(q.name.as_str())
            .ok_or_else(|| anyhow::anyhow!("Unknown state probe: {}", q.name))?;
        let value = result
            .get_variant(q.variant.as_deref())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Unknown variant '{}' for state '{}'",
                    q.variant.as_deref().unwrap_or(""),
                    q.name
                )
            })?;

        let op = &args[0];
        match op.as_str() {
            "is" => {
                let expected = args
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("Missing value after 'is'"))?;
                if value != *expected {
                    std::process::exit(1);
                }
            }
            "isnot" => {
                let expected = args
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("Missing value after 'isnot'"))?;
                if value == *expected {
                    std::process::exit(1);
                }
            }
            "get" => {
                if args.len() < 3 {
                    bail!("Usage: dek state <name> get <val>... <default>");
                }
                let allowed = &args[1..args.len() - 1];
                let fallback = &args[args.len() - 1];
                if allowed.iter().any(|a| a == value) {
                    print!("{}", value);
                } else {
                    print!("{}", fallback);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Filter to requested queries
    let display_results: Vec<(&str, &str, Option<&str>)> = if queries.is_empty() {
        // All probes, raw values
        results
            .iter()
            .map(|r| (r.name.as_str(), r.raw.as_str(), None))
            .collect()
    } else {
        let mut out = Vec::new();
        for q in &queries {
            let result = result_map
                .get(q.name.as_str())
                .ok_or_else(|| anyhow::anyhow!("Unknown state probe: {}", q.name))?;
            let value = result.get_variant(q.variant.as_deref()).ok_or_else(|| {
                anyhow::anyhow!(
                    "Unknown variant '{}' for state '{}'",
                    q.variant.as_deref().unwrap_or(""),
                    q.name
                )
            })?;
            let label = if q.variant.is_some() {
                // Reconstruct "name.variant"
                q.variant.as_deref()
            } else {
                None
            };
            out.push((q.name.as_str(), value, label));
        }
        out
    };

    // Single query, no json → plain value
    if display_results.len() == 1 && !json && !queries.is_empty() {
        println!("{}", display_results[0].1);
        return Ok(());
    }

    if json {
        let mut map = serde_json::Map::new();
        if queries.is_empty() {
            // Full JSON with nested objects
            for r in &results {
                let mut obj = serde_json::Map::new();
                obj.insert("raw".to_string(), r.raw_json());
                if let Some(ref orig) = r.original {
                    obj.insert(
                        "original".to_string(),
                        serde_json::Value::String(orig.clone()),
                    );
                }
                for (k, v) in &r.templates {
                    obj.insert(k.clone(), serde_json::Value::String(v.clone()));
                }
                map.insert(r.name.clone(), serde_json::Value::Object(obj));
            }
        } else {
            // Queried subset
            for (name, value, variant) in &display_results {
                let key = match variant {
                    Some(v) => format!("{}.{}", name, v),
                    None => name.to_string(),
                };
                map.insert(key, serde_json::Value::String(value.to_string()));
            }
        }
        println!("{}", serde_json::Value::Object(map));
    } else {
        let max_name = display_results
            .iter()
            .map(|(n, _, v)| {
                if let Some(var) = v {
                    n.len() + 1 + var.len()
                } else {
                    n.len()
                }
            })
            .max()
            .unwrap_or(0);
        for (name, value, variant) in &display_results {
            let label = match variant {
                Some(v) => format!("{}.{}", name, v),
                None => name.to_string(),
            };
            use owo_colors::OwoColorize;
            let lines: Vec<&str> = value.lines().collect();
            let mut lines = value.lines();
            if let Some(first) = lines.next() {
                println!(
                    "  {:>width$}  {}",
                    c!(label, cyan),
                    c!(first, bold),
                    width = max_name
                );
                for line in lines {
                    println!("{:indent$}{}", "", c!(line, bold), indent = max_name + 4);
                }
            }
        }
    }
    Ok(())
}

/// Evaluate a subset of states (+ transitive deps), returning name→result map
pub fn eval_states(
    states: &[StateConfig],
    needed: &[String],
) -> Result<HashMap<String, StateResult>> {
    // Compute transitive deps
    let name_set: HashMap<&str, &StateConfig> =
        states.iter().map(|s| (s.name.as_str(), s)).collect();
    let mut required: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = needed.to_vec();
    while let Some(name) = stack.pop() {
        if !required.insert(name.clone()) {
            continue;
        }
        if let Some(s) = name_set.get(name.as_str()) {
            for dep in &s.deps {
                stack.push(dep.clone());
            }
        }
    }

    // Filter to required states, preserving config order
    let filtered: Vec<StateConfig> = states
        .iter()
        .filter(|s| required.contains(&s.name))
        .cloned()
        .collect();

    let results = eval_all(&filtered)?;
    Ok(results.into_iter().map(|r| (r.name.clone(), r)).collect())
}

pub fn completions(states: &[StateConfig]) -> Vec<String> {
    let mut items = Vec::new();
    for s in states {
        items.push(s.name.clone());
        items.push(format!("{}.raw", s.name));
        items.push(format!("{}.original", s.name));
        for tmpl_name in s.templates.keys() {
            items.push(format!("{}.{}", s.name, tmpl_name));
        }
    }
    items.sort();
    items
}
