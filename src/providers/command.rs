use super::{CheckResult, Provider, StateItem};
use anyhow::{bail, Result};
use indicatif::ProgressBar;
use std::io::Write;

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

        let status = crate::util::shell_cmd(check_script)
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

    fn apply_live(&self, state: &StateItem, pb: &ProgressBar) -> Result<()> {
        let value = state.value.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Command '{}' missing scripts", state.key))?;

        let parts: Vec<&str> = value.splitn(3, '\x00').collect();
        let apply_script = parts.get(1)
            .ok_or_else(|| anyhow::anyhow!("Command '{}' missing apply script", state.key))?;
        let confirm = parts.get(2).map(|s| *s == "1").unwrap_or(false);

        if confirm {
            use owo_colors::OwoColorize;
            let proceed = pb.suspend(|| -> Result<bool> {
                print!("Apply {}? [y/N] ", c!(state.key, bold));
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                Ok(input.trim().eq_ignore_ascii_case("y"))
            })?;
            if !proceed {
                return Ok(());
            }
        }

        let status = crate::util::shell_cmd(apply_script)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;

        if !status.success() {
            bail!("apply failed (exit {})", status.code().unwrap_or(-1));
        }

        Ok(())
    }

    fn apply(&self, _state: &StateItem) -> Result<()> {
        unreachable!("apply_live overridden")
    }
}
