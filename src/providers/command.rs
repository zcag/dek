use super::{CheckResult, Provider, StateItem};
use anyhow::{bail, Result};
use std::process::Command;

pub struct CommandProvider;

impl Provider for CommandProvider {
    fn name(&self) -> &'static str {
        "command"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        // Value contains check and apply separated by \x00
        let value = state.value.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Command '{}' missing scripts", state.key))?;

        let parts: Vec<&str> = value.splitn(2, '\x00').collect();
        let check_script = parts.first()
            .ok_or_else(|| anyhow::anyhow!("Command '{}' missing check script", state.key))?;

        let status = Command::new("sh")
            .arg("-c")
            .arg(check_script)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;

        if status.success() {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("check failed (exit {})", status.code().unwrap_or(-1)),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let value = state.value.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Command '{}' missing scripts", state.key))?;

        let parts: Vec<&str> = value.splitn(2, '\x00').collect();
        let apply_script = parts.get(1)
            .ok_or_else(|| anyhow::anyhow!("Command '{}' missing apply script", state.key))?;

        let status = Command::new("sh")
            .arg("-c")
            .arg(apply_script)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;

        if !status.success() {
            bail!("apply failed (exit {})", status.code().unwrap_or(-1));
        }

        Ok(())
    }
}
