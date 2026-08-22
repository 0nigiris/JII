//! Reading API tokens without asking anyone to leave a credential in a shell profile.
//!
//! JII used to tell people to put `export GITHUB_TOKEN=…` in `~/.bashrc`. That works, and it
//! is what half the internet says, but it is bad advice from *this* program in particular: an
//! exported variable is inherited by **every** process the user starts — including the binary
//! JII just installed from an unverified GitHub release — and `~/.bashrc` is world-readable on
//! a default Fedora account. A tool whose whole pitch is trust levels should not hand a
//! credential to everything on the machine to save one HTTP rate limit (ADR-0083).
//!
//! So a token is looked up in three places, first hit wins:
//!
//! 1. **the configured environment variable** — still fully supported. It is what CI sets, and
//!    what `GITHUB_TOKEN=… jii install …` sets for one command. Explicit and scoped.
//! 2. **`$XDG_CONFIG_HOME/jii/<var lowercased>`** — a plain one-line file next to `config.toml`
//!    that only JII reads. Nothing exports it, so nothing else inherits it.
//! 3. **a forge's own credential helper** — e.g. `gh auth token`. If the user already logged in
//!    with the GitHub CLI there is nothing to set up at all, and the secret stays in whatever
//!    store `gh` chose. The command comes from the forge (`Forge::token_command`), never from
//!    here, so the core still knows nothing about any particular source (ADR-0004).
//!
//! Nothing in this module ever *writes* a token, and nothing prints one: [`Origin`] exists so
//! `jii doctor` can say **where** a token came from without ever echoing the value.

use std::path::PathBuf;

/// Where a token was found. Carries no secret — only the provenance, for `jii doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The configured environment variable, named here so doctor can print it.
    Env(String),
    /// A file under the config directory.
    File(PathBuf),
    /// A forge's credential helper, e.g. `gh auth token`.
    Helper(String),
}

/// A resolved token plus where it came from. The value is deliberately not `Debug`-printed.
pub struct Token {
    pub value: String,
    pub origin: Origin,
}

impl std::fmt::Debug for Token {
    /// Never render the secret — a stray `{:?}` in a log is exactly the leak we're avoiding.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token").field("origin", &self.origin).finish_non_exhaustive()
    }
}

/// The file a token for `var` is read from: the config directory, `var` lowercased.
/// `GITHUB_TOKEN` → `~/.config/jii/github_token`. Source-agnostic by construction — a forge
/// configured with `CODEBERG_TOKEN` gets `~/.config/jii/codeberg_token` for free.
pub fn token_path(var: &str) -> Option<PathBuf> {
    crate::config::Config::config_dir().map(|d| d.join(var.to_ascii_lowercase()))
}

/// Read a token file: its first non-empty line, trimmed. `None` if the file is absent or holds
/// nothing usable. A read error is treated as absence — a token is an optimization, and failing
/// a whole search over an unreadable optional file would be worse than being rate-limited.
fn from_file(var: &str) -> Option<(String, PathBuf)> {
    let path = token_path(var)?;
    let body = std::fs::read_to_string(&path).ok()?;
    let line = body.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some((line.to_string(), path))
}

/// Whether `path` is readable by group or other. Used by `jii doctor` to tell the user their
/// token file is exposed; the token is still *used* — it is their file and their call.
#[cfg(unix)]
pub fn is_world_readable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o077 != 0)
}

#[cfg(not(unix))]
pub fn is_world_readable(_path: &std::path::Path) -> bool {
    false
}

/// Run a forge's credential helper and take its first output line. Any failure — helper not
/// installed, not logged in, non-zero exit — is simply "no token".
fn from_helper(argv: &[&str]) -> Option<String> {
    let (program, args) = argv.split_first()?;
    let out = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.to_string())
}

/// Resolve a token for `var`, consulting `helper` (a forge's credential command) last.
/// See the module docs for the order and why it is that order.
pub fn resolve(var: &str, helper: Option<&[&str]>) -> Option<Token> {
    if let Ok(v) = std::env::var(var)
        && !v.trim().is_empty()
    {
        return Some(Token { value: v.trim().to_string(), origin: Origin::Env(var.to_string()) });
    }
    if let Some((value, path)) = from_file(var) {
        return Some(Token { value, origin: Origin::File(path) });
    }
    let argv = helper?;
    let value = from_helper(argv)?;
    Some(Token { value, origin: Origin::Helper(argv.join(" ")) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_never_prints_itself() {
        let t = Token {
            value: "ghp_supersecret".to_string(),
            origin: Origin::Env("GITHUB_TOKEN".to_string()),
        };
        let rendered = format!("{t:?}");
        assert!(!rendered.contains("ghp_supersecret"), "the secret leaked into Debug: {rendered}");
        assert!(rendered.contains("GITHUB_TOKEN"), "provenance is still visible: {rendered}");
    }

    #[test]
    fn the_file_name_follows_the_env_var_and_never_the_source() {
        // Source-agnostic: the path is a pure function of the configured variable name.
        let gh = token_path("GITHUB_TOKEN").expect("test host resolves a config dir");
        assert!(gh.ends_with("github_token"), "{}", gh.display());
        let cb = token_path("CODEBERG_TOKEN").unwrap();
        assert!(cb.ends_with("codeberg_token"), "{}", cb.display());
        assert_eq!(gh.parent(), cb.parent(), "both live beside config.toml");
    }

    #[test]
    fn a_helper_that_fails_or_is_missing_is_just_no_token() {
        assert!(from_helper(&["jii-no-such-helper-xyz", "token"]).is_none());
        assert!(from_helper(&["false"]).is_none(), "non-zero exit yields nothing");
        assert!(from_helper(&[]).is_none(), "an empty argv is not a command");
    }

    #[test]
    fn a_helper_supplies_its_first_non_empty_line() {
        // `echo` is POSIX and present on every host we build on.
        let got = from_helper(&["sh", "-c", "printf '\\n  \\n ghp_from_helper \\nextra\\n'"]);
        assert_eq!(got.as_deref(), Some("ghp_from_helper"));
    }
}
