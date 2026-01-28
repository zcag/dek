use super::{CheckResult, Provider, StateItem};
use crate::util::{run_cmd, run_sudo};
use anyhow::{bail, Result};

pub struct SystemdProvider;

impl Provider for SystemdProvider {
    fn name(&self) -> &'static str {
        "service"
    }

    fn check(&self, state: &StateItem) -> Result<CheckResult> {
        let config = parse_service_config(state)?;
        let name = &state.key;

        // Check if service exists
        let exists = run_cmd("systemctl", &["cat", name])?.status.success();
        if !exists {
            return Ok(CheckResult::Missing {
                detail: format!("service '{}' not found", name),
            });
        }

        // Check enabled state if required
        if config.enabled {
            let enabled = run_cmd("systemctl", &["is-enabled", name])?
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
            let active = run_cmd("systemctl", &["is-active", name])?
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

        // Enable if required
        if config.enabled {
            let output = run_sudo("systemctl", &["enable", name])?;
            if !output.status.success() {
                bail!(
                    "systemctl enable failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        // Start if active state required
        if config.state == "active" {
            let output = run_sudo("systemctl", &["start", name])?;
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

struct ServiceConfig {
    state: String,
    enabled: bool,
}

fn parse_service_config(state: &StateItem) -> Result<ServiceConfig> {
    let value = state.value.as_deref().unwrap_or("state=active,enabled=false");

    let mut config = ServiceConfig {
        state: "active".to_string(),
        enabled: false,
    };

    for part in value.split(',') {
        if let Some((key, val)) = part.split_once('=') {
            match key.trim() {
                "state" => config.state = val.trim().to_string(),
                "enabled" => config.enabled = val.trim() == "true",
                _ => {}
            }
        }
    }

    Ok(config)
}
