# UI Geometry & Symmetry Audit — baseline (t_5fc90d28)

Scope: audited Slint UI (`ui/audited`, `ui/components`, `ui/themes`) at 980×680 and 1200×800,
dark + light, states normal/dirty/pending/error/unsupported/readonly.
Method: 84 baseline screenshots rendered with `cargo run -p orbis-ui --example ui_snapshot -- --screenshot …`
(software renderer, offscreen) + zoomed visual inspection + pixel measurements.
**No production code was changed.** Evidence PNGs: `/home/artt/.hermes/profiles/ui/cache/scratch/audit-baseline/` (84 files, naming `<section>-<WxH>-<theme>[-<state>].png`).

Contract reference: REQ-VISUAL / SC-VISUAL-980 / SC-VISUAL-STATES (acceptance-contract.yaml:119-128),
interaction-contract.md THEME-SWITCH ("Обе темы читаемы на 980x680 и 1200x800 без overlap/clipping").

---

## Confirmed defects

### D-AUD-G01 · HIGH · Dashboard is the only page with a 14px gutter (all others 28px)
- Where: `ui/audited/sections/dashboard.slint:74-77` — `width: root.width - 28px; x: 14px; padding-top: 4px`.
- Every other page (performance, power, cooling, graphics, backlight, display, system, settings, about)
  uses `width: root.width - 56px; x: 28px; padding-top: 8px`.
- Observable: on Dashboard the content card column starts 14px from the sidebar (vs 28px everywhere else),
  ends 14px from the window edge (vs 28px), and the page header sits 4px higher.
  Cross-page nav flips show the whole card column jump ~14px left/right.
- Evidence: `dashboard-980x680-dark.png`, `dashboard-1200x800-dark.png` vs `graphics-980x680-dark.png`.
- Fix (mechanical): dashboard.slint:74 `width: root.width - 56px`, :75 `x: 28px`, :77 `padding-top: 8px`.

### D-AUD-G02 · HIGH · Performance page reserves ~170px of phantom space for an invisible card
- Where: `ui/audited/sections/performance.slint:122` — big "Лимиты мощности и температуры" card uses
  `visible: root.ui-state.power-limits-ready`. Slint `visible: false` hides without collapsing the
  element, so the card still occupies its full height in the VerticalLayout; the real fallback card
  (`if (!power-limits-ready)`, :143) renders *below* the hole.
- Observable: whenever power limits are not Ready (backend Loading — the production startup state —
  and the unsupported scenario), there is a ~170px empty band between the profile card and the
  "Лимиты мощности и температуры" title card. Reproduces at both sizes and both themes.
- Evidence: `performance-980x680-dark.png`, `performance-1200x800-light.png` (gap between profile card
  bottom and the limits title card), `performance-*-unsupported.png` (same gap).
- Same pattern class (currently harmless but inconsistent): `cooling.slint:119,124` hide the
  "Остаться"/"Отбросить" buttons with `visible:`; they pack invisibly to the left of the right-aligned
  action row (no visible hole today because the row is right-aligned — verified `cooling-980x680-dark.png`).
- Fix: replace the `visible:` card + trailing `if` fallback pair with a single conditional pair
  (`if (root.ui-state.power-limits-ready) : SectionCard {…}` / `if (!…) : SectionCard {…}`) so the
  hidden state reserves no space. Optionally convert cooling.slint:119/124 to `if` for consistency.

### D-AUD-G03 · MED-HIGH · About card: paragraph text overflows the card's right border (~20px)
- Where: `ui/audited/sections/about.slint:61` and `:68` — `Text { width: parent.width; … }` directly
  inside the padded VerticalLayout (`padding: 20px`) of the SectionCard. In Slint, `parent` is the
  layout element whose width *includes* padding; the explicit width overrides the layout's
  padding-constrained child width, so the text box starts at the padded x but extends past the
  right padding edge and crosses the card border.
- Observable: second paragraph ("Интерфейс построен на данных…") touches/crosses the card's right
  border line on both themes; first paragraph reaches the border at 980. Reproduces at 980 and 1200.
- Evidence: `about-1200x800-light.png` (text visibly crosses the border), zoom crop;
  `about-980x680-dark.png` (text reaches the border).
- Fix: delete the explicit `width: parent.width` on both Texts (the VerticalLayout already stretches
  children to the padded content width; `wrap: word-wrap` keeps working). No other property changes.
- Pattern sweep: the other `width: parent.width` occurrences are safe —
  `fan-curve-editor-v3.slint:83` (parent = tile Rectangle, centered text), `preview-dialog-window.slint:31`
  (symmetric `x:14px, width-28px`), `cooling.slint:109` (parent = inner unpadded layout),
  chart/slider/stepper usages are Rectangle-math inside fixed boxes.

