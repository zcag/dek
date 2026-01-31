use super::{CheckResult, Provider, StateItem};
use crate::util::expand_path;
use anyhow::{Context, Result};
use std::fs;

/// Configuration for shell variable providers (aliases and env vars)
struct ShellVarConfig {
    name: &'static str,
    file: &'static str,
    source_line: &'static str,
    header: &'static str,
    format_line: fn(&str, &str) -> String,
    format_prefix: fn(&str) -> String,
}

const ALIAS_CONFIG: ShellVarConfig = ShellVarConfig {
    name: "alias",
    file: "~/.dek_aliases",
    source_line: "[ -f ~/.dek_aliases ] && source ~/.dek_aliases",
    header: "# dek-managed aliases\n",
    format_line: |k, v| format!("alias {}='{}'", k, v),
    format_prefix: |k| format!("alias {}=", k),
};

const ENV_CONFIG: ShellVarConfig = ShellVarConfig {
    name: "env",
    file: "~/.dek_env",
    source_line: "[ -f ~/.dek_env ] && source ~/.dek_env",
    header: "# dek-managed environment variables\n",
    format_line: |k, v| format!("export {}=\"{}\"", k, v),
    format_prefix: |k| format!("export {}=", k),
};

fn check_shell_var(cfg: &ShellVarConfig, state: &StateItem) -> Result<CheckResult> {
    let file_path = expand_path(cfg.file);
    let key = &state.key;
    let value = state.value.as_deref().unwrap_or("");
    let expected_line = (cfg.format_line)(key, value);

    if !file_path.exists() {
        return Ok(CheckResult::Missing {
            detail: format!("{} file '{}' does not exist", cfg.name, file_path.display()),
        });
    }

    let content = fs::read_to_string(&file_path)
        .with_context(|| format!("failed to read: {}", file_path.display()))?;

    if content.lines().any(|line| line == expected_line) {
        Ok(CheckResult::Satisfied)
    } else {
        Ok(CheckResult::Missing {
            detail: format!("{} '{}' not defined or has different value", cfg.name, key),
        })
    }
}

fn apply_shell_var(cfg: &ShellVarConfig, state: &StateItem) -> Result<()> {
    let file_path = expand_path(cfg.file);
    let key = &state.key;
    let value = state.value.as_deref().unwrap_or("");
    let new_line = (cfg.format_line)(key, value);
    let prefix = (cfg.format_prefix)(key);

    let content = if file_path.exists() {
        fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read: {}", file_path.display()))?
    } else {
        String::from(cfg.header)
    };

    // Remove existing definition if present
    let lines: Vec<_> = content
        .lines()
        .filter(|line| !line.starts_with(&prefix))
        .collect();

    let mut new_content = lines.join("\n");
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&new_line);
    new_content.push('\n');

    fs::write(&file_path, &new_content)
        .with_context(|| format!("failed to write: {}", file_path.display()))?;

    ensure_sourced_in_rc(cfg.source_line)?;
    Ok(())
}

pub struct AliasProvider;

impl Provider for AliasProvider {
    fn name(&self) -> &'static str {
        ALIAS_CONFIG.name
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        check_shell_var(&ALIAS_CONFIG, state)
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        apply_shell_var(&ALIAS_CONFIG, state)
    }
}

pub struct EnvProvider;

impl Provider for EnvProvider {
    fn name(&self) -> &'static str {
        ENV_CONFIG.name
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        check_shell_var(&ENV_CONFIG, state)
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        apply_shell_var(&ENV_CONFIG, state)
    }
}

/// Ensure a source line exists in the user's shell rc file
fn ensure_sourced_in_rc(line: &str) -> Result<()> {
    let rc_path = expand_path(detect_shell_rc());

    let content = if rc_path.exists() {
        fs::read_to_string(&rc_path)
            .with_context(|| format!("failed to read: {}", rc_path.display()))?
    } else {
        String::new()
    };

    if content.lines().any(|l| l == line) {
        return Ok(());
    }

    let mut new_content = content;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(line);
    new_content.push('\n');

    fs::write(&rc_path, &new_content)
        .with_context(|| format!("failed to write: {}", rc_path.display()))?;

    Ok(())
}

fn detect_shell_rc() -> &'static str {
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.contains("zsh") {
            return "~/.zshrc";
        } else if shell.contains("fish") {
            return "~/.config/fish/config.fish";
        }
    }
    "~/.bashrc"
}
