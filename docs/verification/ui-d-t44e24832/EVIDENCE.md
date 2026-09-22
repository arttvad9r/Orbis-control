# UI-D (t_44e24832) — Live nav geometry + Dynamic Boost row — EVIDENCE

Candidate: orbis-control `target/release/orbis-control`, built from this
worktree at the commit carrying this file (UI fix included, see
"Defect found and fixed during verification" below).

## Method

Private D-Bus session bus per run (`dbus-daemon --session`, unix socket under
/tmp), scripted fixture peer `tools/fake-peer` serving Session1.PowerLimits with
the repo's standard test triple; production binary launched against that bus
with sandboxed XDG_CONFIG_HOME/XDG_STATE_HOME, X11 backend (`:0` +
session XAUTHORITY). The user's own live orbis-control instance is never
touched: the new window is identified by window-id diff and everything is torn
down by recorded PIDs. No hardware access anywhere; the fixture peer is
read-only.

Fixture truth was measured on the wire before each app launch
(`gdbus call io.github.orbiscontrol.Session1.PowerLimits`), not assumed:

```
SPL / PL1             45 Вт   (20–80, шаг 5, default 45)
NVIDIA Dynamic Boost  15 Вт   (0–25,  шаг 5, default 10)
Целевая температура GPU 75 °C (60–87, шаг 1, default 80)
SPPT / FPPT / CPU-temp: not provided by the fixture (absent from the wire)
```

Nav measurement: `tools/probe_nav2.py` (methodology audited in D2 run
t_86a3dbba, EVIDENCE.md). Row bands are scanned in the sidebar icon slab
(x 12..45) below the titlebar; captions/selected-rail are excluded by left-ink
position; icon boxes are expected at absolute x 22..40 (ink 23..25 at density
1), labels at x 51 (measured ink left 52). Light-theme ink cutoff recalibrated
from live pixels: ink core measures sum 342–386 vs background 707–747, cutoff
550 (probe records its calibration inline). Dark cutoff unchanged (>210).

Runner: `tools/run_live.sh <theme> <W> <H>`; per run it captures
dashboard (nav measurement), clicks Производительность (nav interaction),
captures the performance section, scrolls the section area (mouse wheel over
the content region) and captures again so the below-fold GPU limits rows are
reachable and visible at 980x680.

## Defect found and fixed during verification

The first live run rendered six LimitRows: the three backend-reported fields
plus "SPPT / PL2", "FPPT / PL3", "Лимит температуры CPU" as "0 ?" rows with
degenerate "0–0 · шаг 1" sliders. This contradicts the approved composition
(performance.slint: rows render only fields the backend actually reports —
`unit != ""` gates) and the honest-evidence invariant.

Root cause: the per-event UI round trip. `handle_worker_event`
(crates/orbis-ui/src/main.rs) rebuilds the whole controller state via
`from_slint(app.get_ui_state())`; `power_limits_from_slint` re-inserted all six
canonical fields whenever `power_limits_ready`, turning the projection defaults
of absent fields (0/0/step 1/empty unit, `unit_from_label("") → Unit::Unknown`)
back into authoritative entries. After any unrelated worker event, rows the
backend never reported appeared.

Fix: `read_power_value` (crates/orbis-ui/src/main.rs) now treats the empty
projected unit as "field not reported by the backend" and returns None — the
same semantics as the Slint `unit != ""` gates. Regression test:
`absent_power_limit_fields_do_not_resurrect_through_ui_round_trip`
(crates/orbis-ui/src/main_tests.rs) — RED before the fix (round trip produced 6
fields from 1), GREEN after.

Post-fix, all four theme×size cells show exactly three LimitRows (SPL,
Dynamic Boost, GPU temp target) and zero "0 ?" rows; absent-field rows are
honestly absent.

## Results

Nav geometry (probe_nav2.py, JSON next to each capture):

| cell               | nav rows | icon_left spread | label_left spread | verdict |
|--------------------|----------|------------------|-------------------|---------|
| dark 980x680       | 8        | 2 px             | 0 px              | PASS    |
| light 980x680      | 8        | 2 px             | 0 px              | PASS    |
| dark 1200x800      | 8        | 2 px             | 0 px              | PASS    |
| light 1200x800     | 8        | 2 px             | 0 px              | PASS    |

All measured icon ink lefts are 23/24/25 — inside the audited expected range
[23, 25] for the 18x18 icon box at x 22..40 (F4a: constant icon column). All
nav labels start at x 52 on every row (F4b: constant label column). Captions
(УПРАВЛЕНИЕ / ПРИЛОЖЕНИЕ) and the selected-row rail are excluded by the D2
left-ink rule and are not nav rows.

Dynamic Boost row (vision-read from cap-perf-scrolled captures, cross-checked
against the wire fixture above):

| cell               | NVIDIA Dynamic Boost row            | "0 ?" rows | no invented CPU-boost row |
|--------------------|-------------------------------------|------------|---------------------------|
| dark 980x680       | 15 Вт, slider 0–25, "0–25 · шаг 5"  | none       | yes (absent)              |
| light 980x680      | 15 Вт, slider 0–25, "0–25 · шаг 5"  | none       | yes (absent)              |
| dark 1200x800      | 15 Вт, slider 0–25, "0–25 · шаг 5"  | none       | yes (absent)              |
| light 1200x800     | 15 Вт, slider 0–25, "0–25 · шаг 5"  | none       | yes (absent)              |

The card headline states the honest write status ("Только чтение:
подтверждённый privileged backend записи отсутствует") — the fixture provides
no Hardware1 writer, so sliders are disabled; that is the approved read-only
rendering, not a defect.

Interactions exercised live (each run): window appears at the requested size
(geometry-dashboard.txt records exact 980x680 / 1200x800), nav click
Производительность switches to the performance section (cap-perf-*.png), mouse
wheel scroll over the content region makes the below-fold limits card visible
(cap-perf-scrolled-*.png).

## Files

- live2/<theme>-<WxH>/cap-dashboard-*.png — dashboard, nav measurement input
- live2/<theme>-<WxH>/nav-measure.json — probe output
- live2/<theme>-<WxH>/cap-perf-*.png / cap-perf-scrolled-*.png — limits card
- live2/<theme>-<WxH>/fixture-powerlimits.txt — fixture truth from the wire
- live2/<theme>-<WxH>/geometry-dashboard.txt — exact window geometry
- live2/<theme>-<WxH>/power-limit-log.txt — power-limit/panic/error log filter
- tools/run_live.sh, tools/fake-peer/, tools/probe_nav2.py,
  tools/probe_nav.py, tools/crop_br.py, tools/crop_region.py,
  tools/sample_slab.py, tools/probe_dbus_env.sh

## Scope note

The card scopes ONE live nav measurement; the full nav matrix (980/1200 ×
themes) is UI-D2 (t_86a3dbba, completed). All four cells were refreshed here
because the "0 ?" fix changed rendering after D2's captures; the fix does not
touch nav geometry and D2's nav verdicts remain valid.
