#!/usr/bin/env python3
"""Generate a self-hosted star-history SVG for the README.

Third-party live widgets (star-history.com, starchart.cc) share GitHub API tokens and
routinely answer 503/"rate limited" — so the README chart randomly breaks. Instead we
render our own SVG from the repo's stargazer timestamps and commit it: it is served from
the repo, always renders, and a scheduled workflow keeps it current (see
`.github/workflows/star-history.yml`).

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

# Brand palette (sampled from assets/banner.png).
PURPLE = "#6A31F2"
INK = "#F5F4F5"
DIM = "#8A8394"
BG = "#000000"
GRID = "#FFFFFF14"  # ~8% white

W, H = 800, 420
ML, MR, MT, MB = 60, 26, 58, 46  # plot margins
PX0, PX1 = ML, W - MR
PY0, PY1 = MT, H - MB


def gh_get(url: str) -> list:
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("JII_GITHUB_TOKEN")
    req = urllib.request.Request(url)
    req.add_header("Accept", "application/vnd.github.star+json")
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
        batch = gh_get(url)
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


def nice_step(hi: int, ticks: int = 4) -> int:
    """A round-ish integer y-step so the axis reads 0, s, 2s, …"""
    raw = max(1, hi / ticks)
    for step in (1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000):
        if step >= raw:
            return step
    return int(raw)


def esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def build_svg(repo: str, stamps: list[datetime]) -> str:
    now = datetime.now(timezone.utc)
    n = len(stamps)

    # Cumulative points (t_i, i); extend the last value to "now" so the line reaches today.
    if n == 0:
        pts = [(now, 0)]
        t_min = now
    else:
        pts = [(t, i + 1) for i, t in enumerate(stamps)]
        pts.append((now, n))
        t_min = stamps[0]
    t_max = now
    span = max((t_max - t_min).total_seconds(), 1.0)
    y_hi = max(1, n)
    step = nice_step(y_hi)
    y_top = ((y_hi + step - 1) // step) * step  # round up to a full step

    def sx(t: datetime) -> float:
        return PX0 + (t - t_min).total_seconds() / span * (PX1 - PX0)

    def sy(v: float) -> float:
        return PY1 - (v / y_top) * (PY1 - PY0)

    parts: list[str] = []
    parts.append(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
        f'viewBox="0 0 {W} {H}" font-family="ui-sans-serif,Segoe UI,Helvetica,Arial,sans-serif">'
    )
    parts.append(
        '<defs><linearGradient id="fill" x1="0" y1="0" x2="0" y2="1">'
        f'<stop offset="0" stop-color="{PURPLE}" stop-opacity="0.35"/>'
        f'<stop offset="1" stop-color="{PURPLE}" stop-opacity="0"/>'
        "</linearGradient></defs>"
    )
    parts.append(f'<rect width="{W}" height="{H}" rx="12" fill="{BG}"/>')

    # Title + current total.
    parts.append(
        f'<text x="{ML}" y="34" fill="{INK}" font-size="20" font-weight="700">'
        f"Star history</text>"
    )
    parts.append(
        f'<text x="{PX1}" y="34" fill="{PURPLE}" font-size="20" font-weight="700" '
        f'text-anchor="end">★ {n}</text>'
    )
    parts.append(
        f'<text x="{ML}" y="{H-14}" fill="{DIM}" font-size="12">{esc(repo)}</text>'
    )

    # Horizontal grid + y labels.
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

    # X date labels (start … now), a few evenly spaced.
    label_ticks = 1 if span < 86400 else 4
    for k in range(label_ticks + 1):
        frac = k / label_ticks
        t = datetime.fromtimestamp(
            t_min.timestamp() + frac * span, tz=timezone.utc
        )
        x = PX0 + frac * (PX1 - PX0)
        anchor = "start" if k == 0 else ("end" if k == label_ticks else "middle")
        parts.append(
            f'<text x="{x:.1f}" y="{PY1+20:.1f}" fill="{DIM}" font-size="11" '
            f'text-anchor="{anchor}">{t.strftime("%b %-d, %Y")}</text>'
        )

    # Area fill + line (stepwise cumulative).
    line = " ".join(f"{sx(t):.1f},{sy(v):.1f}" for t, v in pts)
    area = f"{PX0:.1f},{PY1:.1f} " + line + f" {sx(pts[-1][0]):.1f},{PY1:.1f}"
    parts.append(f'<polygon points="{area}" fill="url(#fill)"/>')
    parts.append(
        f'<polyline points="{line}" fill="none" stroke="{PURPLE}" '
        f'stroke-width="3" stroke-linejoin="round" stroke-linecap="round"/>'
    )
    # Dots at each real star event.
    for t, vv in pts[:-1] if n else []:
        parts.append(
            f'<circle cx="{sx(t):.1f}" cy="{sy(vv):.1f}" r="3.5" '
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
    svg = build_svg(REPO, stamps)
    os.makedirs(os.path.dirname(OUT) or ".", exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(svg + "\n")
    print(f"wrote {OUT} ({len(stamps)} stars)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
