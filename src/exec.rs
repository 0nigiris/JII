//! Plan executor: the one place that turns a previewed [`InstallPlan`] into effects.
//!
//! Every [`Action`] variant has a focused handler here — there is deliberately no
//! generic "do anything" step. Command actions are delegated to [`Privilege`];
//! file actions (download/place/remove) are handled directly. The same
//! `describe_action` used by `--dry-run` is printed for each action as it runs, so
//! what executes always matches what was previewed.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{JiiError, Result};
use crate::model::{Action, InstallPlan, Verification};
use crate::privilege::Privilege;
use crate::ui::{Renderer, describe_action};

/// Execute a plan's actions in order. Primes privilege escalation once up front if
/// any action needs root, so a batch prompts at most once.
pub async fn run_plan(plan: &InstallPlan, privilege: &Privilege, renderer: &Renderer) -> Result<()> {
    if plan.needs_root() {
        privilege.prime().await?;
    }
    for action in &plan.actions {
        renderer.info(&describe_action(action));
        run_action(action, privilege).await?;
    }
    Ok(())
}

/// Dispatch a single action to its handler.
async fn run_action(action: &Action, privilege: &Privilege) -> Result<()> {
    match action {
        Action::RunCommand { argv, needs_root } => privilege.run(argv, *needs_root).await,
        Action::Download { url, dest, verify } => download(url, dest, verify).await,
        Action::Place { src, dest, mode } => place(src, dest, *mode),
        Action::RemoveFile { path } => remove_file(path),
    }
}

/// Download `url` to `dest` over HTTPS and enforce `verify` before the bytes are
/// written. The file only appears at `dest` if verification passed.
async fn download(url: &str, dest: &Path, verify: &Verification) -> Result<()> {
    if !url.starts_with("https://") {
        return Err(JiiError::Other(anyhow::anyhow!(
            "refusing to download over insecure transport: {url}"
        )));
    }
    let response = reqwest::get(url)
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("download failed: {e}")))?
        .error_for_status()
        .map_err(|e| JiiError::Other(anyhow::anyhow!("download failed: {e}")))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("download failed: {e}")))?;

    verify_bytes(&bytes, verify)?;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| JiiError::io(parent.display().to_string(), e))?;
    }
    std::fs::write(dest, &bytes).map_err(|e| JiiError::io(dest.display().to_string(), e))?;
    Ok(())
}

/// Verify downloaded bytes against the declared method. Unsupported methods fail
/// closed rather than silently accepting the artifact.
fn verify_bytes(bytes: &[u8], verify: &Verification) -> Result<()> {
    match verify {
        Verification::Sha256(expected) => {
            let actual = hex_digest(bytes);
            if actual.eq_ignore_ascii_case(expected) {
                Ok(())
            } else {
                Err(JiiError::Other(anyhow::anyhow!(
                    "checksum mismatch: expected {expected}, got {actual}"
                )))
            }
        }
        Verification::None => Ok(()),
        Verification::Gpg => Err(JiiError::Other(anyhow::anyhow!(
            "gpg verification is not supported yet"
        ))),
        Verification::Sigstore => Err(JiiError::Other(anyhow::anyhow!(
            "sigstore verification is not supported yet"
        ))),
    }
}

/// Lowercase hex SHA-256 of `bytes`.
fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Copy `src` to `dest` and set its unix mode (e.g. make a downloaded binary
/// executable at ~/.local/bin).
fn place(src: &Path, dest: &Path, mode: u32) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| JiiError::io(parent.display().to_string(), e))?;
    }
    std::fs::copy(src, dest).map_err(|e| JiiError::io(dest.display().to_string(), e))?;
    let mut perms = std::fs::metadata(dest)
        .map_err(|e| JiiError::io(dest.display().to_string(), e))?
        .permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(dest, perms).map_err(|e| JiiError::io(dest.display().to_string(), e))?;
    Ok(())
}

/// Remove a single file (uninstall for file-based sources).
fn remove_file(path: &Path) -> Result<()> {
    std::fs::remove_file(path).map_err(|e| JiiError::io(path.display().to_string(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_hello_matches() {
        // Known digest of b"hello".
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_eq!(hex_digest(b"hello"), expected);
    }

    #[test]
    fn verify_sha256_accepts_correct_and_is_case_insensitive() {
        let v = Verification::Sha256(
            "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824".into(),
        );
        assert!(verify_bytes(b"hello", &v).is_ok());
    }

    #[test]
    fn verify_sha256_rejects_wrong_digest() {
        let v = Verification::Sha256("deadbeef".into());
        assert!(verify_bytes(b"hello", &v).is_err());
    }

    #[test]
    fn verify_none_accepts_anything() {
        assert!(verify_bytes(b"whatever", &Verification::None).is_ok());
    }

    #[test]
    fn verify_gpg_and_sigstore_fail_closed() {
        assert!(verify_bytes(b"x", &Verification::Gpg).is_err());
        assert!(verify_bytes(b"x", &Verification::Sigstore).is_err());
    }

    #[test]
    fn place_copies_and_sets_mode_then_remove_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dest = dir.path().join("bin/tool");
        std::fs::write(&src, b"#!/bin/sh\n").unwrap();

        place(&src, &dest, 0o755).unwrap();
        assert!(dest.exists());
        let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);

        remove_file(&dest).unwrap();
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn run_action_command_success_and_failure() {
        let priv_ = Privilege::detect();
        let ok = Action::RunCommand {
            argv: vec!["true".into()],
            needs_root: false,
        };
        let bad = Action::RunCommand {
            argv: vec!["false".into()],
            needs_root: false,
        };
        assert!(run_action(&ok, &priv_).await.is_ok());
        assert!(run_action(&bad, &priv_).await.is_err());
    }
}
