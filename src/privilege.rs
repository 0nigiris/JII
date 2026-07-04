//! Privilege escalation — the single place that runs external commands and elevates.
//!
//! Its one responsibility is running a command with the right elevation (`sudo` on a
//! TTY, `pkexec` otherwise). It does not know about plans or non-command actions;
//! the plan executor (see `exec`) dispatches command actions here. JII itself is
//! never fully run as root.

use tokio::process::Command;

use crate::error::{JiiError, Result};
use crate::platform::{ElevationKind, Platform};

/// Runs external commands, elevating when required.
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

    /// The concrete argv to run, with the elevation prefix when `needs_root`.
    pub fn elevated_argv(&self, argv: &[String], needs_root: bool) -> Vec<String> {
        if !needs_root {
            return argv.to_vec();
        }
        let prefix = match self.kind {
            ElevationKind::Sudo => "sudo",
            ElevationKind::Pkexec => "pkexec",
        };
        let mut out = vec![prefix.to_string()];
        out.extend(argv.iter().cloned());
        out
    }

    /// Prime credentials once (`sudo -v`) so a batch of root commands prompts at most
    /// once. A no-op for `pkexec`, which prompts per invocation.
    pub async fn prime(&self) -> Result<()> {
        if self.kind != ElevationKind::Sudo {
            return Ok(());
        }
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

    /// Run one command with inherited stdio (so output and prompts pass through),
    /// elevating if `needs_root`.
    pub async fn run(&self, argv: &[String], needs_root: bool) -> Result<()> {
        let argv = self.elevated_argv(argv, needs_root);
        let status = Command::new(&argv[0])
            .args(&argv[1..])
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

    fn argv() -> Vec<String> {
        vec!["dnf5".into(), "install".into(), "-y".into(), "fastfetch".into()]
    }

    #[test]
    fn root_command_gets_sudo_prefix() {
        let p = Privilege {
            kind: ElevationKind::Sudo,
        };
        assert_eq!(p.elevated_argv(&argv(), true)[0], "sudo");
    }

    #[test]
    fn root_command_gets_pkexec_prefix() {
        let p = Privilege {
            kind: ElevationKind::Pkexec,
        };
        assert_eq!(p.elevated_argv(&argv(), true)[0], "pkexec");
    }

    #[test]
    fn user_command_is_unmodified() {
        let p = Privilege {
            kind: ElevationKind::Sudo,
        };
        assert_eq!(p.elevated_argv(&argv(), false), argv());
    }
}
