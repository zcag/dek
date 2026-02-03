use super::{CheckResult, Provider, StateItem};
use anyhow::Result;
use std::process::Command;

pub struct AssertProvider;

impl Provider for AssertProvider {
    fn name(&self) -> &'static str {
        "assert"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        // Key is the check command
        // Value contains stdout_pattern\x00stderr_pattern
        let check_cmd = &state.key;

        let output = Command::new("sh")
            .arg("-c")
            .arg(check_cmd)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(CheckResult::Missing {
                detail: format!("exit {}: {}", output.status.code().unwrap_or(-1), stderr.trim()),
            });
        }

        // Check stdout/stderr patterns if specified
        if let Some(ref value) = state.value {
            let parts: Vec<&str> = value.splitn(2, '\x00').collect();
            let stdout_pattern = parts.first().filter(|s| !s.is_empty());
            let stderr_pattern = parts.get(1).filter(|s| !s.is_empty());

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if let Some(pattern) = stdout_pattern {
                let re = regex::Regex::new(pattern)
                    .map_err(|e| anyhow::anyhow!("Invalid stdout regex '{}': {}", pattern, e))?;
                if !re.is_match(&stdout) {
                    return Ok(CheckResult::Missing {
                        detail: format!("stdout '{}' doesn't match '{}'", stdout.trim(), pattern),
                    });
                }
            }

            if let Some(pattern) = stderr_pattern {
                let re = regex::Regex::new(pattern)
                    .map_err(|e| anyhow::anyhow!("Invalid stderr regex '{}': {}", pattern, e))?;
                if !re.is_match(&stderr) {
                    return Ok(CheckResult::Missing {
                        detail: format!("stderr '{}' doesn't match '{}'", stderr.trim(), pattern),
                    });
                }
            }
        }

        Ok(CheckResult::Satisfied)
    }

    fn apply(&self, _state: &StateItem) -> Result<()> {
        // Assertions don't apply anything - they just check
        Ok(())
    }
}
