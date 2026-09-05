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
    ///
    /// Already root, or no helper at all: the command runs as itself. The first case is
    /// correct (root needs nothing in front of it); the second is caught by
    /// [`Privilege::ensure_possible`] before anything runs, so it never reaches here.
    pub fn elevated_argv(&self, argv: &[String], needs_root: bool) -> Vec<String> {
        if !needs_root {
            return argv.to_vec();
        }
        let Some(prefix) = self.kind.helper() else {
            return argv.to_vec();
        };
        let mut out = vec![prefix.to_string()];
        out.extend(argv.iter().cloned());
        out
    }

    /// Refuse, in words, when a step needs root on a machine that cannot grant it.
    ///
    /// Called before the first privileged command so the run stops with an explanation
    /// and a way out, instead of the spawn failure a tester actually saw
    /// ("failed to run sudo: No such file or directory").
    pub fn ensure_possible(&self, needs_root: bool) -> Result<()> {
        if needs_root && self.kind == ElevationKind::Missing {
            return Err(JiiError::NoElevation);
        }
        Ok(())
    }

    /// Prime credentials once (`sudo -v`) so a batch of root commands prompts at most
    /// once. A no-op for every other kind: `pkexec` prompts per invocation, `doas` has
    /// no priming verb, and root has nothing to prime.
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
        self.ensure_possible(needs_root)?;
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

    /// Run one command **streaming** its output line by line — each line is handed to `on_line`
    /// *as it arrives* and also accumulated — while still returning `(success, combined_output)`
    /// (so a chatty manager can be reduced to a one-line summary without flooding the terminal).
    /// This is what lets a live progress bar read the manager's own
    /// `[3/41]` / `NN%` chatter without waiting for the whole command to finish (which
    /// `.output()` would). The caller must have `prime`d first (stdin is closed, so a manager
    /// must be non-interactive — our plans already pass `-y`).
    ///
    /// stdout and stderr are both piped and read concurrently (managers split progress across
    /// the two), so neither can block the other by filling its pipe. Lines are split on `\n`:
    /// when piped, managers line-buffer plain text rather than the `\r`-animated bar they draw
    /// on a TTY, so newline framing is exactly what arrives here.
    pub async fn run_streamed<F: FnMut(&str)>(
        &self,
        argv: &[String],
        needs_root: bool,
        mut on_line: F,
    ) -> Result<(bool, String)> {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};

        self.ensure_possible(needs_root)?;
        let argv = self.elevated_argv(argv, needs_root);
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| JiiError::spawn(&argv[0], e))?;

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let mut out_lines = BufReader::new(stdout).lines();
        let mut err_lines = BufReader::new(stderr).lines();

        let mut combined = String::new();
        let (mut out_done, mut err_done) = (false, false);
        while !(out_done && err_done) {
            tokio::select! {
                line = out_lines.next_line(), if !out_done => match line {
                    Ok(Some(l)) => { on_line(&l); combined.push_str(&l); combined.push('\n'); }
                    Ok(None) => out_done = true,
                    Err(e) => return Err(JiiError::spawn(&argv[0], e)),
                },
                line = err_lines.next_line(), if !err_done => match line {
                    Ok(Some(l)) => { on_line(&l); combined.push_str(&l); combined.push('\n'); }
                    Ok(None) => err_done = true,
                    Err(e) => return Err(JiiError::spawn(&argv[0], e)),
                },
            }
        }

        let status = child.wait().await.map_err(|e| JiiError::spawn(&argv[0], e))?;
        Ok((status.success(), combined))
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

    /// A root shell runs the manager itself. Prefixing `sudo` there is what broke every
    /// install on the tester's Arch container, which had no sudo at all.
    #[test]
    fn as_root_the_command_runs_bare() {
        let p = Privilege {
            kind: ElevationKind::AlreadyRoot,
        };
        assert_eq!(p.elevated_argv(&argv(), true), argv());
        assert!(p.ensure_possible(true).is_ok());
    }

    #[test]
    fn doas_stands_in_for_sudo_where_that_is_what_the_distro_ships() {
        let p = Privilege {
            kind: ElevationKind::Doas,
        };
        assert_eq!(p.elevated_argv(&argv(), true)[0], "doas");
    }

    /// No helper anywhere: JII must say so before running anything, and only for steps
    /// that actually need root.
    #[test]
    fn no_helper_is_refused_in_words_but_only_when_root_is_needed() {
        let p = Privilege {
            kind: ElevationKind::Missing,
        };
        assert!(matches!(p.ensure_possible(true), Err(JiiError::NoElevation)));
        assert!(p.ensure_possible(false).is_ok());
    }

    #[tokio::test]
    async fn run_streamed_forwards_each_line_and_captures_output() {
        let p = Privilege::detect();
        let mut seen: Vec<String> = Vec::new();
        let (ok, combined) = p
            .run_streamed(
                &["sh".into(), "-c".into(), "printf '[1/2] a\\n[2/2] b\\n'".into()],
                false,
                |line| seen.push(line.to_string()),
            )
            .await
            .unwrap();
        assert!(ok);
        assert_eq!(seen, ["[1/2] a", "[2/2] b"]);
        assert!(combined.contains("[1/2] a") && combined.contains("[2/2] b"));
    }

    #[tokio::test]
    async fn run_streamed_reports_failure_without_erroring() {
        let p = Privilege::detect();
        let (ok, _) = p
            .run_streamed(&["false".into()], false, |_| {})
            .await
            .unwrap();
        assert!(!ok); // a non-zero exit is (false, output), not a hard Err
    }
}
