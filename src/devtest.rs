//! The hidden tester checklist: `jii yes-I-am-dev-and-want-to-test`.
//!
//! Built for **external testers** running JII in a fresh VM (see `docs/TESTING.md`; the
//! command is deliberately absent from `--help` and the README). It walks a scripted
//! checklist of *real* JII invocations — real installs included — showing each command's
//! live output, asking the tester "did this look right?" after every step, and duplicating
//! **everything** (commands, full output, exit codes, verdicts) into one `.log` file.
//!
//! At the end it can upload the log to a public paste service in one keypress (username and
//! hostname are scrubbed first; the local file always stays), and prints a pre-filled
//! GitHub-issue link carrying the system info, the log URL and the PASS/FAIL summary — so
//! reporting a broken run takes one click instead of screenshots.
//!
//! Everything here is English on purpose: the artifact travels to the issue tracker.

use std::io::Write as _;
use std::path::PathBuf;

use tokio::io::AsyncReadExt;

use crate::error::{JiiError, Result};

/// One checklist step: a human title, the `jii` arguments to run, and what the tester
/// should expect to see (shown before the run so "looks right?" has a yardstick).
struct Step {
    title: &'static str,
    args: &'static [&'static str],
    expect: &'static str,
}

/// The scripted checklist. Every step avoids JII's interactive chooser (explicit `-y`,
/// `--no`, or pinned specs) because the child's stdout is piped for capture — its y/n
/// prompts still work (stdin is inherited), but a full-screen menu could not redraw.
/// Steps 10–11 are *expected* to end in a clear error: that is the behavior under test.
fn checklist() -> Vec<Step> {
    vec![
        Step { title: "Version banner", args: &["--version"], expect: "The version prints and matches the release you installed." },
        Step { title: "Doctor (read-only)", args: &["doctor", "--no"], expect: "Source table + system checks; no question is asked, nothing is changed." },
        Step { title: "Search with junk heuristics", args: &["search", "htop"], expect: "dnf/apt/… first; obscure registry squatters (pipx/cargo 'htop') shown red untrusted." },
        Step { title: "Info card", args: &["info", "htop"], expect: "A card with description, source list and a recommendation." },
        Step { title: "REAL install", args: &["htop", "-y"], expect: "htop installs through your system manager (sudo may prompt). Friendly one-line preview, then success." },
        Step { title: "List installs", args: &["list"], expect: "htop appears in the table with its source and version." },
        Step { title: "Explain an install", args: &["how", "htop"], expect: "When/whence htop was installed, its version and trust." },
        Step { title: "Update one package", args: &["update", "htop", "-y"], expect: "Either 'already up to date' or a clean in-place update." },
        Step { title: "REAL removal", args: &["remove", "htop", "-y"], expect: "htop is removed via the same source that installed it." },
        Step { title: "Dead-end UX (not found)", args: &["totally-nonexistent-xyz321", "--no"], expect: "A clear 'not found' with browse links — never a bare dead end, never a crash." },
        Step { title: "Version-pin rejection", args: &["npm@1.0", "--no"], expect: "A clear 'version pins not supported yet' error — the pin must NOT be silently ignored." },
        Step { title: "Sources view", args: &["sources"], expect: "Active/unavailable sources for THIS machine; irrelevant distro managers hidden." },
    ]
}

/// A step's outcome as judged by the tester.
#[derive(Clone, Copy, PartialEq)]
enum Verdict {
    Pass,
    Fail,
    Skip,
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::Skip => "SKIP",
        }
    }
}

struct StepResult {
    title: &'static str,
    exit: Option<i32>,
    verdict: Verdict,
    note: String,
}

