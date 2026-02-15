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
// FETCH (download URL to file)
// =============================================================================

pub struct FetchProvider;

/// Decode value: "path\x00ttl"
fn parse_fetch_value(state: &StateItem) -> (&str, Option<std::time::Duration>) {
    let raw = state.value.as_deref().unwrap_or("");
    let (path, ttl_str) = raw.split_once('\x00').unwrap_or((raw, ""));
    let ttl = if ttl_str.is_empty() {
        None
    } else {
        crate::util::parse_duration(ttl_str).ok()
    };
    (path, ttl)
}

impl Provider for FetchProvider {
    fn name(&self) -> &'static str {
        "file.fetch"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let url = &state.key;
        let (path, ttl) = parse_fetch_value(state);
        let dst = expand_path(path);

        if dst.as_os_str().is_empty() {
            bail!("file.fetch: destination not specified for '{}'", url);
        }

        if !dst.exists() {
            return Ok(CheckResult::Missing {
                detail: format!("destination '{}' does not exist", dst.display()),
            });
        }

        let src_content = crate::util::fetch_url(url, ttl)?;
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
        let url = &state.key;
        let (path, ttl) = parse_fetch_value(state);
        let dst = expand_path(path);

        if dst.as_os_str().is_empty() {
            bail!("file.fetch: destination not specified for '{}'", url);
        }

        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dirs for: {}", dst.display()))?;
        }

        let content = crate::util::fetch_url(url, ttl)?;
        fs::write(&dst, &content)
            .with_context(|| format!("failed to write: {}", dst.display()))?;

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

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dirs for: {}", file_path.display()))?;
        }

        let mut content = if file_path.exists() {
            fs::read_to_string(&file_path)
                .with_context(|| format!("failed to read: {}", file_path.display()))?
        } else {
            String::new()
        };

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

// =============================================================================
// TEMPLATE - render Jinja template files with state values
// =============================================================================

pub struct TemplateProvider;

impl Provider for TemplateProvider {
    fn name(&self) -> &'static str {
        "file.template"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let dst = expand_path(&state.key);
        let rendered = state.value.as_deref().unwrap_or("");

        if !dst.exists() {
            return Ok(CheckResult::Missing {
                detail: format!("destination '{}' does not exist", dst.display()),
            });
        }

        let current = fs::read_to_string(&dst)
            .with_context(|| format!("failed to read: {}", dst.display()))?;

        if current == rendered {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("contents differ for '{}'", dst.display()),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let dst = expand_path(&state.key);
        let rendered = state.value.as_deref().unwrap_or("");

        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dirs for: {}", dst.display()))?;
        }

        fs::write(&dst, rendered)
            .with_context(|| format!("failed to write: {}", dst.display()))?;

        Ok(())
    }
}

// =============================================================================
// FILE.LINE - structured ensure_line with original pattern matching
// =============================================================================

pub struct FileLineProvider;

impl Provider for FileLineProvider {
    fn name(&self) -> &'static str {
        "file.line"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let file_path = expand_path(&state.key);
        let value = state.value.as_deref().unwrap_or("");
        let line = value.split('\x01').next().unwrap_or("");

        if !file_path.exists() {
            return Ok(CheckResult::Missing {
                detail: format!("file '{}' does not exist", file_path.display()),
            });
        }

        let content = fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read: {}", file_path.display()))?;

        if content.contains(line) {
            Ok(CheckResult::Satisfied)
        } else {
            Ok(CheckResult::Missing {
                detail: format!("line missing in '{}'", file_path.display()),
            })
        }
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let file_path = expand_path(&state.key);
        let value = state.value.as_deref().unwrap_or("");
        let parts: Vec<&str> = value.splitn(4, '\x01').collect();
        let line = parts[0];
        let original = parts.get(1).filter(|s| !s.is_empty()).copied();
        let mode = parts.get(2).copied().unwrap_or("replace");
        let match_type = parts.get(3).copied().unwrap_or("literal");

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dirs for: {}", file_path.display()))?;
        }

        let mut content = if file_path.exists() {
            fs::read_to_string(&file_path)
                .with_context(|| format!("failed to read: {}", file_path.display()))?
        } else {
            String::new()
        };

        if content.contains(line) {
            return Ok(());
        }

        if let Some(pattern) = original {
            let file_lines: Vec<&str> = content.lines().collect();
            let mut new_lines: Vec<String> = Vec::with_capacity(file_lines.len() + 1);
            let mut found = false;

            // Build matcher based on type
            let is_regex = match_type == "regex";
            let re = if is_regex {
                Some(regex::Regex::new(pattern)
                    .map_err(|e| anyhow::anyhow!("Invalid original_regex '{}': {}", pattern, e))?)
            } else {
                None
            };

            for file_line in &file_lines {
                let matches = if let Some(ref re) = re {
                    re.is_match(file_line)
                } else {
                    file_line.trim() == pattern.trim()
                };

                if !found && matches {
                    found = true;
                    match mode {
                        "below" => {
                            new_lines.push(file_line.to_string());
                            new_lines.push(line.to_string());
                        }
                        _ => new_lines.push(line.to_string()),
                    }
                } else {
                    new_lines.push(file_line.to_string());
                }
            }

            if found {
                content = new_lines.join("\n");
                if !content.ends_with('\n') {
                    content.push('\n');
                }
            } else {
                if !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(line);
                content.push('\n');
            }
        } else {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(line);
            content.push('\n');
        }

        fs::write(&file_path, &content)
            .with_context(|| format!("failed to write: {}", file_path.display()))?;

        Ok(())
    }
}
