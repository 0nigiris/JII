#!/usr/bin/env python3
"""Generate a self-hosted star + download history SVG for the README.

Third-party live widgets (star-history.com, starchart.cc) share GitHub API tokens and
routinely answer 503/"rate limited" — so the README chart randomly breaks. Instead we
render our own SVG from the repo's stargazer timestamps and release download counts and
commit it: it is served from the repo, always renders, and a scheduled workflow keeps it
current (see `.github/workflows/star-history.yml`).

Two series on one chart: cumulative **stars** (left axis, purple) over time, and
cumulative **downloads** (right axis, cyan) over release dates.

Pure standard library. Auth via `GITHUB_TOKEN` (the Actions token) or `JII_GITHUB_TOKEN`
locally; unauthenticated also works for small repos but is rate-limited.

Usage:  python3 scripts/gen_star_history.py [owner/repo] [out.svg]
"""

from __future__ import annotations

import json
import os
import sys
import urllib.request
from datetime import datetime, timezone

REPO = sys.argv[1] if len(sys.argv) > 1 else "0nigiris/JII"
OUT = sys.argv[2] if len(sys.argv) > 2 else "assets/star-history.svg"

# Brand palette (sampled from assets/banner.png). CYAN is the second-series accent —
# distinct from the brand purple and legible on the black background.
PURPLE = "#6A31F2"
CYAN = "#22D3EE"
INK = "#F5F4F5"
DIM = "#8A8394"
BG = "#000000"
GRID = "#FFFFFF14"  # ~8% white

W, H = 800, 420
ML, MR, MT, MB = 60, 54, 58, 46  # plot margins (MR widened for the right download axis)
PX0, PX1 = ML, W - MR
PY0, PY1 = MT, H - MB


def gh_get(url: str, accept: str = "application/vnd.github+json") -> list:
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("JII_GITHUB_TOKEN")
    req = urllib.request.Request(url)
    req.add_header("Accept", accept)
    req.add_header("User-Agent", "jii-star-history")
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


def build_svg(
    repo: str,
    stamps: list[datetime],
    downloads: list[tuple[datetime, int]],
) -> str:
    now = datetime.now(timezone.utc)
    n = len(stamps)
    dl_total = downloads[-1][1] if downloads else 0
    have_dl = dl_total > 0

    # Star cumulative points (t_i, i); extend the last value to "now".
    if n == 0:
        star_pts = [(now, 0)]
    else:
        star_pts = [(t, i + 1) for i, t in enumerate(stamps)]
        star_pts.append((now, n))

    # Download cumulative points; extend the last value to "now" so the line reaches today.
    dl_pts = list(downloads)
    if dl_pts:
        dl_pts.append((now, dl_pts[-1][1]))

    # Time axis spans the earliest event of either series to now.
    candidates = [p[0] for p in star_pts] + [p[0] for p in dl_pts]
    t_min = min(candidates) if candidates else now
    t_max = now
    span = max((t_max - t_min).total_seconds(), 1.0)

    # Left (stars) axis.
    y_hi = max(1, n)
    step = nice_step(y_hi)
    star_top = ((y_hi + step - 1) // step) * step

    # Right (downloads) axis.
    dl_hi = max(1, dl_total)
    dl_step = nice_step(dl_hi)
    dl_top = ((dl_hi + dl_step - 1) // dl_step) * dl_step

    def sx(t: datetime) -> float:
        return PX0 + (t - t_min).total_seconds() / span * (PX1 - PX0)

    def sy_star(v: float) -> float:
        return PY1 - (v / star_top) * (PY1 - PY0)

    def sy_dl(v: float) -> float:
        return PY1 - (v / dl_top) * (PY1 - PY0)

    parts: list[str] = []
    parts.append(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
        f'viewBox="0 0 {W} {H}" font-family="ui-sans-serif,Segoe UI,Helvetica,Arial,sans-serif">'
    )
    parts.append(f'<rect width="{W}" height="{H}" rx="12" fill="{BG}"/>')

    # Title.
    parts.append(
        f'<text x="{ML}" y="34" fill="{INK}" font-size="20" font-weight="700">'
        f"Stars &amp; downloads</text>"
    )
    # Totals (double as the legend): stars purple, downloads cyan.
    if have_dl:
        parts.append(
            f'<text x="{PX1}" y="26" fill="{PURPLE}" font-size="18" font-weight="700" '
            f'text-anchor="end">★ {n}</text>'
        )
        parts.append(
            f'<text x="{PX1}" y="47" fill="{CYAN}" font-size="18" font-weight="700" '
            f'text-anchor="end">↓ {dl_total}</text>'
        )
    else:
        parts.append(
            f'<text x="{PX1}" y="34" fill="{PURPLE}" font-size="20" font-weight="700" '
            f'text-anchor="end">★ {n}</text>'
        )
    parts.append(
        f'<text x="{ML}" y="{H-14}" fill="{DIM}" font-size="12">{esc(repo)}</text>'
    )

    # Horizontal grid, aligned to the star axis; label stars on the left, downloads on
    # the right (each series scaled so its own max sits at the top gridline).
    v = 0
    while v <= star_top:
        y = sy_star(v)
        parts.append(
            f'<line x1="{PX0}" y1="{y:.1f}" x2="{PX1}" y2="{y:.1f}" '
            f'stroke="{GRID}" stroke-width="1"/>'
        )
        parts.append(
            f'<text x="{PX0-8}" y="{y+4:.1f}" fill="{PURPLE}" font-size="11" '
            f'text-anchor="end">{v}</text>'
        )
        if have_dl:
            frac = v / star_top if star_top else 0
            parts.append(
                f'<text x="{PX1+8}" y="{y+4:.1f}" fill="{CYAN}" font-size="11" '
                f'text-anchor="start">{round(frac * dl_top)}</text>'
            )
        v += step

    # X date labels (start … now), a few evenly spaced.
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

    # Downloads line (right axis) — drawn first so stars sit on top.
    if have_dl and len(dl_pts) >= 1:
        dl_line = " ".join(f"{sx(t):.1f},{sy_dl(v):.1f}" for t, v in dl_pts)
        parts.append(
            f'<polyline points="{dl_line}" fill="none" stroke="{CYAN}" '
            f'stroke-width="3" stroke-linejoin="round" stroke-linecap="round"/>'
        )
        for t, vv in dl_pts[:-1]:
            parts.append(
                f'<circle cx="{sx(t):.1f}" cy="{sy_dl(vv):.1f}" r="3.5" '
                f'fill="{BG}" stroke="{CYAN}" stroke-width="2"/>'
            )

    # Stars line (left axis), no area fill.
    star_line = " ".join(f"{sx(t):.1f},{sy_star(v):.1f}" for t, v in star_pts)
    parts.append(
        f'<polyline points="{star_line}" fill="none" stroke="{PURPLE}" '
        f'stroke-width="3" stroke-linejoin="round" stroke-linecap="round"/>'
    )
    for t, vv in star_pts[:-1] if n else []:
        parts.append(
            f'<circle cx="{sx(t):.1f}" cy="{sy_star(vv):.1f}" r="3.5" '
            f'fill="{BG}" stroke="{PURPLE}" stroke-width="2"/>'
        )

    parts.append("</svg>")
    return "\n".join(parts)


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
    svg = build_svg(REPO, stamps, downloads)
    os.makedirs(os.path.dirname(OUT) or ".", exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(svg + "\n")
    dl_total = downloads[-1][1] if downloads else 0
    print(f"wrote {OUT} ({len(stamps)} stars, {dl_total} downloads)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