/// The log sink: mirrors everything to a file, scrubbing the username and hostname so the
/// artifact is safe to paste publicly. The console keeps the raw (unscrubbed) text.
struct TestLog {
    path: PathBuf,
    file: std::fs::File,
    scrub: Vec<(String, &'static str)>,
}

impl TestLog {
    fn create() -> Result<TestLog> {
        let name = format!("jii-test-{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        // Prefer the current directory; fall back to $HOME if it is not writable.
        let path = match std::fs::File::create(&name) {
            Ok(f) => return Ok(TestLog { path: PathBuf::from(name), file: f, scrub: scrub_pairs() }),
            Err(_) => directories::BaseDirs::new()
                .map(|b| b.home_dir().join(&name))
                .ok_or_else(|| JiiError::Other(anyhow::anyhow!("cannot create the log file")))?,
        };
        let file = std::fs::File::create(&path).map_err(|e| JiiError::io(&path, e))?;
        Ok(TestLog { path, file, scrub: scrub_pairs() })
    }

    /// Append raw text to the log (scrubbed); the caller prints to the console itself.
    fn log(&mut self, text: &str) {
        let mut scrubbed = text.to_string();
        for (needle, replacement) in &self.scrub {
            scrubbed = scrubbed.replace(needle, replacement);
        }
        let _ = self.file.write_all(scrubbed.as_bytes());
    }

    /// Print a line to the console **and** the log.
    fn say(&mut self, line: &str) {
        println!("{line}");
        self.log(&format!("{line}\n"));
    }
}

/// What to scrub from the public log: the username and the hostname. Longest first, so a
/// hostname containing the username still scrubs cleanly.
fn scrub_pairs() -> Vec<(String, &'static str)> {
    let mut pairs: Vec<(String, &'static str)> = Vec::new();
    if let Ok(host) = std::fs::read_to_string("/etc/hostname") {
        let host = host.trim().to_string();
        if host.len() > 1 {
            pairs.push((host, "HOST"));
        }
    }
    for var in ["USER", "LOGNAME"] {
        if let Ok(user) = std::env::var(var) {
            if user.len() > 1 && !pairs.iter().any(|(n, _)| *n == user) {
                pairs.push((user, "USER"));
            }
        }
    }
    pairs.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
    pairs
}

/// Host facts for the report header / issue body.
fn host_summary() -> String {
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_string()))
        })
        .unwrap_or_else(|| "unknown OS".to_string());
    format!("{os} · {} · jii {}", std::env::consts::ARCH, env!("CARGO_PKG_VERSION"))
}

/// Ask a one-line question on the tester's terminal (plain stdin — the checklist runs on a
/// real TTY by definition; a non-TTY run takes the default).
fn ask(question: &str, default: &str) -> String {
    if !crate::platform::Platform::detect().is_tty {
        return default.to_string();
    }
    print!("{question} ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return default.to_string();
    }
    let ans = line.trim().to_string();
    if ans.is_empty() { default.to_string() } else { ans }
}

/// Run one child `jii` invocation with stdout+stderr piped (forwarded live to the console
/// and into the log) and stdin inherited, so sudo/y-n prompts still reach the tester.
/// Returns the exit code (None if terminated by a signal).
async fn run_step(log: &mut TestLog, args: &[&str]) -> Result<Option<i32>> {
    let exe = std::env::current_exe().map_err(|e| JiiError::io("current executable", e))?;
    let mut child = tokio::process::Command::new(&exe)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| JiiError::spawn("jii", e))?;

    let mut out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    let mut captured = String::new();
    let mut buf_out = [0u8; 4096];
    let mut buf_err = [0u8; 4096];
    let mut out_open = true;
    let mut err_open = true;
    // Forward chunks as they arrive (no line buffering — a prompt has no trailing newline).
    while out_open || err_open {
        tokio::select! {
            n = out.read(&mut buf_out), if out_open => {
                match n {
                    Ok(0) | Err(_) => out_open = false,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf_out[..n]).into_owned();
                        print!("{chunk}");
                        let _ = std::io::stdout().flush();
                        captured.push_str(&chunk);
                    }
                }
            }
            n = err.read(&mut buf_err), if err_open => {
                match n {
                    Ok(0) | Err(_) => err_open = false,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf_err[..n]).into_owned();
                        eprint!("{chunk}");
                        let _ = std::io::stderr().flush();
                        captured.push_str(&chunk);
                    }
                }
            }
        }
    }
    let status = child.wait().await.map_err(|e| JiiError::spawn("jii", e))?;
    log.log(&captured);
    if !captured.ends_with('\n') {
        log.log("\n");
    }
    Ok(status.code())
}