### D-AUD-G04 · MED · Sidebar nav rows: icon top-aligned instead of centered, and the icon column zigzags
- Where: `ui/components/nav-item.slint:25-44`. The row's HorizontalLayout stretches children to the
  44px row height; the fixed 18px Icon has no vertical centering, so it sits at the row top while the
  Text centers its glyphs vertically → icon floats ~13px above the label's visual center.
  Additionally `alignment: center` on that layout centers each row's icon+label *group* horizontally,
  so the icon x-position varies per row with label length (measured: «Производительность» icon x≈34,
  «Питание» x≈71, «Охлаждение» x≈59, «Экран» x≈78) — the icon column is ragged.
- Observable: every nav row shows the icon high relative to its label; the icon column does not form
  a straight vertical line. Reproduces in both themes, both sizes, all states.
- Evidence: zoomed crops of `settings-980x680-dark.png` (rows «Производительность», «Питание») and
  `system-1200x800-dark.png` (all rows); measurement script
  `scripts/ui-audit-measure-nav.py` (cluster output is approximate; the zoomed crops are decisive).
- Codebase precedent for the fix: `components/mode-card.slint:44-68` wraps the icon in a centering
  VerticalLayout (explicit comment about renderer placement), `components/metric-card.slint:31-35`
  centers with explicit `y:` — NavItem is the only icon+label row missing the centering wrapper.
- Fix: (a) wrap NavItem's Icon in `VerticalLayout { alignment: center; }` (ModeCard pattern) —
  unambiguous defect fix; (b) decide the horizontal treatment: left-align rows (`alignment: start`,
  icon column at a constant x — recommended, standard nav alignment, also matches the reference
  icon+label language) and align the TitleBar identity group to the same column
  (`components/title-bar.slint:57-89` currently centers its icon+title in the 230px box).
  Part (b) slightly changes the approved look → planner sign-off recommended.

---

## Coverage notes for the planner (no action in this card)

1. **Harness gap:** `crates/orbis-ui/examples/ui_snapshot.rs` never populates the power-limit fields
   (`spl-unit`, `sppt-unit`, `fppt-unit`, cpu-temp/boost/gpu-temp units, `power-limits-ready=true`,
   draft/dirty masks). The LimitRow grid — the largest interactive block on Performance — is not
   rendered in *any* of the 84 screenshots, and no scenario exercises power-limit dirty/pending rows
   required by SC-VISUAL-STATES ("drive_normal_dirty_pending_error_states"). Recommend extending
   `demo_state`/scenarios in a follow-up card before the WS-UI repair work is QA'd.
2. Hidden-element semantics: after F2, verify that the Loading→Ready transition doesn't reintroduce a
   gap (conditional `if` swaps are layout-stable).
3. Clean areas verified (no defects found): Performance profile cards & LimitRow-adjacent spacing,
   Cooling chart/steppers/tabs at both sizes, Graphics mode cards + chips (incl. pending/unsupported),
   Power slider+InfoRows, Backlight buttons+rows, Display mode buttons, Settings toggles/choices,
   System rows, TitleBar buttons, PreviewDialog (exactly 430×220, margins symmetric).
   No horizontal overflow / horizontal scrolling of main content found on any page at 980×680.
   No theme-specific geometry differences found (all four defects reproduce in dark and light).

## Fix plan (proposed order, all WS-UI)

| # | Defect | File(s) | Change | Risk |
|---|--------|---------|--------|------|
| F1 | G01 | sections/dashboard.slint:74-77 | 14px gutter → 28px (3 values) | trivial |
| F2 | G02 | sections/performance.slint:121-148 | `visible:` card + `if` fallback → conditional pair; optionally cooling.slint:119/124 | small; re-render states |
| F3 | G03 | sections/about.slint:61,68 | drop `width: parent.width` | trivial |
| F4a | G04 | components/nav-item.slint | center icon vertically (ModeCard pattern) | small |
| F4b | G04 | components/nav-item.slint + components/title-bar.slint | left-align nav column, align TitleBar identity to same x | design sign-off |
| F5 | harness | examples/ui_snapshot.rs | populate power-limit fields + dirty/pending scenarios | small; enables QA of F2 |

Verification after fixes (per SC-VISUAL-980/STATES): re-run the 84-shot matrix, diff against this
baseline, and confirm: Dashboard gutter == 28px; Performance has no phantom band in
loading/unsupported; About text inside borders with ≥16px right padding; nav icons centered per row
and (if F4b approved) forming a straight column at a constant x.
