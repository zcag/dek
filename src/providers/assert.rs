use super::{CheckResult, Provider, StateItem};
use anyhow::Result;

pub struct AssertProvider;

impl Provider for AssertProvider {
    fn name(&self) -> &'static str {
        "assert"
    }

    fn is_check_only(&self) -> bool {
        true
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        // Value encoding: command\x00mode\x00stdout_pattern\x00stderr_pattern\x00message
        let value = state.value.as_deref().unwrap_or("");
        let parts: Vec<&str> = value.splitn(5, '\x00').collect();
        let cmd = parts.first().copied().unwrap_or("");
        let mode = parts.get(1).copied().unwrap_or("check");
        let stdout_pattern = parts.get(2).filter(|s| !s.is_empty()).copied();
        let stderr_pattern = parts.get(3).filter(|s| !s.is_empty()).copied();
        let message = parts.get(4).filter(|s| !s.is_empty()).copied();

        if mode == "foreach" {
            let output = crate::util::shell_cmd(cmd).output()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
            if lines.is_empty() {
                Ok(CheckResult::Satisfied)
            } else {
                Ok(CheckResult::Missing {
                    detail: lines.join(", "),
                })
            }
        } else {
            // check mode
            let output = crate::util::shell_cmd(cmd).output()?;

            if !output.status.success() {
                let detail = if let Some(msg) = message {
                    msg.to_string()
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    format!(
                        "exit {}: {}",
                        output.status.code().unwrap_or(-1),
                        stderr.trim()
                    )
                };
                return Ok(CheckResult::Missing { detail });
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if let Some(pattern) = stdout_pattern {
                let re = regex::Regex::new(pattern)
                    .map_err(|e| anyhow::anyhow!("Invalid stdout regex '{}': {}", pattern, e))?;
                if !re.is_match(&stdout) {
                    let detail = if let Some(msg) = message {
                        msg.to_string()
                    } else {
                        format!("stdout '{}' doesn't match '{}'", stdout.trim(), pattern)
                    };
                    return Ok(CheckResult::Missing { detail });
                }
            }

            if let Some(pattern) = stderr_pattern {
                let re = regex::Regex::new(pattern)
                    .map_err(|e| anyhow::anyhow!("Invalid stderr regex '{}': {}", pattern, e))?;
                if !re.is_match(&stderr) {
                    let detail = if let Some(msg) = message {
                        msg.to_string()
                    } else {
                        format!("stderr '{}' doesn't match '{}'", stderr.trim(), pattern)
                    };
                    return Ok(CheckResult::Missing { detail });
                }
            }

            Ok(CheckResult::Satisfied)
        }
    }

    fn apply(&self, _state: &StateItem) -> Result<()> {
        Ok(())
    }
}
