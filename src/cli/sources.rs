//! `jii sources` — showing, enabling, disabling, adding and removing installation sources.
//!
//! Enabling and disabling are config edits. *Adding* and *removing* are not: `jii sources
//! remove flatpak` means uninstalling the package manager itself, which is a system change
//! made with the distro's own tool and shown as an exact command before it runs. Nix is the
//! awkward one — it is configured by a file under `/etc`, so writing it is staged through a
//! backup and a root move rather than an in-place edit (ADR-0072).

use super::{Cli, SysManager};
use crate::config::Config;
use crate::engine::Engine;
use crate::ui::{Renderer, prompt};

/// Back up `path` to `<path>.jii-bak` and overwrite it with `content`, returning the backup
/// path (Nix Etap B, ADR-0056). Used for a **user-owned** config (`needs_root == false`), so
/// this is plain user-space file IO — no privilege escalation (the root-owned case goes through
/// [`write_nix_config_root`]). The backup is written first, so a failed write always leaves a
/// recoverable copy.
pub(super) fn write_nix_config(path: &std::path::Path, content: &str) -> std::io::Result<std::path::PathBuf> {
    let backup = jii_backup_path(path);
    std::fs::copy(path, &backup)?;
    std::fs::write(path, content)?;
    Ok(backup)
}
/// The `<path>.jii-bak` sibling of a config file (its pre-edit backup).
fn jii_backup_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut backup = path.as_os_str().to_owned();
    backup.push(".jii-bak");
    std::path::PathBuf::from(backup)
}
/// The two elevated commands that back up and then overwrite a root-owned config `path`, with
/// the session's `sudo`/`pkexec` prefix already applied (Nix Etap C, ADR-0058). Returned as
/// `(backup_cmd, write_cmd)` so the CLI can *show them verbatim* before running anything. The
/// write command copies from a placeholder temp path (`{tmp}`) that [`write_nix_config_root`]
/// fills in — the shown form uses the same real temp path it will run.
pub(super) fn root_write_argv(
    privilege: &crate::privilege::Privilege,
    path: &std::path::Path,
) -> (Vec<String>, Vec<String>) {
    let dest = path.display().to_string();
    let backup = jii_backup_path(path).display().to_string();
    let tmp = root_tmp_path(path).display().to_string();
    let backup_cmd = privilege.elevated_argv(
        &["cp".into(), "-a".into(), "--".into(), dest.clone(), backup],
        true,
    );
    let write_cmd =
        privilege.elevated_argv(&["cp".into(), "--".into(), tmp, dest], true);
    (backup_cmd, write_cmd)
}
/// A per-process temp path (in the system temp dir) used to stage a root config's new contents
/// before `sudo cp` moves it into place. Deterministic for one `path` within a run so
/// [`root_write_argv`] and [`write_nix_config_root`] agree; created with `O_EXCL` at write time.
fn root_tmp_path(path: &std::path::Path) -> std::path::PathBuf {
    let stem = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".into());
    std::env::temp_dir().join(format!("jii-nixedit-{}-{stem}", std::process::id()))
}
/// Write a root-owned Nix config via the privilege path (Etap C, ADR-0058): stage `content` in a
/// user-owned temp file (`O_EXCL`, so an existing path/symlink can't be clobbered), prime the
/// escalation once, then run the pre-built `backup_cmd` and `write_cmd` (exactly what the CLI
/// already showed). Returns the backup path on success. JII never runs fully as root — only these
/// two concrete `cp` steps escalate, through `privilege.rs`.
pub(super) async fn write_nix_config_root(
    privilege: &crate::privilege::Privilege,
    path: &std::path::Path,
    content: &str,
    backup_cmd: &[String],
    write_cmd: &[String],
) -> std::result::Result<std::path::PathBuf, String> {
    use std::io::Write;
    let tmp = root_tmp_path(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    let staged = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(e) = staged {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    drop(file);
    privilege.prime().await.map_err(|e| e.to_string())?;
    let run = async {
        privilege.run(backup_cmd, true).await.map_err(|e| e.to_string())?;
        privilege.run(write_cmd, true).await.map_err(|e| e.to_string())
    }
    .await;
    let _ = std::fs::remove_file(&tmp);
    run?;
    Ok(jii_backup_path(path))
}
/// Detect the host's system package manager, in the order distros ship them. `xbps` is
/// detected via `xbps-install` (the remove tool `xbps-remove` ships in the same package).
async fn detect_system_manager() -> Option<SysManager> {
    use crate::provider::which;
    if which("dnf5").await {
        Some(SysManager::Dnf("dnf5"))
    } else if which("dnf").await {
        Some(SysManager::Dnf("dnf"))
    } else if which("apt-get").await {
        Some(SysManager::Apt)
    } else if which("pacman").await {
        Some(SysManager::Pacman)
    } else if which("zypper").await {
        Some(SysManager::Zypper)
    } else if which("xbps-install").await {
        Some(SysManager::Xbps)
    } else if which("emerge").await {
        Some(SysManager::Portage)
    } else {
        None
    }
}

impl Cli {
    /// Sources path: list enabled providers and whether each is usable on this machine.
    /// Native managers for other distros (pacman on Fedora) are hidden unless `all` — a user
    /// shouldn't have to reason about a package manager their system doesn't have.
    pub(super) async fn sources(
        &self,
        all: bool,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let full = engine.source_catalog().await;
        // A disabled source is dropped when the provider registry is built, so it is absent from
        // the catalog entirely — read the ids from config, or a source you turned off would be
        // invisible here and there'd be nothing to tell you how to turn it back on.
        let disabled = engine.config().sources.disabled.clone();
        // Which sources are ecosystem *managers* (bootstrappable/removable), by id → is it a
        // pure script install (brew/nix/AUR helper)? System repos aren't here, so they get no
        // add/remove hint — you don't uninstall the package manager your OS is built on.
        let managers: std::collections::HashMap<&str, bool> = engine
            .ecosystem_catalog()
            .await
            .into_iter()
            .map(|e| (e.id, matches!(e.bootstrap, crate::provider::Bootstrap::Script { .. })))
            .collect();
        let hidden = full.iter().filter(|e| !e.relevant).count();
        let shown: Vec<&crate::engine::SourceEntry> =
            full.iter().filter(|e| all || e.relevant).collect();

        if renderer.is_json() {
            let mut rows: Vec<_> = shown
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id, "trust": e.trust.label(),
                        "available": e.available, "relevant": e.relevant,
                        "enabled": true,
                        "manager": managers.contains_key(e.id),
                    })
                })
                .collect();
            // Disabled sources carry the same key set as enabled ones (a stable schema for
            // tooling); what a dropped provider can't report is an explicit null.
            rows.extend(disabled.iter().map(|id| {
                serde_json::json!({
                    "id": id, "trust": serde_json::Value::Null,
                    "available": false, "relevant": serde_json::Value::Null,
                    "enabled": false,
                    "manager": serde_json::Value::Null,
                })
            }));
            renderer.json_value(&serde_json::json!(rows));
            return Ok(());
        }

        let palette = renderer.palette();
        type Row<'a> = &'a crate::engine::SourceEntry;
        // A right-hand hint for an ecosystem manager: `[remove: …]` when installed,
        // `[add: …]` when not. Empty for system repos (not in `managers`).
        let hint = |e: &crate::engine::SourceEntry| -> String {
            if !managers.contains_key(e.id) {
                return String::new();
            }
            let key = if e.available { "sources.remove_hint" } else { "sources.add_hint2" };
            format!("  {}", palette.dim(&crate::t!(key, id = e.id)))
        };
        let (active, inactive): (Vec<Row>, Vec<Row>) =
            shown.into_iter().partition(|e| e.available);
        if !active.is_empty() {
            renderer.heading(&crate::t!("sources.active"));
            for e in &active {
                let mark = palette.good(palette.mark_ok());
                renderer.info(&format!(
                    "  {mark} {} ({}){}",
                    palette.source(&format!("{:8}", e.id)),
                    palette.trust(e.trust),
                    hint(e),
                ));
            }
        }
        if !inactive.is_empty() {
            renderer.heading(&crate::t!("sources.unavailable"));
            for e in &inactive {
                renderer.info(&format!(
                    "  {}{}",
                    palette.dim(&format!("{} {:8} ({})", palette.mark_bad(), e.id, e.trust.display())),
                    hint(e),
                ));
            }
        }
        // Sources you turned off yourself. Listed from config (they're absent from the catalog),
        // each with the exact command that brings it back.
        if !disabled.is_empty() {
            renderer.heading(&crate::t!("sources.disabled_header"));
            for id in &disabled {
                renderer.info(&format!(
                    "  {}  {}",
                    palette.dim(&format!("{} {id:8}", palette.mark_bad())),
                    palette.dim(&crate::t!("sources.enable_hint", id = id.clone())),
                ));
            }
        }
        // Nudge that some sources were hidden, so `--all` is discoverable.
        if !all && hidden > 0 {
            renderer.info("");
            renderer.info(&palette.dim(&crate::t!("sources.hidden", count = hidden)));
        }
        // Turning a source off is the one thing this view could never tell you about (the ask:
        // "how do I disable a repository?" — the answer existed, nothing pointed at it).
        renderer.info("");
        renderer.info(&palette.dim(&crate::t!("sources.toggle_hint")));
        Ok(())
    }

    /// `jii sources disable|enable <id>` — flip a source's `[sources] disabled` entry and save.
    /// A disabled source is dropped when the provider registry is built, so JII stops searching
    /// it everywhere at once. The id is validated so a typo fails loudly instead of silently.
    pub(super) fn sources_set_enabled(
        &self,
        id: &str,
        enable: bool,
        mut config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        if !crate::config::KNOWN_SOURCES.contains(&id) {
            renderer.error(&crate::t!("sources.unknown", id = id));
            renderer.info(&crate::t!(
                "sources.known",
                list = crate::config::KNOWN_SOURCES.join(", ")
            ));
            return Ok(());
        }
        let currently_enabled = config.is_enabled(id);
        if enable == currently_enabled {
            let key = if enable { "sources.already_enabled" } else { "sources.already_disabled" };
            renderer.info(&crate::t!(key, id = id));
            return Ok(());
        }
        if enable {
            config.sources.disabled.retain(|s| s != id);
        } else {
            config.sources.disabled.push(id.to_string());
        }
        config.save()?;
        let key = if enable { "sources.enabled" } else { "sources.disabled_ok" };
        renderer.success(&crate::t!(key, id = id));
        Ok(())
    }

    /// `jii sources add <id>` — bootstrap a missing ecosystem manager. Two honest paths, no
    /// magic: a manager that lives in the distro repos (npm, cargo, go, pipx, flatpak, snap) is
    /// resolved cross-distro (`nodejs-npm` on Fedora, `npm` elsewhere) and handed to the
    /// **normal install path** — same preview → confirm → execute → record as any package. A
    /// manager that bootstraps via its own upstream script (Homebrew, Nix, an AUR helper) is
    /// **shown, never run** — JII does not pipe an installer into your shell (ADR-0005/0006).
    pub(super) async fn sources_add(
        &self,
        id: &str,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        // `jii sources add yay|paru` — an AUR helper, only meaningful on Arch.
        if matches!(id, "yay" | "paru") {
            return self.add_aur_helper(id, renderer).await;
        }
        let engine = Engine::new(config.clone())?;
        let catalog = engine.ecosystem_catalog().await;

        let Some(eco) = catalog.iter().find(|e| e.id == id) else {
            renderer.error(&crate::t!("providers.unknown", name = id));
            let known: Vec<_> = catalog.iter().map(|e| e.id).collect();
            renderer.info(&crate::t!("providers.known", names = known.join(", ")));
            return Ok(());
        };
        self.bootstrap_ecosystem(&engine, eco, config, renderer).await
    }

    /// `jii sources add yay|paru` — an AUR helper is built from a PKGBUILD via `makepkg`, which
    /// JII will **show, never run** (the same trust boundary as the brew/nix installer scripts).
    /// If a helper is already present, say so. Arch-only: refuse elsewhere with a clear note.
    pub(super) async fn add_aur_helper(&self, helper: &str, renderer: &Renderer) -> crate::error::Result<()> {
        if !crate::platform::Platform::detect().arch_like {
            renderer.error(&crate::t!("aur.not_arch", helper = helper.to_string()));
            return Ok(());
        }
        if crate::provider::which(helper).await {
            renderer.success(&crate::t!("aur.helper_present", helper = helper.to_string()));
            return Ok(());
        }
        renderer.info(&crate::t!("aur.helper_intro", helper = helper.to_string()));
        renderer.info(&format!(
            "  git clone https://aur.archlinux.org/{helper}-bin.git",
        ));
        renderer.info(&format!("  cd {helper}-bin && makepkg -si"));
        Ok(())
    }

    /// `jii sources remove <id>` — uninstall an ecosystem manager from the system. Refuses the
    /// system package managers (dnf/apt/pacman…): removing them would break the OS. A
    /// script-installed manager (Homebrew, Nix) can't be auto-removed — its own uninstall is
    /// pointed to. A repo-provided manager (flatpak/snap/cargo/pipx/go) is removed through the
    /// host's system package manager, with the **exact elevated command shown first** and a
    /// default-no confirmation. AUR helpers (yay/paru) are removed via pacman.
    pub(super) async fn sources_remove(
        &self,
        id: &str,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        // AUR helpers are ordinary pacman packages — remove them directly.
        if matches!(id, "yay" | "paru") {
            return self.remove_via_pacman(id, renderer).await;
        }
        let engine = Engine::new(config.clone())?;
        let catalog = engine.ecosystem_catalog().await;

        let Some(eco) = catalog.iter().find(|e| e.id == id) else {
            // Not an ecosystem manager. If it's a *known* source, it's a system manager we
            // refuse to uninstall; otherwise it's just an unknown id.
            if crate::config::KNOWN_SOURCES.contains(&id) {
                renderer.error(&crate::t!("sources.remove_system", id = id));
            } else {
                renderer.error(&crate::t!("sources.unknown", id = id));
                let known: Vec<_> = catalog.iter().map(|e| e.id).collect();
                renderer.info(&crate::t!("providers.known", names = known.join(", ")));
            }
            return Ok(());
        };
        if !eco.installed {
            renderer.info(&crate::t!("sources.remove_not_installed", label = eco.label));
            return Ok(());
        }
        let names = match eco.bootstrap {
            // A script-installed manager (brew/nix/AUR): JII can't cleanly uninstall it.
            crate::provider::Bootstrap::Script { .. } => {
                renderer.info(&crate::t!("sources.remove_script", label = eco.label));
                return Ok(());
            }
            crate::provider::Bootstrap::Packages(names) => names,
        };
        let Some(mgr) = detect_system_manager().await else {
            renderer.error(&crate::t!("sources.remove_no_system_mgr", label = eco.label));
            return Ok(());
        };
        // Only remove the package(s) actually installed on this host, so we never guess-remove
        // a wrong name (go is `golang` on Fedora, `go` on Arch, `golang-go` on Debian).
        let mut installed = Vec::new();
        for n in names {
            if mgr.pkg_installed(n).await {
                installed.push((*n).to_string());
            }
        }
        if installed.is_empty() {
            renderer.warn(&crate::t!(
                "sources.remove_unknown_pkg",
                label = eco.label,
                names = names.join(", ")
            ));
            return Ok(());
        }
        self.run_system_remove(&mgr, &installed, eco.label, config, renderer).await
    }

    /// Remove an AUR helper (yay/paru) via pacman — the exact elevated command shown first,
    /// default-no confirmation. Arch-only; a no-op with a note if it isn't installed.
    async fn remove_via_pacman(
        &self,
        helper: &str,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        if !crate::provider::which(helper).await {
            renderer.info(&crate::t!("sources.remove_not_installed", label = helper.to_string()));
            return Ok(());
        }
        let argv = vec![
            "pacman".to_string(),
            "-Rs".to_string(),
            "--noconfirm".to_string(),
            helper.to_string(),
        ];
        self.confirm_and_run_removal(&argv, true, helper, renderer).await
    }

    /// Show the elevated system-manager removal command, confirm (default no), then run it.
    async fn run_system_remove(
        &self,
        mgr: &SysManager,
        pkgs: &[String],
        label: &str,
        _config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let (argv, needs_root) = mgr.remove_argv(pkgs);
        self.confirm_and_run_removal(&argv, needs_root, label, renderer).await
    }

    /// Shared tail of every manager removal: print the exact elevated command, honour
    /// `--dry-run`, ask a default-no confirmation, then run it through the privilege layer.
    async fn confirm_and_run_removal(
        &self,
        argv: &[String],
        needs_root: bool,
        label: &str,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let privilege = crate::privilege::Privilege::detect();
        let shown = privilege.elevated_argv(argv, needs_root);
        renderer.warn(&crate::t!("sources.remove_confirm", label = label.to_string()));
        renderer.info(&format!("  {}", shown.join(" ")));
        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_unchanged"));
            return Ok(());
        }
        let flags = self.prompt_flags(false);
        if !prompt::confirm(renderer, &crate::t!("sources.remove_prompt"), false, &flags) {
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }
        privilege.prime().await?;
        match privilege.run(argv, needs_root).await {
            Ok(()) => renderer.success(&crate::t!("sources.removed", label = label.to_string())),
            Err(e) => renderer.error(&crate::t!(
                "sources.remove_failed",
                label = label.to_string(),
                error = e.to_string()
            )),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ColorChoice;

    #[test]
    fn write_nix_config_backs_up_then_overwrites() {
        // Nix Etap B: the original is preserved at <path>.jii-bak before the new content lands.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("home.nix");
        std::fs::write(&cfg, "old\n").unwrap();
        let backup = write_nix_config(&cfg, "new\n").unwrap();
        assert_eq!(backup, cfg.with_file_name("home.nix.jii-bak"));
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "new\n");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "old\n");
    }

    #[tokio::test]
    async fn dry_run_root_edit_never_writes_and_stages_nothing() {
        use clap::Parser;
        // Etap C: a root-owned config under `--dry-run` shows the sudo commands but must not
        // write, back up, or even stage a temp file.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("configuration.nix");
        std::fs::write(&cfg, "original\n").unwrap();

        let kind = crate::model::StrategyKind::EditFile {
            path: cfg.clone(),
            new_content: "edited\n".into(),
            diff: "+ edited".into(),
            apply: "sudo nixos-rebuild switch".into(),
            needs_root: true,
        };
        let cli = Cli::parse_from(["jii", "-d", "install", "foo"]);
        let renderer =
            Renderer::new(ColorChoice::Never, false, crate::config::OutputMode::Friendly);
        cli.apply_edit_file(false, &kind, false, &renderer).await;

        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "original\n");
        assert!(!cfg.with_file_name("configuration.nix.jii-bak").exists());
        assert!(!root_tmp_path(&cfg).exists());
    }

    #[test]
    fn root_write_argv_shows_backup_then_write_with_elevation() {
        // The exact elevated commands JII prints (and later runs) for a root-owned config.
        let priv_ = crate::privilege::Privilege::detect();
        let path = std::path::Path::new("/etc/nixos/configuration.nix");
        let (backup_cmd, write_cmd) = root_write_argv(&priv_, path);
        // Both back up to <path>.jii-bak and copy the staged temp into place.
        assert!(backup_cmd.iter().any(|a| a == "/etc/nixos/configuration.nix.jii-bak"));
        assert!(backup_cmd.iter().any(|a| a == "cp"));
        assert!(write_cmd.iter().any(|a| a == "/etc/nixos/configuration.nix"));
        assert!(write_cmd.contains(&root_tmp_path(path).display().to_string()));
    }
}
