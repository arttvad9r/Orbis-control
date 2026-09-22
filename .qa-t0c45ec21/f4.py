#!/usr/bin/env python3
"""F4a/F4b re-measurement with correctly derived row bands.

Instead of guessing row y positions, we derive them: the sidebar's selected row
carries a soft accent fill 44px tall, and each row's label glyphs give a second
anchor. We:
  1. locate each nav row's y-extent from the foreground row profile in the sidebar;
  2. per row measure the ICON glyph bbox (columns icon_x0..icon_x0+17) and the
     LABEL glyph bbox (columns >= label_x0);
  3. compare icon vertical centre vs label vertical centre (F4a);
  4. compare icon_x0 and label_x0 across rows (F4b).
"""
from pathlib import Path

import numpy as np
from PIL import Image

QA = Path("/home/artt/.hermes/profiles/qa/cache/scratch/qa-t5b9d96c6/shots")
ROOT = Path("/home/artt/Orbis-control-implementation")
SIDEBAR_W = 232


def arr(name_or_path):
    p = QA / name_or_path if isinstance(name_or_path, str) else name_or_path
    return np.asarray(Image.open(p).convert("RGB"), dtype=np.int16)


def nav_rows(a):
    """Return (y0, y1) for each nav row by profiling the sidebar foreground.

    Uses the label/icon glyph columns only (x 20..200) so wide fills are included
    as row evidence but the title bar is excluded (starts below y=50).
    """
    bg = a[500, 116]
    diff = np.abs(a[:, 0:SIDEBAR_W] - bg).sum(axis=2)
    fg = diff > 45
    profile = fg[:, 20:200].sum(axis=1)
    rows = []
    inb = False
    for y in range(50, a.shape[0]):
        if profile[y] > 0 and not inb:
            start = y
            inb = True
        elif profile[y] == 0 and inb:
            rows.append((start, y - 1))
            inb = False
    if inb:
        rows.append((start, a.shape[0] - 1))
    # merge fragments separated by < 6px
    merged = []
    for r in rows:
        if merged and r[0] - merged[-1][1] <= 6:
            merged[-1] = (merged[-1][0], r[1])
        else:
            merged.append(list(r))
    return [(int(s), int(e)) for s, e in merged if e - s >= 12]


def row_metrics(a, y0, y1):
    bg = a[500, 116]
    diff = np.abs(a[y0:y1 + 1, 0:SIDEBAR_W] - bg).sum(axis=2)
    fg = diff > 45
    colcount = fg.sum(axis=0)
    # a full-height fill column (selected/hover background) has nearly the row height
    fill = colcount > (y1 - y0 + 1) * 0.8
    glyph = fg.any(axis=0) & (~fill)
    cols = sorted(set(int(c) for c in np.where(glyph)[0]))
    if not cols:
        return None
    icon_x0 = cols[0]
    icon_right = icon_x0
    for p, q in zip(cols, cols[1:]):
        if q - p >= 6:
            icon_right = p
            break
    label_cols = [c for c in cols if c > icon_right + 1]
    label_x0 = label_cols[0] if label_cols else None

    def ybox(c0, c1):
        sub = fg[:, c0:c1 + 1]
        ys = np.where(sub.any(axis=1))[0]
        return (int(ys.min()), int(ys.max())) if len(ys) else (None, None)

    icon_y = ybox(icon_x0, icon_right)
    label_y = ybox(label_x0, max(label_cols)) if label_x0 else (None, None)
    return dict(icon_x0=icon_x0, icon_right=icon_right, label_x0=label_x0,
                icon_y=icon_y, label_y=label_y,
                fill_cols=int(fill.sum()), row_h=y1 - y0 + 1)


for name in ("c5ca-settings-dark-restore.png", "c5ca-settings-toggled.png",
             "p980-settings.png"):
    a = arr(name)
    print("=" * 84)
    print(f"{name}  ({a.shape[1]}x{a.shape[0]})   sidebar rows found:")
    rows = nav_rows(a)
    print(f"  {len(rows)} rows: {rows}")
    icon_xs, label_xs = [], []
    for (y0, y1) in rows:
        m = row_metrics(a, y0, y1)
        if m is None:
            print(f"   y[{y0},{y1}] h={y1 - y0 + 1}: no glyph pixels")
            continue
        icon_xs.append(m["icon_x0"])
        if m["label_x0"] is not None:
            label_xs.append(m["label_x0"])
        iy0, iy1 = m["icon_y"]
        ly0, ly1 = m["label_y"]
        imid = (iy0 + iy1) / 2 if iy0 is not None else None
        lmid = (ly0 + ly1) / 2 if ly0 is not None else None
        delta = (imid - lmid) if (imid is not None and lmid is not None) else None
        print(f"   y[{y0:3d},{y1:3d}] h={m['row_h']:2d}  icon_x0={m['icon_x0']:3d} "
              f"icon_y=[{iy0},{iy1}] mid={imid}   label_x0={m['label_x0']} "
              f"label_y=[{ly0},{ly1}] mid={lmid}  icon-label_centre_delta="
              f"{delta:+.1f}px  fill_cols={m['fill_cols']}")
    if icon_xs:
        print(f"   > icon_x0 spread = {max(icon_xs) - min(icon_xs)} "
              f"(columns {min(icon_xs)}..{max(icon_xs)}) across {len(icon_xs)} rows")
    if label_xs:
        print(f"   > label_x0 spread = {max(label_xs) - min(label_xs)} "
              f"(columns {min(label_xs)}..{max(label_xs)}) across {len(label_xs)} rows")
    print()
