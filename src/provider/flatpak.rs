// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Flatpak provider.
//!
//! Uses `flatpak search --columns=…` for stable machine output. All plans use
//! `--user` (install/uninstall/update and the Flathub remote), so they run entirely in
//! user space — no root, no polkit prompt, and no dependency on a running system D-Bus
//! (which minimal/live systems may lack). Steps are therefore not marked `needs_root`.
//!
//! Flatpak packages are identified by application id (e.g. `org.gimp.GIMP`); that
//! id is used as the candidate/record `name`. (Known limitation: removing a Flatpak
//! by a friendly name like `gimp` may not resolve — see docs/TASKS.md Phase 3.)

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{
    Bootstrap, Ecosystem, Provider, command_plan, nonempty_lines, parse_installed_records,
    post_json_opt, run_capture, which,
};
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

    /// A sandboxed bundle with its own runtime.
    fn nature(&self) -> super::SourceNature {
        super::SourceNature::Sandboxed
    }

    fn highlights(&self, _candidate: &PackageCandidate) -> Vec<String> {
        vec![crate::t!("reason.flatpak_sandboxed"), crate::t!("reason.flatpak_crossdistro")]
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    fn ecosystem(&self) -> Option<Ecosystem> {
        Some(Ecosystem {
            label: "Flatpak",
            bootstrap: Bootstrap::Packages(&["flatpak"]),
        })
    }

    /// A freshly installed Flatpak has no remotes, so every install plan (which targets
    /// `flathub`) would resolve to nothing. Add it — user scope, idempotent, no root.
    async fn plan_post_bootstrap(&self) -> Result<Option<InstallPlan>> {
        let argv = [
            BIN,
            "remote-add",
            "--user",
            "--if-not-exists",
            "flathub",
            "https://flathub.org/repo/flathub.flatpakrepo",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let reasons = vec![crate::t!("reason.flatpak_add_flathub")];
        Ok(Some(command_plan(ID, "flathub", argv, false, reasons)))
    }

    /// Flatpak can be searched over the network (Flathub's web API) without the CLI, so a
    /// host without Flatpak can still be told an app exists there — the install path then
    /// offers to bootstrap Flatpak first instead of falling to GitHub (ADR-0061, part B).
    fn can_search(&self) -> bool {
        true
    }

    fn web_url(&self, candidate: &PackageCandidate) -> Option<String> {
        // The candidate name is the app-id (e.g. md.obsidian.Obsidian) → its Flathub page.
        Some(format!("https://flathub.org/apps/{}", candidate.name))
    }

    fn launch_command(&self, candidate: &PackageCandidate) -> Option<Vec<String>> {
        // A Flatpak app isn't a program on PATH — it runs through flatpak, by app-id.
        Some(vec!["flatpak".into(), "run".into(), candidate.name.clone()])
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // With the CLI present, use it — it respects the user's configured remotes and shows
        // versions. Without it, fall back to Flathub's v2 search API so an uninstalled Flatpak
        // still answers "I have this"; the candidate looks the same, so plan_install is identical.
        if which(BIN).await {
            let out = run_capture(&[
                BIN,
                "search",
                &query.raw,
                "--columns=name,application,version,branch,remotes",
            ])
            .await?;

            let rows = parse_rows(&out);
            return Ok(best_match(&query.raw, &rows)
                .map(|row| candidate_from(&row))
                .into_iter()
                .collect());
        }
        flathub_search(&query.raw).await
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let remote = candidate
            .raw
            .get("remote")
            .and_then(|v| v.as_str())
            .unwrap_or("flathub");
        let appid = &candidate.name;

        let mut reasons = vec![
            crate::t!("reason.flatpak_short"),
            crate::t!("reason.flatpak_remote", remote = remote),
        ];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }

        let mut plan = user_plan(appid, &["install", "--user", "-y", remote, appid], reasons);
        // The install targets the **user** installation, but on a typical desktop Flathub is
        // configured system-wide only — then `--user … flathub` fails with "remote not found".
        // Register the user-scope remote first (idempotent, no root); a candidate from another
        // remote (a distro's own flatpak repo) is left alone, we don't know its URL.
        if remote == "flathub" {
            plan.actions.insert(
                0,
                Action::RunCommand {
                    argv: [
                        BIN, "remote-add", "--user", "--if-not-exists", "flathub",
                        "https://flathub.org/repo/flathub.flatpakrepo",
                    ]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                    needs_root: false,
                },
            );
        }
        Ok(plan)
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "flatpak")];
        Ok(user_plan(&record.name, &["uninstall", "--user", "-y", &record.name], reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `flatpak uninstall --user -y a b c` — a user-scope op, so no polkit/root at all.
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["uninstall", "--user", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.remove_many", mgr = "flatpak", names = names.join(", "))];
        Ok(Some(user_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "flatpak")];
        Ok(user_plan(&record.name, &["update", "--user", "-y", &record.name], reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `flatpak update --user -y a b c`.
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["update", "--user", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.update_many", mgr = "flatpak", names = names.join(", "))];
        Ok(Some(user_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `flatpak update -y` with **no scope flag** updates *every* installation — both the
        // per-user apps JII installs and the system-wide ones the desktop store (KDE Discover,
        // GNOME Software) put under /var/lib/flatpak. Scoping this to `--user` (as install does,
        // to stay root-free) was a real bug: `jii update` reported success while a pile of
        // system apps stayed stale in Discover. "Update everything" has to mean everything.
        //
        // We still never run jii as root here: the system-scope portion is authorized by
        // flatpak's own polkit agent, not by us wrapping it in sudo (needs_root stays false).
        let reasons = vec![crate::t!("reason.flatpak_upgrade_all")];
        Ok(Some(user_plan("all flatpaks", &["update", "-y"], reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // List across both installations so a record installed elsewhere still resolves.
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
        popularity: None,
        suspicious: false,
        raw: json!({ "remote": remote, "branch": row.branch, "appid": row.appid }),
    }
}

/// One hit from Flathub's v2 search API (only the fields JII needs).
#[derive(Deserialize)]
struct FlathubHit {
    name: String,
    app_id: String,
}

/// The v2 search response shape: `{ "hits": [ … ] }`.
#[derive(Deserialize)]
struct FlathubSearch {
    hits: Vec<FlathubHit>,
}

/// Search Flathub's web API — used only when the flatpak CLI isn't installed. Maps each hit
/// to the same `Row` shape the CLI path produces, so ranking and `candidate_from` (and thus
/// `plan_install`) are identical: installing then bootstraps Flatpak and runs `flatpak install`.
async fn flathub_search(query: &str) -> Result<Vec<PackageCandidate>> {
    let body = json!({ "query": query });
    let Some(resp) =
        post_json_opt::<_, FlathubSearch>(ID, "https://flathub.org/api/v2/search", &body).await?
    else {
        return Ok(Vec::new());
    };
    let rows: Vec<Row> = resp
        .hits
        .into_iter()
        .map(|h| Row {
            name: h.name,
            appid: h.app_id,
            version: String::new(),
            branch: "stable".to_string(),
            remotes: vec!["flathub".to_string()],
        })
        .collect();
    Ok(best_match(query, &rows)
        .map(|row| candidate_from(&row))
        .into_iter()
        .collect())
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
    async fn install_plan_registers_the_user_flathub_remote_first() {
        // Flathub configured system-wide only would fail a `--user` install; the plan
        // registers the user-scope remote first (idempotent, unprivileged).
        let rows = parse_rows(SAMPLE);
        let c = candidate_from(&best_match("gimp", &rows).unwrap());
        let plan = Flatpak::new().plan_install(&c).await.unwrap();
        assert!(!plan.needs_root());
        assert_eq!(plan.actions.len(), 2);
        match &plan.actions[0] {
            Action::RunCommand { argv, .. } => {
                assert_eq!(argv[..5], ["flatpak", "remote-add", "--user", "--if-not-exists", "flathub"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
        match &plan.actions[1] {
            Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["flatpak", "install", "--user", "-y", "flathub", "org.gimp.GIMP"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
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
                assert_eq!(
                    argv,
                    &["flatpak", "update", "--user", "-y", "org.gimp.GIMP", "org.videolan.VLC"]
                );
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
                // No `--user`: update-all must cover system-wide apps too (the Discover bug),
                // and still without jii escalating (flatpak's polkit handles the system part).
                assert_eq!(argv, &["flatpak", "update", "-y"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }
}
