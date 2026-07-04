//! Privilege escalation — the single place that runs commands and elevates.
//!
//! Providers only *describe* steps (`Step { needs_root, argv, .. }`); this module
//! decides how to elevate (`sudo` on a TTY, `pkexec` otherwise), shows the exact
//! commands first, and runs them. JII itself is never fully run as root.

use tokio::process::Command;

use crate::error::{JiiError, Result};
use crate::model::{InstallPlan, Step};
use crate::platform::{ElevationKind, Platform};
use crate::ui::Renderer;

/// Executes plans, elevating privileged steps as needed.
pub struct Privilege {
    kind: ElevationKind,
}

impl Privilege {
    /// Choose the elevation mechanism from the current session.
    pub fn detect() -> Self {
        Privilege {
            kind: Platform::detect().elevation_kind(),
        }
    }

    /// The concrete argv to run for a step, with the elevation prefix if required.
    pub fn elevated_argv(&self, step: &Step) -> Vec<String> {
        if !step.needs_root {
            return step.argv.clone();
        }
        let prefix = match self.kind {
            ElevationKind::Sudo => "sudo",
            ElevationKind::Pkexec => "pkexec",
        };
        let mut argv = vec![prefix.to_string()];
        argv.extend(step.argv.iter().cloned());
        argv
    }

    /// Execute every step in the plan, elevating where needed.
    ///
    /// The exact commands are shown first; if any step needs root and we are using
    /// `sudo`, credentials are primed once so there is a single password prompt for
    /// the whole batch.
    pub async fn execute_plan(&self, plan: &InstallPlan, renderer: &Renderer) -> Result<()> {
        let needs_root = plan.steps.iter().any(|s| s.needs_root);

        renderer.info("Running:");
        for step in &plan.steps {
            renderer.info(&format!("  $ {}", self.elevated_argv(step).join(" ")));
        }

        if needs_root && self.kind == ElevationKind::Sudo {
            self.prime_sudo().await?;
        }

        for step in &plan.steps {
            self.run_step(step).await?;
        }
        Ok(())
    }

    /// Prime sudo credentials once (`sudo -v`) so the batch prompts at most once.
    async fn prime_sudo(&self) -> Result<()> {
        let status = Command::new("sudo")
            .arg("-v")
            .status()
            .await
            .map_err(|e| JiiError::spawn("sudo", e))?;
        if !status.success() {
            return Err(JiiError::Other(anyhow::anyhow!(
                "privilege escalation was declined"
            )));
        }
        Ok(())
    }

    /// Run a single step with inherited stdio (so output and prompts pass through).
    async fn run_step(&self, step: &Step) -> Result<()> {
        let argv = self.elevated_argv(step);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        if let Some(cwd) = &step.cwd {
            cmd.current_dir(cwd);
        }
        let status = cmd
            .status()
            .await
            .map_err(|e| JiiError::spawn(&argv[0], e))?;
        if !status.success() {
            return Err(JiiError::Other(anyhow::anyhow!(
                "command failed: {}",
                argv.join(" ")
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(needs_root: bool) -> Step {
        Step {
            argv: vec!["dnf5".into(), "install".into(), "-y".into(), "fastfetch".into()],
            needs_root,
            cwd: None,
        }
    }

    #[test]
    fn root_step_gets_sudo_prefix() {
        let p = Privilege {
            kind: ElevationKind::Sudo,
        };
        assert_eq!(p.elevated_argv(&step(true))[0], "sudo");
    }

    #[test]
    fn root_step_gets_pkexec_prefix() {
        let p = Privilege {
            kind: ElevationKind::Pkexec,
        };
        assert_eq!(p.elevated_argv(&step(true))[0], "pkexec");
    }

    #[test]
    fn user_step_is_unmodified() {
        let p = Privilege {
            kind: ElevationKind::Sudo,
        };
        assert_eq!(p.elevated_argv(&step(false))[0], "dnf5");
    }
}
