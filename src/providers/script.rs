use super::{CheckResult, Provider, StateItem};
use crate::util::expand_path;
use anyhow::Result;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub struct ScriptProvider;

impl ScriptProvider {
    fn target_path(name: &str) -> PathBuf {
        expand_path("~/.local/bin").join(name)
    }
}

impl Provider for ScriptProvider {
    fn name(&self) -> &'static str {
        "script"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let target = Self::target_path(&state.key);

        if !target.exists() {
            return Ok(CheckResult::Missing {
                detail: format!("'{}' not installed", target.display()),
            });
        }

        // Check if content matches (if we have source content)
        if let Some(source_content) = &state.value {
            let target_content = fs::read_to_string(&target).unwrap_or_default();
            if target_content != *source_content {
                return Ok(CheckResult::Missing {
                    detail: "content differs".to_string(),
                });
            }
        }

        Ok(CheckResult::Satisfied)
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let target = Self::target_path(&state.key);
        let content = state.value.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Script '{}' missing content", state.key))?;

        // Ensure ~/.local/bin exists
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write script
        fs::write(&target, content)?;

        // Make executable
        let mut perms = fs::metadata(&target)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target, perms)?;

        Ok(())
    }
}
