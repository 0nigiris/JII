// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Source-agnostic progress reading from a manager's live output.
//!
//! JII's core never branches on the source id (ADR-0004), and neither does this: it reads
//! only the *universal* shapes package managers print while working — a bracketed step
//! counter like `[ 3/41]` (dnf5, apt, pip) or a bare `NN%` (download bars). One line in,
//! an optional [`Progress`] out. A manager JII has never met still animates a real bar as
//! long as it speaks one of these two dialects; one that speaks neither simply falls back
//! to the timed spinner, never a wrong number.
//!
//! This is the reading side only. Turning a [`Progress`] into a drawn bar lives in
//! [`crate::ui`]; feeding lines to it lives in the streaming executor.

/// One progress reading parsed from a single output line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Completion, clamped to `0..=100`.
    pub percent: u8,
    /// The `(done, total)` counter that produced it, when a step counter (not a raw `%`)
    /// was the source — so the UI can show `[3/41]` alongside the bar.
    pub steps: Option<(u32, u32)>,
}

/// Read a progress signal out of one output line, or `None` when the line states none.
///
/// A bracketed `[done/total]` counter wins over a bare `NN%`: it is monotonic across a whole
/// transaction (dnf installs `[ 1/41]` … `[41/41]`), whereas a `%` is usually per-file and
/// resets each download — the counter is the honest whole-job progress. Only when there is no
/// counter do we fall back to the last percentage on the line (a download bar's current value).
pub fn parse_progress(line: &str) -> Option<Progress> {
    if let Some((done, total)) = bracketed_ratio(line) {
        let percent = ((done as f64 / total as f64) * 100.0).round().clamp(0.0, 100.0) as u8;
        return Some(Progress { percent, steps: Some((done, total)) });
    }
    last_percent(line).map(|percent| Progress { percent, steps: None })
}

/// The last `NN%` on the line whose number is `0..=100`, or `None`. Walks back from each `%`
/// collecting the run of digits before it, so `curl  1.2MB/s  45%` reads 45 and `(100%)` reads
/// 100. The *last* one wins because a bar prints older values earlier on the same refreshed line.
fn last_percent(line: &str) -> Option<u8> {
    let bytes = line.as_bytes();
    let mut found = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'%' {
            continue;
        }
        // Collect the contiguous ASCII digits immediately to the left of this '%'.
        let mut start = i;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start == i {
            continue; // a lone '%' with no number in front of it
        }
        if let Ok(n) = line[start..i].parse::<u16>()
            && n <= 100
        {
            found = Some(n as u8);
        }
    }
    found
}

/// The last bracketed `done/total` counter — `[ 3/41]` or `(1/5)` — with `0 < total` and
/// `done <= total`, or `None`. Requiring the brackets (and *nothing but* the ratio inside them)
/// is what keeps prose ratios and dates out: `[24/07/2026]` holds `24/07/2026`, not a clean
/// `N/M`, so it is rejected rather than misread as 57%.
fn bracketed_ratio(line: &str) -> Option<(u32, u32)> {
    let bytes = line.as_bytes();
    let mut result = None;
    let mut i = 0;
    while i < bytes.len() {
        let close = match bytes[i] {
            b'[' => b']',
            b'(' => b')',
            _ => {
                i += 1;
                continue;
            }
        };
        // Find the matching close bracket.
        if let Some(rel) = line[i + 1..].find(close as char) {
            let inner = line[i + 1..i + 1 + rel].trim();
            if let Some(ratio) = parse_ratio(inner) {
                result = Some(ratio);
            }
            i = i + 1 + rel + 1;
        } else {
            break;
        }
    }
    result
}

/// Parse exactly `done/total` (spaces around the slash allowed) with `total > 0` and
/// `done <= total`. Anything else — extra slashes, non-digits, `total == 0` — is `None`.
fn parse_ratio(inner: &str) -> Option<(u32, u32)> {
    let (a, b) = inner.split_once('/')?;
    let done: u32 = a.trim().parse().ok()?;
    let total: u32 = b.trim().parse().ok()?;
    if total > 0 && done <= total {
        Some((done, total))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dnf5_step_counter_drives_the_bar() {
        // dnf5 pads the counter; the whole-transaction progress is the counter, not a %.
        let p = parse_progress("[ 3/41] Installing curl-8.8.0").unwrap();
        assert_eq!(p.percent, 7); // 3/41 = 7.3% → 7
        assert_eq!(p.steps, Some((3, 41)));
        assert_eq!(parse_progress("[41/41] Verifying …").unwrap().percent, 100);
    }

    #[test]
    fn pip_style_parenthesised_counter() {
        assert_eq!(parse_progress("(1/5) Downloading numpy").unwrap().steps, Some((1, 5)));
    }

    #[test]
    fn bare_percentage_when_no_counter() {
        let p = parse_progress("Downloading gimp   1.2 MB/s   45%").unwrap();
        assert_eq!(p.percent, 45);
        assert_eq!(p.steps, None);
        assert_eq!(parse_progress("(100%)").unwrap().percent, 100);
    }

    #[test]
    fn counter_wins_over_percentage_on_the_same_line() {
        // Both present → the monotonic counter is the honest whole-job figure.
        let p = parse_progress("[ 2/10] app  99%").unwrap();
        assert_eq!(p.steps, Some((2, 10)));
        assert_eq!(p.percent, 20);
    }

    #[test]
    fn last_percentage_on_a_refreshed_line_wins() {
        assert_eq!(parse_progress("10%   30%   55%").unwrap().percent, 55);
    }

    #[test]
    fn dates_and_prose_ratios_are_not_progress() {
        assert_eq!(parse_progress("Log rotated [24/07/2026]"), None); // not a clean N/M
        assert_eq!(parse_progress("see section 3/4 of the manual"), None); // unbracketed
        assert_eq!(parse_progress("Reading state information..."), None);
        assert_eq!(parse_progress("100 % done"), None); // space breaks the number-then-% shape
    }

    #[test]
    fn rejects_impossible_counters_and_percentages() {
        assert_eq!(parse_progress("[5/0] weird"), None); // total 0
        assert_eq!(parse_progress("[9/3] weird"), None); // done > total
        assert_eq!(parse_progress("temperature 140% of limit"), None); // > 100
    }
}
