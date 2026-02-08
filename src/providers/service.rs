use super::{CheckResult, Provider, StateItem};
use crate::util::{run_cmd, run_cmd_live, run_sudo, run_sudo_live};
use anyhow::{bail, Result};
use indicatif::ProgressBar;

pub struct SystemdProvider;

impl Provider for SystemdProvider {
    fn name(&self) -> &'static str {
        "service"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let config = parse_service_config(state)?;
        let name = &state.key;
        let user = config.is_user();

        // Check if service exists
        let exists = systemctl_cmd(&["cat", name], user)?.status.success();
        if !exists {
            return Ok(CheckResult::Missing {
                detail: format!("service '{}' not found", name),
            });
        }

        // Check enabled state if required
        if config.enabled {
            let enabled = systemctl_cmd(&["is-enabled", name], user)?
                .status
                .success();
            if !enabled {
                return Ok(CheckResult::Missing {
                    detail: format!("service '{}' not enabled", name),
                });
            }
        }

        // Check active state if required
        if config.state == "active" {
            let active = systemctl_cmd(&["is-active", name], user)?
                .status
                .success();
            if !active {
                return Ok(CheckResult::Missing {
                    detail: format!("service '{}' not active", name),
                });
            }
        }

        Ok(CheckResult::Satisfied)
    }

    fn apply(&self, state: &StateItem) -> Result<()> {
        let config = parse_service_config(state)?;
        let name = &state.key;
        let user = config.is_user();

        if config.enabled {
            let output = systemctl_run(&["enable", name], user)?;
            if !output.status.success() {
                bail!(
                    "systemctl enable failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        if config.state == "active" {
            let output = systemctl_run(&["start", name], user)?;
            if !output.status.success() {
                bail!(
                    "systemctl start failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(())
    }

    fn apply_live(&self, state: &StateItem, pb: &ProgressBar) -> Result<()> {
        let config = parse_service_config(state)?;
        let name = &state.key;
        let user = config.is_user();

        if config.enabled {
            let output = systemctl_run_live(&["enable", name], user, pb)?;
            if !output.status.success() {
                bail!(
                    "systemctl enable failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        if config.state == "active" {
            let output = systemctl_run_live(&["start", name], user, pb)?;
            if !output.status.success() {
                bail!(
                    "systemctl start failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(())
    }
}

/// Run systemctl for checking (no sudo needed)
fn systemctl_cmd(args: &[&str], user: bool) -> Result<std::process::Output> {
    if user {
        let mut full_args = vec!["--user"];
        full_args.extend(args);
        run_cmd("systemctl", &full_args)
    } else {
        run_cmd("systemctl", args)
    }
}

/// Run systemctl for mutations - user scope runs directly, system scope uses sudo
fn systemctl_run(args: &[&str], user: bool) -> Result<std::process::Output> {
    if user {
        let mut full_args = vec!["--user"];
        full_args.extend(args);
        run_cmd("systemctl", &full_args)
    } else {
        run_sudo("systemctl", args)
    }
}

/// Run systemctl for mutations with live progress
fn systemctl_run_live(args: &[&str], user: bool, pb: &ProgressBar) -> Result<std::process::Output> {
    if user {
        let mut full_args = vec!["--user"];
        full_args.extend(args);
        run_cmd_live("systemctl", &full_args, pb)
    } else {
        run_sudo_live("systemctl", args, pb)
    }
}

struct ServiceConfig {
    state: String,
    enabled: bool,
    scope: String,
}

impl ServiceConfig {
    fn is_user(&self) -> bool {
        self.scope == "user"
    }
}

fn parse_service_config(state: &StateItem) -> Result<ServiceConfig> {
    let value = state.value.as_deref().unwrap_or("state=active,enabled=false,scope=system");

    let mut config = ServiceConfig {
        state: "active".to_string(),
        enabled: false,
        scope: "system".to_string(),
    };

    for part in value.split(',') {
        if let Some((key, val)) = part.split_once('=') {
            match key.trim() {
                "state" => config.state = val.trim().to_string(),
                "enabled" => config.enabled = val.trim() == "true",
                "scope" => config.scope = val.trim().to_string(),
                _ => {}
            }
        }
    }

    Ok(config)
}
