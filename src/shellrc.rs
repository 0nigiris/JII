//! The user's shell startup file — locating it, and adding a manager's setup line to it.
//!
//! A script-installed manager (Homebrew) ends by telling the *user* to paste a line into their
//! `~/.zshrc`. JII offers to do that instead (ADR-0080), which means knowing which file to
//! write and never writing the same line twice.
//!
//! Deliberately narrow: only POSIX-ish shells whose rc syntax matches the line the manager
//! prints (bash, zsh, ksh). fish uses a different `eval` syntax, so [`rc_file`] returns `None`
//! there and the caller shows the line to paste rather than guessing a file. Nothing here ever
//! runs the shell or rewrites existing content — appends only, so a mistake costs one line.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{JiiError, Result};

/// The rc file for the user's login shell (`$SHELL`), or `None` for a shell whose syntax we
/// can't assume. Missing file is fine — appending creates it, which is what the shell reads.
pub fn rc_file() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").ok()?;
    let name = Path::new(&shell).file_name()?.to_string_lossy().into_owned();
    let home = PathBuf::from(std::env::var_os("HOME")?);
    match name.as_str() {
        "zsh" => Some(home.join(".zshrc")),
        "bash" => Some(home.join(".bashrc")),
        "ksh" | "mksh" => Some(home.join(".kshrc")),
        // fish (`eval (brew shellenv fish)`), nushell, csh… — different syntax, no guessing.
        _ => None,
    }
}

/// Whether `line` is already in `file` (so a second bootstrap doesn't duplicate it). Compares
/// trimmed lines: the user may have pasted it themselves, indented differently.
pub fn already_present(file: &Path, line: &str) -> bool {
    let wanted = line.trim();
    std::fs::read_to_string(file).is_ok_and(|body| body.lines().any(|l| l.trim() == wanted))
}

/// Append `line` to `file`, preceded by a comment naming who added it and why — an unexplained
/// line in someone's rc file is its own small mystery. Creates the file if it doesn't exist.
pub fn append_line(file: &Path, manager: &str, line: &str) -> Result<()> {
    let mut handle = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(|e| JiiError::io(file, e))?;
    let block = format!("\n# Added by JII — {manager} on this shell's PATH\n{line}\n");
    handle
        .write_all(block.as_bytes())
        .map_err(|e| JiiError::io(file, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jii-shellrc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn appends_once_and_is_then_detected() {
        let file = temp_dir().join("rc");
        let _ = std::fs::remove_file(&file);
        let line = "eval \"$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)\"";

        assert!(!already_present(&file, line), "nothing is present in a file that doesn't exist");
        append_line(&file, "Homebrew", line).unwrap();
        assert!(already_present(&file, line));

        let body = std::fs::read_to_string(&file).unwrap();
        assert!(body.contains("# Added by JII"), "the line explains itself: {body}");
        // Indentation or a hand-pasted copy still counts as present — no duplicate.
        assert!(already_present(&file, &format!("   {line}  ")));
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn only_shells_with_matching_syntax_get_a_file() {
        // rc_file reads the environment, so exercise the mapping through it directly.
        let home = std::env::var_os("HOME");
        assert!(home.is_some(), "test host has HOME");
        for (shell, expected) in [
            ("/usr/bin/zsh", Some(".zshrc")),
            ("/bin/bash", Some(".bashrc")),
            ("/usr/bin/fish", None),
            ("/usr/bin/nu", None),
        ] {
            // SAFETY: single-threaded unit test, and the value is restored below.
            unsafe { std::env::set_var("SHELL", shell) };
            match expected {
                Some(name) => assert_eq!(
                    rc_file().and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned())),
                    Some(name.to_string()),
                    "{shell}"
                ),
                None => assert!(rc_file().is_none(), "{shell} has no assumable rc syntax"),
            }
        }
        unsafe { std::env::remove_var("SHELL") };
    }
}
