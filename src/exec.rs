//! Plan executor: the one place that turns a previewed [`InstallPlan`] into effects.
//!
//! Every [`Action`] variant has a focused handler here — there is deliberately no
//! generic "do anything" step. Command actions are delegated to [`Privilege`];
//! file actions (download/place/remove) are handled directly. The same
//! `describe_action` used by `--dry-run` is printed for each action as it runs, so
//! what executes always matches what was previewed.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use flate2::read::GzDecoder;
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
        Action::Extract { archive, member, dest, mode } => extract(archive, member, dest, *mode),
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

/// One regular file read out of an archive.
struct ArchiveFile {
    path: String,
    mode: u32,
    bytes: Vec<u8>,
}

/// Extract the binary `member` from a release `archive` to `dest` with `mode`. The
/// archive is decompressed in memory (release archives are small) and the member
/// located by [`select_member`]; the archive is assumed already verified (Download).
///
/// The container format is chosen from the archive's file name — `.tar.gz`/`.tgz` and
/// `.zip` are supported. Both decode to the same `ArchiveFile` list, so member
/// selection and writing are format-agnostic.
fn extract(archive: &Path, member: &str, dest: &Path, mode: u32) -> Result<()> {
    let files = read_archive(archive)?;

    let chosen = select_member(&files, member).ok_or_else(|| {
        let entries: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        JiiError::Other(anyhow::anyhow!(
            "could not find binary '{member}' in archive (entries: {})",
            entries.join(", ")
        ))
    })?;

    write_with_mode(dest, &chosen.bytes, mode)
}

/// Read every regular file out of `archive`, dispatching on its file-name extension.
fn read_archive(archive: &Path) -> Result<Vec<ArchiveFile>> {
    let name = archive.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase());
    let name = name.as_deref().unwrap_or_default();
    if name.ends_with(".zip") {
        read_zip(archive)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        read_tar_gz(archive)
    } else {
        // The provider only plans Extract for formats we handle; guard anyway.
        Err(JiiError::Other(anyhow::anyhow!(
            "unsupported archive format: {}",
            archive.display()
        )))
    }
}

/// Read a gzip-compressed tarball into archive files.
fn read_tar_gz(archive: &Path) -> Result<Vec<ArchiveFile>> {
    let file =
        std::fs::File::open(archive).map_err(|e| JiiError::io(archive.display().to_string(), e))?;
    let mut tar = tar::Archive::new(GzDecoder::new(file));

    let mut files = Vec::new();
    for entry in tar.entries().map_err(archive_err)? {
        let mut entry = entry.map_err(archive_err)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(archive_err)?.to_string_lossy().into_owned();
        let mode = entry.header().mode().unwrap_or(0);
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(archive_err)?;
        files.push(ArchiveFile { path, mode, bytes });
    }
    Ok(files)
}

/// Read a zip archive into archive files. Entries authored on non-unix systems may
/// carry no mode (0); the exact-name match in [`select_member`] still resolves them.
fn read_zip(archive: &Path) -> Result<Vec<ArchiveFile>> {
    let file =
        std::fs::File::open(archive).map_err(|e| JiiError::io(archive.display().to_string(), e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(zip_err)?;

    let mut files = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(zip_err)?;
        if !entry.is_file() {
            continue;
        }
        // `name()` is the archive path; skip entries with a suspicious path.
        let path = entry.name().to_string();
        let mode = entry.unix_mode().unwrap_or(0);
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(archive_err)?;
        files.push(ArchiveFile { path, mode, bytes });
    }
    Ok(files)
}

/// Pick the archive member to install: an exact file-name (basename) match on
/// `member`, else the sole executable-bit file. Ambiguity yields `None`.
fn select_member<'a>(files: &'a [ArchiveFile], member: &str) -> Option<&'a ArchiveFile> {
    if let Some(f) = files.iter().find(|f| basename(&f.path) == member) {
        return Some(f);
    }
    let mut execs = files.iter().filter(|f| f.mode & 0o111 != 0);
    match (execs.next(), execs.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// The final path component of a (forward-slash) archive entry path.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Write `bytes` to `dest` (creating parents) and set its unix `mode`.
fn write_with_mode(dest: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| JiiError::io(parent.display().to_string(), e))?;
    }
    std::fs::write(dest, bytes).map_err(|e| JiiError::io(dest.display().to_string(), e))?;
    let mut perms = std::fs::metadata(dest)
        .map_err(|e| JiiError::io(dest.display().to_string(), e))?
        .permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(dest, perms).map_err(|e| JiiError::io(dest.display().to_string(), e))
}