/// Upload the (already scrubbed) log to a public paste service: 0x0.st first, then
/// paste.c-net.org. Returns the URL, or `None` when both fail (the local file remains).
async fn upload_log(path: &PathBuf) -> Option<String> {
    let body = std::fs::read(path).ok()?;
    let client = crate::provider::http_client().ok()?;

    // 0x0.st takes a multipart `file` field and answers with the URL as plain text.
    let part = reqwest::multipart::Part::bytes(body.clone())
        .file_name(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "jii-test.log".into()));
    let form = reqwest::multipart::Form::new().part("file", part);
    if let Ok(resp) = client.post("https://0x0.st").multipart(form).send().await
        && resp.status().is_success()
        && let Ok(text) = resp.text().await
    {
        let url = text.trim().to_string();
        if url.starts_with("http") {
            return Some(url);
        }
    }
    // Fallback: paste.c-net.org takes the raw body and answers with the URL.
    if let Ok(resp) = client.post("https://paste.c-net.org/").body(body).send().await
        && resp.status().is_success()
        && let Ok(text) = resp.text().await
    {
        let url = text.trim().to_string();
        if url.starts_with("http") {
            return Some(url);
        }
    }
    None
}

/// Percent-encode a string for a URL query component (RFC 3986 unreserved set kept).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The pre-filled new-issue URL for the JII repo.
fn issue_url(results: &[StepResult], log_url: Option<&str>) -> String {
    let passed = results.iter().filter(|r| r.verdict == Verdict::Pass).count();
    let failed: Vec<&StepResult> = results.iter().filter(|r| r.verdict == Verdict::Fail).collect();
    let title = if failed.is_empty() {
        format!("Test run OK: {} ({passed}/{} steps)", host_summary(), results.len())
    } else {
        format!("Test run: {} failed step(s) on {}", failed.len(), host_summary())
    };
    let mut body = format!("**Environment:** {}\n\n", host_summary());
    if let Some(url) = log_url {
        body.push_str(&format!("**Full log:** {url}\n\n"));
    }
    body.push_str("| # | Step | Exit | Verdict |\n|---|------|------|--------|\n");
    for (i, r) in results.iter().enumerate() {
        let exit = r.exit.map(|c| c.to_string()).unwrap_or_else(|| "signal".into());
        body.push_str(&format!("| {} | {} | {exit} | {} |\n", i + 1, r.title, r.verdict.label()));
    }
    for r in &failed {
        if !r.note.is_empty() {
            body.push_str(&format!("\n**{}:** {}\n", r.title, r.note));
        }
    }
    format!(
        "https://github.com/0nigiris/JII/issues/new?title={}&body={}",
        url_encode(&title),
        url_encode(&body)
    )
}

