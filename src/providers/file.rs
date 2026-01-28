use super::{CheckResult, Provider, StateItem};
use crate::util::expand_path;
use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs as unix_fs;

// =============================================================================
// COPY
// =============================================================================

pub struct CopyProvider;

impl Provider for CopyProvider {
    fn name(&self) -> &'static str {
        "file.copy"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let src = expand_path(&state.key);
        let dst = expand_path(state.value.as_deref().unwrap_or(""));

        if dst.as_os_str().is_empty() {
            bail!("file.copy: destination not specified for '{}'", state.key);
        }

        if !dst.exists() {
            return Ok(CheckResult::Missing {
                detail: format!("destination '{}' does not exist", dst.display()),
            });
        }

        // Compare contents
        let src_content = fs::read(&src)
            .with_context(|| format!("failed to read source: {}", src.display()))?;
        let dst_content = fs::read(&dst)
            .with_context(|| format!("failed to read destination: {}", dst.display()))?;

        if src_content == dst_content {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("contents differ for '{}'", dst.display()),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let src = expand_path(&state.key);
        let dst = expand_path(state.value.as_deref().unwrap_or(""));

        if dst.as_os_str().is_empty() {
            bail!("file.copy: destination not specified for '{}'", state.key);
        }

        // Create parent directories
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dirs for: {}", dst.display()))?;
        }

        fs::copy(&src, &dst)
            .with_context(|| format!("failed to copy {} -> {}", src.display(), dst.display()))?;

        Ok(())
    }
}

// =============================================================================
// SYMLINK
// =============================================================================

pub struct SymlinkProvider;

impl Provider for SymlinkProvider {
    fn name(&self) -> &'static str {
        "file.symlink"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let target = expand_path(&state.key);
        let link = expand_path(state.value.as_deref().unwrap_or(""));

        if link.as_os_str().is_empty() {
            bail!("file.symlink: link path not specified for '{}'", state.key);
        }

        if !link.is_symlink() {
            return Ok(CheckResult::Missing {
                detail: format!("'{}' is not a symlink", link.display()),
            });
        }

        let current_target = fs::read_link(&link)
            .with_context(|| format!("failed to read symlink: {}", link.display()))?;

        if current_target == target {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!(
                    "symlink points to '{}', expected '{}'",
                    current_target.display(),
                    target.display()
                ),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let target = expand_path(&state.key);
        let link = expand_path(state.value.as_deref().unwrap_or(""));

        if link.as_os_str().is_empty() {
            bail!("file.symlink: link path not specified for '{}'", state.key);
        }

        // Create parent directories
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dirs for: {}", link.display()))?;
        }

        // Remove existing file/symlink if present
        if link.exists() || link.is_symlink() {
            if link.is_dir() && !link.is_symlink() {
                bail!(
                    "cannot replace directory '{}' with symlink",
                    link.display()
                );
            }
            fs::remove_file(&link)
                .with_context(|| format!("failed to remove existing: {}", link.display()))?;
        }

        unix_fs::symlink(&target, &link)
            .with_context(|| format!("failed to create symlink {} -> {}", link.display(), target.display()))?;

        Ok(())
    }
}

// =============================================================================
// ENSURE_LINE
// =============================================================================

pub struct EnsureLineProvider;

impl Provider for EnsureLineProvider {
    fn name(&self) -> &'static str {
        "file.ensure_line"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let file_path = expand_path(&state.key);
        let lines_to_ensure: Vec<&str> = state
            .value
            .as_deref()
            .unwrap_or("")
            .lines()
            .collect();

        if !file_path.exists() {
            return Ok(CheckResult::Missing {
                detail: format!("file '{}' does not exist", file_path.display()),
            });
        }

        let content = fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read: {}", file_path.display()))?;

        let missing: Vec<_> = lines_to_ensure
            .iter()
            .filter(|line| !content.contains(*line))
            .collect();

        if missing.is_empty() {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("{} line(s) missing in '{}'", missing.len(), file_path.display()),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let file_path = expand_path(&state.key);
        let lines_to_ensure: Vec<&str> = state
            .value
            .as_deref()
            .unwrap_or("")
            .lines()
            .collect();

        // Create parent directories
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dirs for: {}", file_path.display()))?;
        }

        // Read existing content or start empty
        let mut content = if file_path.exists() {
            fs::read_to_string(&file_path)
                .with_context(|| format!("failed to read: {}", file_path.display()))?
        } else {
            String::new()
        };

        // Append missing lines
        let mut modified = false;
        for line in lines_to_ensure {
            if !content.contains(line) {
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(line);
                content.push('\n');
                modified = true;
            }
        }

        if modified {
            fs::write(&file_path, &content)
                .with_context(|| format!("failed to write: {}", file_path.display()))?;
        }

        Ok(())
    }
}
