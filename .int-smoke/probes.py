#!/usr/bin/env python3
"""Smoke probes for the merged candidate's renders (t_3864d46e).

1. Per-render sanity: non-blank, expected size, decoded OK.
2. Cross-section distinctness at 980x680 dark (pages really switch).
3. Regression: merged normal renders vs f45f55d (UI-QA PASS candidate)
   renders must be pixel-identical for every (section, size, theme).
4. Functional scenario probes: error/pending/unsupported renders differ
   from normal and carry the expected signal (error red in status band).
"""
import sys
from pathlib import Path

import numpy as np
from PIL import Image

MERGED = Path(sys.argv[1])
BASE = Path(sys.argv[2]) if len(sys.argv) > 2 else None

SECTIONS = ["dashboard", "performance", "power", "cooling", "graphics",
            "backlight", "display", "system", "settings", "about"]
GEOS = ["980x680", "1200x800"]


def load(p: Path) -> np.ndarray:
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def nonblank(a: np.ndarray) -> tuple[bool, str]:
    h, w, _ = a.shape
    corners = [a[2, 2], a[2, w - 3], a[h - 3, 2], a[h - 3, w - 3]]
    bg = np.median(np.stack(corners).reshape(-1, 3), axis=0)
    diff = np.abs(a - bg).sum(axis=2)
    fg = int((diff > 40).sum())
    return fg > 500, f"fg_pixels={fg} bg={bg.tolist()}"


def red_present(a: np.ndarray, y0=40, y1=140, x0=232, x1=None) -> tuple[bool, int]:
    """Error-status red: pixels with r clearly above g and b in the header band."""
    band = a[y0:y1, x0:(x1 or a.shape[1] - 24), :]
    r, g, b = band[:, :, 0], band[:, :, 1], band[:, :, 2]
    hits = int(((r - g > 40) & (r - b > 40) & (r > 90)).sum())
    return hits > 0, hits


fail = []

# --- 1 + 2: merged sanity and distinctness ---------------------------------
digests = {}
problems = []
for sec in SECTIONS:
    for geo in GEOS:
        p = MERGED / f"{sec}-{geo}-dark.png"
        if not p.exists():
            problems.append(f"MISSING {p.name}")
            continue
        a = load(p)
        ok, info = nonblank(a)
        w, h = (int(v) for v in geo.split("x"))
        if a.shape != (h, w, 3):
            problems.append(f"SIZE {p.name} got {a.shape}")
        if not ok:
            problems.append(f"BLANK {p.name} ({info})")
        digests[(sec, geo)] = a.sum(axis=2)

for geo in GEOS:
    ref = digests.get(("dashboard", geo))
    for sec in SECTIONS[1:]:
        d = digests.get((sec, geo))
        if ref is None or d is None:
            continue
        frac = float((d != ref).mean())
        if frac < 0.005:
            problems.append(f"NOT-DISTINCT {sec}@{geo} vs dashboard: diff_frac={frac:.4f}")

# --- 3: regression vs f45f55d ---------------------------------------------
reg_rows = []
if BASE is not None:
    for sec in SECTIONS:
        for geo in GEOS:
            for theme in ("dark", "light"):
                if theme == "light" and sec not in ("cooling", "performance", "about"):
                    continue
                m = MERGED / f"{sec}-{geo}-{theme}.png"
                b = BASE / f"{sec}-{geo}-{theme}.png"
                if not m.exists() or not b.exists():
                    reg_rows.append(f"{sec}-{geo}-{theme}: MISSING-SIDE")
                    fail.append(f"regression-missing {sec}-{geo}-{theme}")
                    continue
                am, ab = load(m), load(b)
                if am.shape != ab.shape:
                    reg_rows.append(f"{sec}-{geo}-{theme}: SHAPE {am.shape} vs {ab.shape}")
                    fail.append(f"regression-shape {sec}-{geo}-{theme}")
                    continue
                frac = float((am != ab).any(axis=2).mean())
                maxd = int(np.abs(am.astype(int) - ab.astype(int)).max())
                identical = am.tobytes() == ab.tobytes()
                reg_rows.append(f"{sec}-{geo}-{theme}: identical={identical} diff_frac={frac:.6f} maxdelta={maxd}")
                if not identical:
                    fail.append(f"regression-diff {sec}-{geo}-{theme} diff_frac={frac:.6f}")

# --- 4: functional scenario probes ----------------------------------------
scen = []
pairs = [
    ("cooling-980x680-dark", "error", "red"),
    ("graphics-980x680-dark", "pending", "diff"),
    ("performance-980x680-dark", "unsupported", "diff"),
]
for stem, scen_name, kind in pairs:
    pn = MERGED / f"{stem}.png"
    ps = MERGED / f"{stem}-{scen_name}.png"
    if not pn.exists() or not ps.exists():
        scen.append(f"{scen_name}: MISSING-RENDER")
        fail.append(f"scenario-missing {scen_name}")
        continue
    an, asx = load(pn), load(ps)
    differs = not np.array_equal(an, asx)
    frac = float((an != asx).any(axis=2).mean())
    row = f"{scen_name}: differs_from_normal={differs} diff_frac={frac:.4f}"
    if kind == "red":
        ok, hits = red_present(asx)
        row += f" error_red_pixels={hits} red_present={ok}"
        if not ok:
            fail.append(f"scenario-red-missing {scen_name}")
    if not differs:
        fail.append(f"scenario-nochange {scen_name}")
    scen.append(row)

print("== MERGED SANITY ==")
print("PROBLEMS:", problems if problems else "none")
print("== REGRESSION vs f45f55d (normal scenario) ==")
for r in reg_rows or ["(no baseline provided)"]:
    print(" ", r)
print("== FUNCTIONAL SCENARIOS (merged) ==")
for r in scen:
    print(" ", r)
print("== SMOKE VERDICT ==")
if fail:
    print("FAIL")
    for f in fail:
        print("  -", f)
    sys.exit(1)
print("PASS")
