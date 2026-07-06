//! Flatpak provider.
//!
//! Uses `flatpak search --columns=…` for stable machine output. Flatpak performs
//! its own privilege handling (polkit) for system installs, so its steps are not
//! marked `needs_root` — JII does not wrap them in sudo/pkexec.
//!
//! Flatpak packages are identified by application id (e.g. `org.gimp.GIMP`); that
//! id is used as the candidate/record `name`. (Known limitation: removing a Flatpak
//! by a friendly name like `gimp` may not resolve — see docs/TASKS.md Phase 3.)

use async_trait::async_trait;
use serde_json::json;

use super::{Provider, nonempty_lines, parse_installed_records, run_capture, which};
use crate::error::Result;
use crate::model::{
    Action, InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
};

const BIN: &str = "flatpak";
const ID: &str = "flatpak";

/// The Flatpak installation source.
pub struct Flatpak;

impl Flatpak {
    pub fn new() -> Self {
        Flatpak
    }
}

impl Default for Flatpak {
    fn default() -> Self {
        Flatpak::new()
    }
}

#[async_trait]
impl Provider for Flatpak {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // Flathub is community-maintained (not distro-official), but sandboxed.
        TrustLevel::Community
    }

    fn highlights(&self, _candidate: &PackageCandidate) -> Vec<String> {
        vec![
            "Sandboxed application".to_string(),
            "Cross-distro (Flatpak/Flathub), no root".to_string(),
        ]
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let out = run_capture(&[
            BIN,
            "search",
            &query.raw,
            "--columns=name,application,version,branch,remotes",
        ])
        .await?;

        let rows = parse_rows(&out);
        Ok(best_match(&query.raw, &rows)
            .map(|row| candidate_from(&row))
            .into_iter()
            .collect())
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let remote = candidate
            .raw
            .get("remote")
            .and_then(|v| v.as_str())
            .unwrap_or("flathub");
        let appid = &candidate.name;

        let mut reasons = vec!["Flatpak (sandboxed)".to_string(), format!("Remote: {remote}")];
        if let Some(v) = &candidate.version {
            reasons.push(format!("Version {v}"));
        }

        Ok(user_plan(appid, &["install", "-y", remote, appid], reasons))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![format!("Remove {} (installed via flatpak)", record.name)];
        Ok(user_plan(&record.name, &["uninstall", "-y", &record.name], reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `flatpak uninstall -y a b c` (flatpak handles its own polkit — no JII root).
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["uninstall", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![format!("Remove (via flatpak): {}", names.join(", "))];
        Ok(Some(user_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![format!("Update {} via flatpak", record.name)];
        Ok(user_plan(&record.name, &["update", "-y", &record.name], reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `flatpak update -y a b c`.
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["update", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![format!("Update (via flatpak): {}", names.join(", "))];
        Ok(Some(user_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `flatpak update -y` with no refs = update every installed app/runtime (D10).
        let reasons = vec!["Update all Flatpak apps and runtimes".to_string()];
        Ok(Some(user_plan("all flatpaks", &["update", "-y"], reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        let out = run_capture(&[BIN, "list", "--app", "--columns=application,version"]).await?;
        Ok(parse_installed_records(&out, self.id()))
    }
}

/// A parsed `flatpak search` row.
#[derive(Debug, Clone)]
struct Row {
    name: String,
    appid: String,
    version: String,
    branch: String,
    remotes: Vec<String>,
}

/// Build a single-command Flatpak plan. The command is not marked root — Flatpak
/// handles its own elevation via polkit.
fn user_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    InstallPlan {
        candidate_ref: name.to_string(),
        source_id: ID.to_string(),
        actions: vec![Action::RunCommand {
            argv,
            needs_root: false,
        }],
        download_size: None,
        reasons,
    }
}

/// Convert a matched row into a candidate (identified by appid).
fn candidate_from(row: &Row) -> PackageCandidate {
    let remote = choose_remote(&row.remotes);
    PackageCandidate {
        name: row.appid.clone(),
        source_id: ID.to_string(),
        version: (!row.version.is_empty()).then(|| PkgVersion::new(&row.version)),
        trust: TrustLevel::Community,
        arch_ok: true,
        signed: true,
        summary: (!row.name.is_empty()).then(|| row.name.clone()),
        raw: json!({ "remote": remote, "branch": row.branch, "appid": row.appid }),
    }
}

/// Parse `flatpak search` output. Columns: `name<TAB>appid<TAB>version<TAB>branch<TAB>remotes`.
/// Lines without an appid (e.g. "No matches found") are skipped.
fn parse_rows(stdout: &str) -> Vec<Row> {
    nonempty_lines(stdout)
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\t');
            let name = fields.next()?.trim().to_string();
            let appid = fields.next().unwrap_or("").trim().to_string();
            if appid.is_empty() {
                return None;
            }
            let version = fields.next().unwrap_or("").trim().to_string();
            let branch = fields.next().unwrap_or("").trim().to_string();
            let remotes = fields
                .next()
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            Some(Row {
                name,
                appid,
                version,
                branch,
                remotes,
            })
        })
        .collect()
}

/// Pick the best row for a query. Prefers exact name / appid-tail matches over
/// substring matches; returns `None` if nothing matches.
fn best_match(query: &str, rows: &[Row]) -> Option<Row> {
    rows.iter()
        .filter_map(|row| match_score(query, row).map(|score| (score, row)))
        .min_by_key(|(score, _)| *score)
        .map(|(_, row)| row.clone())
}

/// Lower score = better match. `None` = no match.
fn match_score(query: &str, row: &Row) -> Option<u8> {
    let q = query.to_ascii_lowercase();
    let name = row.name.to_ascii_lowercase();
    let appid = row.appid.to_ascii_lowercase();
    let tail = appid.rsplit('.').next().unwrap_or(&appid);

    if name == q || tail == q {
        Some(0)
    } else if appid == q {
        Some(1)
    } else if tail.contains(&q) {
        Some(2)
    } else if name.contains(&q) {
        Some(3)
    } else if appid.contains(&q) {
        Some(4)
    } else {
        None
    }
}

/// Prefer flathub, else the first available remote.
fn choose_remote(remotes: &[String]) -> String {
    if remotes.iter().any(|r| r == "flathub") {
        "flathub".to_string()
    } else {
        remotes.first().cloned().unwrap_or_else(|| "flathub".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "GIMP User Manual\torg.gimp.GIMP.Manual\t2.10\t2.10\tflathub\n\
GNU Image Manipulation Program\torg.gimp.GIMP\t3.2.4\tstable\tfedora,flathub\n\
Resynthesizer\torg.gimp.GIMP.Plugin.Resynthesizer\t3.0.1\t3\tflathub\n";

    #[test]
    fn parses_rows_and_remotes() {
        let rows = parse_rows(SAMPLE);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].appid, "org.gimp.GIMP");
        assert_eq!(rows[1].version, "3.2.4");
        assert_eq!(rows[1].remotes, vec!["fedora", "flathub"]);
    }

    #[test]
    fn skips_no_matches_line() {
        assert!(parse_rows("No matches found\n").is_empty());
    }

    #[test]
    fn best_match_prefers_the_app_over_plugins_and_manual() {
        let rows = parse_rows(SAMPLE);
        let best = best_match("gimp", &rows).unwrap();
        assert_eq!(best.appid, "org.gimp.GIMP");
    }

    #[test]
    fn best_match_none_when_unrelated() {
        let rows = parse_rows(SAMPLE);
        assert!(best_match("firefox", &rows).is_none());
    }

    #[test]
    fn candidate_uses_appid_as_name_and_prefers_flathub() {
        let rows = parse_rows(SAMPLE);
        let c = candidate_from(&best_match("gimp", &rows).unwrap());
        assert_eq!(c.name, "org.gimp.GIMP");
        assert_eq!(c.source_id, "flatpak");
        assert_eq!(c.trust, TrustLevel::Community);
        assert_eq!(c.raw.get("remote").unwrap().as_str(), Some("flathub"));
    }

    #[tokio::test]
    async fn batch_update_merges_into_one_unprivileged_flatpak_command() {
        let mk = |name: &str| InstalledRecord {
            name: name.to_string(),
            source_id: "flatpak".into(),
            version: None,
            installed_at: chrono::Utc::now(),
            verification: None,
        };
        let (a, b) = (mk("org.gimp.GIMP"), mk("org.videolan.VLC"));
        let plan = Flatpak::new()
            .plan_update_many(&[&a, &b])
            .await
            .unwrap()
            .expect("flatpak batches");
        assert!(!plan.needs_root());
        match &plan.actions[0] {
            Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["flatpak", "update", "-y", "org.gimp.GIMP", "org.videolan.VLC"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn update_all_updates_every_flatpak_unprivileged() {
        let plan = Flatpak::new()
            .plan_update_all()
            .await
            .unwrap()
            .expect("flatpak offers a system update");
        assert!(!plan.needs_root());
        match &plan.actions[0] {
            Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["flatpak", "update", "-y"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }
}
