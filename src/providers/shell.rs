use super::{CheckResult, Provider, StateItem};
use crate::util::expand_path;
use anyhow::{Context, Result};
use std::fs;

const ALIAS_FILE: &str = "~/.dek_aliases";
const ENV_FILE: &str = "~/.dek_env";
const ALIAS_SOURCE_LINE: &str = "[ -f ~/.dek_aliases ] && source ~/.dek_aliases";
const ENV_SOURCE_LINE: &str = "[ -f ~/.dek_env ] && source ~/.dek_env";

// =============================================================================
// ALIAS
// =============================================================================

pub struct AliasProvider;

impl Provider for AliasProvider {
    fn name(&self) -> &'static str {
        "alias"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let file_path = expand_path(ALIAS_FILE);
        let alias_name = &state.key;
        let alias_value = state.value.as_deref().unwrap_or("");
        let expected_line = format!("alias {}='{}'", alias_name, alias_value);

        if !file_path.exists() {
            return Ok(CheckResult::Missing {
                detail: format!("alias file '{}' does not exist", file_path.display()),
            });
        }

        let content = fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read: {}", file_path.display()))?;

        if content.lines().any(|line| line == expected_line) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("alias '{}' not defined or has different value", alias_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let file_path = expand_path(ALIAS_FILE);
        let alias_name = &state.key;
        let alias_value = state.value.as_deref().unwrap_or("");
        let new_line = format!("alias {}='{}'", alias_name, alias_value);

        let content = if file_path.exists() {
            fs::read_to_string(&file_path)
                .with_context(|| format!("failed to read: {}", file_path.display()))?
        } else {
            String::from("# dek-managed aliases\n")
        };

        // Remove existing alias definition if present
        let prefix = format!("alias {}=", alias_name);
        let lines: Vec<&str> = content
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

        // Ensure shell rc sources the alias file
        ensure_sourced_in_rc(ALIAS_SOURCE_LINE)?;

        Ok(())
    }
}

// =============================================================================
// ENV
// =============================================================================

pub struct EnvProvider;

impl Provider for EnvProvider {
    fn name(&self) -> &'static str {
        "env"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let file_path = expand_path(ENV_FILE);
        let var_name = &state.key;
        let var_value = state.value.as_deref().unwrap_or("");
        let expected_line = format!("export {}=\"{}\"", var_name, var_value);

        if !file_path.exists() {
            return Ok(CheckResult::Missing {
                detail: format!("env file '{}' does not exist", file_path.display()),
            });
        }

        let content = fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read: {}", file_path.display()))?;

        if content.lines().any(|line| line == expected_line) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("env var '{}' not defined or has different value", var_name),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let file_path = expand_path(ENV_FILE);
        let var_name = &state.key;
        let var_value = state.value.as_deref().unwrap_or("");
        let new_line = format!("export {}=\"{}\"", var_name, var_value);

        let content = if file_path.exists() {
            fs::read_to_string(&file_path)
                .with_context(|| format!("failed to read: {}", file_path.display()))?
        } else {
            String::from("# dek-managed environment variables\n")
        };

        // Remove existing env var definition if present
        let prefix = format!("export {}=", var_name);
        let lines: Vec<&str> = content
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

        // Ensure shell rc sources the env file
        ensure_sourced_in_rc(ENV_SOURCE_LINE)?;

        Ok(())
    }
}

// =============================================================================
// HELPERS
// =============================================================================

/// Ensure a source line exists in the user's shell rc file
fn ensure_sourced_in_rc(line: &str) -> Result<()> {
    let rc_file = detect_shell_rc();
    let rc_path = expand_path(&rc_file);

    let content = if rc_path.exists() {
        fs::read_to_string(&rc_path)
            .with_context(|| format!("failed to read: {}", rc_path.display()))?
    } else {
        String::new()
    };

    // Already present
    if content.lines().any(|l| l == line) {
        return Ok(());
    }

    // Append the source line
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

/// Detect the user's shell rc file
fn detect_shell_rc() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.contains("zsh") {
            return "~/.zshrc".to_string();
        } else if shell.contains("fish") {
            return "~/.config/fish/config.fish".to_string();
        }
    }
    "~/.bashrc".to_string()
}
