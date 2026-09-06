#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 0nigiris
#
# SPDX-License-Identifier: GPL-3.0-or-later
"""Generate self-hosted growth charts for the README — two simple SVGs.

Third-party live widgets (star-history.com, starchart.cc) share GitHub API tokens and
routinely answer 503/"rate limited" — so a README chart randomly breaks. Instead we
render our own SVGs from the repo's data and commit them: they are served from the repo,
always render, and a scheduled workflow keeps them current (see the workflow in
`.github/workflows/`).

Two independent charts, each a single clean line on its own axis:
  * assets/stars.svg     — cumulative stars over time (purple)
  * assets/downloads.svg — cumulative release downloads over time (cyan)

Pure standard library. Auth via `GITHUB_TOKEN` (the Actions token) or `JII_GITHUB_TOKEN`
locally; unauthenticated also works for small repos but is rate-limited.

Usage:  python3 scripts/gen_charts.py [owner/repo] [out_dir]
"""

from __future__ import annotations

import json
import os
import sys
import urllib.request
from datetime import datetime, timezone

REPO = sys.argv[1] if len(sys.argv) > 1 else "0nigiris/JII"
OUT_DIR = sys.argv[2] if len(sys.argv) > 2 else "assets"

# Brand palette (sampled from assets/banner.png).
PURPLE = "#6A31F2"  # stars
CYAN = "#22D3EE"  # downloads
INK = "#F5F4F5"
DIM = "#8A8394"
BG = "#000000"
GRID = "#FFFFFF14"  # ~8% white

W, H = 720, 400
ML, MR, MT, MB = 58, 26, 58, 46  # plot margins
PX0, PX1 = ML, W - MR
PY0, PY1 = MT, H - MB


def gh_get(url: str, accept: str = "application/vnd.github+json") -> list:
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("JII_GITHUB_TOKEN")
    req = urllib.request.Request(url)
    req.add_header("Accept", accept)
    req.add_header("User-Agent", "jii-charts")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read().decode())


def fetch_starred_at(repo: str) -> list[datetime]:
    """All star timestamps, ascending. Paginates 100 at a time."""
    out: list[datetime] = []
    page = 1
    while True:
        url = f"https://api.github.com/repos/{repo}/stargazers?per_page=100&page={page}"
        batch = gh_get(url, accept="application/vnd.github.star+json")
        if not batch:
            break
        for s in batch:
            ts = s.get("starred_at") if isinstance(s, dict) else None
            if ts:
                out.append(datetime.fromisoformat(ts.replace("Z", "+00:00")))
        if len(batch) < 100:
            break
        page += 1
    out.sort()
    return out


def fetch_downloads(repo: str) -> list[tuple[datetime, int]]:
    """Cumulative download totals over release publish dates.

    GitHub exposes only the *current* per-asset `download_count` (there is no time
    series), so we attribute each release's downloads to its publish date and accumulate.
    Approximate, but the standard way to show a downloads trend."""
    per_release: list[tuple[datetime, int]] = []
    page = 1
    while True:
        url = f"https://api.github.com/repos/{repo}/releases?per_page=100&page={page}"
        batch = gh_get(url)
        if not batch:
            break
        for r in batch:
            if not isinstance(r, dict) or r.get("draft"):
                continue
            ts = r.get("published_at") or r.get("created_at")
            if not ts:
                continue
            total = sum(
                a.get("download_count", 0)
                for a in r.get("assets", [])
                if isinstance(a, dict)
            )
            per_release.append(
                (datetime.fromisoformat(ts.replace("Z", "+00:00")), total)
            )
        if len(batch) < 100:
            break
        page += 1
    per_release.sort(key=lambda x: x[0])
    cum: list[tuple[datetime, int]] = []
    running = 0
    for t, c in per_release:
        running += c
        cum.append((t, running))
    return cum


def nice_step(hi: int, ticks: int = 4) -> int:
    """A round-ish integer step so an axis reads 0, s, 2s, …"""
    raw = max(1, hi / ticks)
    for step in (1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 5000):
        if step >= raw:
            return step
    return int(raw)


def esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def build_chart(
    repo: str,
    title: str,
    accent: str,
    symbol: str,
    pts: list[tuple[datetime, int]],
) -> str:
    """One clean line chart: single Y axis, gridlines, date labels, a total badge."""
    now = datetime.now(timezone.utc)
    total = pts[-1][1] if pts else 0

    # Extend the last value to "now" so the line reaches today; guarantee ≥1 point.
    data = list(pts)
    if data:
        if data[-1][0] < now:
            data.append((now, data[-1][1]))
    else:
        data = [(now, 0)]

    t_min = min(p[0] for p in data)
    t_max = now
    span = max((t_max - t_min).total_seconds(), 1.0)

    y_hi = max(1, total)
    step = nice_step(y_hi)
    y_top = ((y_hi + step - 1) // step) * step

    def sx(t: datetime) -> float:
        return PX0 + (t - t_min).total_seconds() / span * (PX1 - PX0)

    def sy(v: float) -> float:
        return PY1 - (v / y_top) * (PY1 - PY0)

    parts: list[str] = []
    parts.append(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
        f'viewBox="0 0 {W} {H}" font-family="ui-sans-serif,Segoe UI,Helvetica,Arial,sans-serif">'
    )
    parts.append(f'<rect width="{W}" height="{H}" rx="12" fill="{BG}"/>')

    # Title (left) and total badge (right).
    parts.append(
        f'<text x="{ML}" y="34" fill="{INK}" font-size="20" font-weight="700">'
        f"{esc(title)}</text>"
    )
    parts.append(
        f'<text x="{PX1}" y="34" fill="{accent}" font-size="20" font-weight="700" '
        f'text-anchor="end">{symbol} {total}</text>'
    )
    parts.append(
        f'<text x="{ML}" y="{H-14}" fill="{DIM}" font-size="12">{esc(repo)}</text>'
    )

    # Horizontal gridlines with left labels.
    v = 0
    while v <= y_top:
        y = sy(v)
        parts.append(
            f'<line x1="{PX0}" y1="{y:.1f}" x2="{PX1}" y2="{y:.1f}" '
            f'stroke="{GRID}" stroke-width="1"/>'
        )
        parts.append(
            f'<text x="{PX0-8}" y="{y+4:.1f}" fill="{DIM}" font-size="11" '
            f'text-anchor="end">{v}</text>'
        )
        v += step

    # X date labels (start … now).
    label_ticks = 1 if span < 86400 else 4
    for k in range(label_ticks + 1):
        frac = k / label_ticks
        t = datetime.fromtimestamp(t_min.timestamp() + frac * span, tz=timezone.utc)
        x = PX0 + frac * (PX1 - PX0)
        anchor = "start" if k == 0 else ("end" if k == label_ticks else "middle")
        parts.append(
            f'<text x="{x:.1f}" y="{PY1+20:.1f}" fill="{DIM}" font-size="11" '
            f'text-anchor="{anchor}">{t.strftime("%b %-d, %Y")}</text>'
        )

    # Soft area under the line for a bit of body, then the line and dots on top.
    line = " ".join(f"{sx(t):.1f},{sy(v):.1f}" for t, v in data)
    area = (
        f"{PX0:.1f},{PY1:.1f} "
        + line
        + f" {sx(data[-1][0]):.1f},{PY1:.1f}"
    )
    parts.append(f'<polygon points="{area}" fill="{accent}" fill-opacity="0.08"/>')
    parts.append(
        f'<polyline points="{line}" fill="none" stroke="{accent}" '
        f'stroke-width="3" stroke-linejoin="round" stroke-linecap="round"/>'
    )
    # Dots on real data points only (skip the synthetic "now" tail).
    real = data[:-1] if (len(data) > 1 and data[-1][0] == now and total) else data
    for t, vv in real:
        parts.append(
            f'<circle cx="{sx(t):.1f}" cy="{sy(vv):.1f}" r="3.5" '
            f'fill="{BG}" stroke="{accent}" stroke-width="2"/>'
        )

    parts.append("</svg>")
    return "\n".join(parts)


def write(path: str, svg: str) -> None:
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(svg + "\n")


def main() -> int:
    try:
        stamps = fetch_starred_at(REPO)
    except Exception as e:  # noqa: BLE001 — best-effort generator, report and exit non-zero
        print(f"failed to fetch stargazers for {REPO}: {e}", file=sys.stderr)
        return 1
    try:
        downloads = fetch_downloads(REPO)
    except Exception as e:  # noqa: BLE001 — downloads are optional; warn and carry on
        print(f"warning: failed to fetch downloads for {REPO}: {e}", file=sys.stderr)
        downloads = []

    star_pts = [(t, i + 1) for i, t in enumerate(stamps)]
    stars_svg = build_chart(REPO, "Stars", PURPLE, "★", star_pts)
    downloads_svg = build_chart(REPO, "Downloads", CYAN, "↓", downloads)

    write(os.path.join(OUT_DIR, "stars.svg"), stars_svg)
    write(os.path.join(OUT_DIR, "downloads.svg"), downloads_svg)

    dl_total = downloads[-1][1] if downloads else 0
    print(
        f"wrote {OUT_DIR}/stars.svg ({len(stamps)} stars) and "
        f"{OUT_DIR}/downloads.svg ({dl_total} downloads)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