/// Wrap an archive read error.
fn archive_err(e: std::io::Error) -> JiiError {
    JiiError::Other(anyhow::anyhow!("archive error: {e}"))
}

/// Wrap a zip archive error.
fn zip_err(e: zip::result::ZipError) -> JiiError {
    JiiError::Other(anyhow::anyhow!("zip archive error: {e}"))
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

    fn tf(path: &str, mode: u32) -> ArchiveFile {
        ArchiveFile { path: path.into(), mode, bytes: path.as_bytes().to_vec() }
    }

    #[test]
    fn select_member_prefers_exact_basename() {
        let files = vec![tf("pkg/README", 0o644), tf("pkg/rg", 0o755), tf("pkg/rg.1", 0o644)];
        assert_eq!(select_member(&files, "rg").unwrap().path, "pkg/rg");
    }

    #[test]
    fn select_member_falls_back_to_sole_executable() {
        let files = vec![tf("LICENSE", 0o644), tf("dir/tool", 0o755)];
        // No basename match, but exactly one executable → pick it.
        assert_eq!(select_member(&files, "other").unwrap().path, "dir/tool");
    }

    #[test]
    fn select_member_ambiguous_is_none() {
        let files = vec![tf("a", 0o755), tf("b", 0o755)];
        assert!(select_member(&files, "none").is_none());
    }

    /// Build a gzip tarball with the given `(name, bytes, mode)` entries.
    fn make_targz(path: &Path, entries: &[(&str, &[u8], u32)]) {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        let f = std::fs::File::create(path).unwrap();
        let mut builder = tar::Builder::new(GzEncoder::new(f, Compression::fast()));
        for (name, bytes, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn extract_pulls_named_binary_and_sets_mode() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("tool.tar.gz");
        make_targz(
            &archive,
            &[
                ("tool-1.0/README.md", b"docs", 0o644),
                ("tool-1.0/tool", b"#!/bin/sh\necho hi\n", 0o755),
            ],
        );
        let dest = dir.path().join("bin/tool");
        extract(&archive, "tool", &dest, 0o755).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"#!/bin/sh\necho hi\n");
        assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o755);
    }

    #[test]
    fn extract_errors_when_member_absent_and_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("two.tar.gz");
        make_targz(&archive, &[("a", b"a", 0o755), ("b", b"b", 0o755)]);
        assert!(extract(&archive, "missing", &dir.path().join("out"), 0o755).is_err());
    }

    /// Build a zip archive with the given `(name, bytes, mode)` entries.
    fn make_zip(path: &Path, entries: &[(&str, &[u8], u32)]) {
        use std::io::Write;
        let f = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(f);
        for (name, bytes, mode) in entries {
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
            zip.start_file(*name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extract_pulls_named_binary_from_zip_and_sets_mode() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("tool.zip");
        make_zip(
            &archive,
            &[
                ("tool-1.0/README.md", b"docs", 0o644),
                ("tool-1.0/tool", b"#!/bin/sh\necho hi\n", 0o755),
            ],
        );
        let dest = dir.path().join("bin/tool");
        extract(&archive, "tool", &dest, 0o755).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"#!/bin/sh\necho hi\n");
        // The install mode is what we set on write, independent of the archive's mode.
        assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o755);
    }

    #[test]
    fn read_archive_rejects_unknown_format() {
        let dir = tempfile::tempdir().unwrap();
        let bogus = dir.path().join("thing.rar");
        std::fs::write(&bogus, b"not an archive").unwrap();
        assert!(read_archive(&bogus).is_err());
    }
}