/// The whole checklist run. Returns an error (exit ≠ 0) when any step was judged FAIL,
/// so scripted runs can detect a bad round too.
pub async fn run() -> Result<()> {
    let mut log = TestLog::create()?;
    let steps = checklist();

    log.say("JII tester checklist — real commands, real installs. Run this in a VM.");
    log.say(&format!("Environment: {}", host_summary()));
    log.say(&format!("Log file:    {}", log.path.display()));
    log.say(&format!("Steps:       {}", steps.len()));
    log.say("After each step answer: y = looks right · n = something is wrong · s = skip");
    log.say("");

    let total = steps.len();
    let mut results: Vec<StepResult> = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        log.say(&format!("━━━ Step {}/{total}: {} ━━━", i + 1, step.title));
        log.say(&format!("    $ jii {}", step.args.join(" ")));
        log.say(&format!("    Expect: {}", step.expect));
        let exit = run_step(&mut log, step.args).await?;
        let code = exit.map(|c| c.to_string()).unwrap_or_else(|| "killed by signal".into());
        log.say(&format!("    (exit: {code})"));

        let verdict = loop {
            match ask("    Did this look right? [Y/n/s]", "y").to_lowercase().as_str() {
                "y" | "yes" | "д" | "да" => break Verdict::Pass,
                "n" | "no" | "н" | "нет" => break Verdict::Fail,
                "s" | "skip" => break Verdict::Skip,
                _ => {}
            }
        };
        let note = if verdict == Verdict::Fail {
            ask("    What was wrong? (one line, optional):", "")
        } else {
            String::new()
        };
        log.log(&format!("    Verdict: {}{}\n", verdict.label(), if note.is_empty() { String::new() } else { format!(" — {note}") }));
        results.push(StepResult { title: step.title, exit, verdict, note });
        log.say("");
    }

    // Summary.
    let passed = results.iter().filter(|r| r.verdict == Verdict::Pass).count();
    let failed = results.iter().filter(|r| r.verdict == Verdict::Fail).count();
    let skipped = results.iter().filter(|r| r.verdict == Verdict::Skip).count();
    log.say("━━━ Summary ━━━");
    for (i, r) in results.iter().enumerate() {
        log.say(&format!("  {:>2}. {:<28} {}", i + 1, r.title, r.verdict.label()));
    }
    log.say(&format!("  {passed} passed · {failed} failed · {skipped} skipped"));
    log.say(&format!("  Local log kept at: {}", log.path.display()));
    log.say("");

    // One-keypress upload (the log is already scrubbed of username/hostname).
    let mut log_url: Option<String> = None;
    if ask("Upload the log to a public paste service (0x0.st)? [Y/n]", "y").to_lowercase().starts_with(['y', 'д']) {
        println!("Uploading…");
        match upload_log(&log.path).await {
            Some(url) => {
                log.say(&format!("Log uploaded: {url}"));
                log_url = Some(url);
            }
            None => log.say("Upload failed on both services — attach the local file to the issue instead."),
        }
    }

    // The pre-filled issue link — reporting is one click.
    log.say("");
    log.say("Report this run (pre-filled GitHub issue):");
    log.say(&format!("  {}", issue_url(&results, log_url.as_deref())));

    if failed > 0 {
        return Err(JiiError::Other(anyhow::anyhow!(
            "{failed} step(s) were judged FAIL — see {}",
            log.path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_replaces_longest_first() {
        let mut log = TestLog {
            path: PathBuf::from("/dev/null"),
            file: std::fs::File::create("/dev/null").unwrap(),
            scrub: vec![("onibox".to_string(), "HOST"), ("oni".to_string(), "USER")],
        };
        // Ordering is by length descending, so the hostname wins inside itself.
        log.scrub.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        let mut text = "user oni on onibox in /home/oni".to_string();
        for (needle, replacement) in &log.scrub {
            text = text.replace(needle, replacement);
        }
        assert_eq!(text, "user USER on HOST in /home/USER");
    }

    #[test]
    fn issue_url_carries_summary_and_encodes() {
        let results = vec![
            StepResult { title: "Version banner", exit: Some(0), verdict: Verdict::Pass, note: String::new() },
            StepResult { title: "REAL install", exit: Some(1), verdict: Verdict::Fail, note: "spinner froze".into() },
        ];
        let url = issue_url(&results, Some("https://0x0.st/abc.log"));
        assert!(url.starts_with("https://github.com/0nigiris/JII/issues/new?title="));
        assert!(url.contains("body="));
        // Encoded content: no raw spaces/newlines leak into the URL.
        assert!(!url.contains(' ') && !url.contains('\n'));
        // The failure note travels along (percent-encoded).
        assert!(url.contains(&url_encode("spinner froze")));
    }

    #[test]
    fn checklist_steps_never_open_the_interactive_chooser() {
        // Each step must carry -y/--no or be a non-choosing command: the child's stdout is
        // piped, so a full-screen menu could not redraw. Guard the invariant.
        for step in checklist() {
            let non_choosing = matches!(
                step.args[0],
                "--version" | "doctor" | "search" | "info" | "list" | "how" | "sources" | "cache"
            );
            assert!(
                non_choosing || step.args.contains(&"-y") || step.args.contains(&"--no"),
                "step '{}' could open the chooser",
                step.title
            );
        }
    }
}
