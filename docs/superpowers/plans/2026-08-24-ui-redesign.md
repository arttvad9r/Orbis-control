# UI Redesign (single window, 4 sections, Catppuccin) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the multi-window Orbis UI with one frameless 760×600 window (sidebar: Dashboard / Fans / Hardware / Settings), Catppuccin Mocha/Latte theme, custom title bar with drag/minimize/close, and remove the Updates and Automation surfaces from the UI.

**Architecture:** Only the UI layer changes. `UiState`, worker/controller/session wiring stay untouched. The compiled entry stays `ui/app-entry.slint` → `AppWindow` (shell) hosting four section components; old secondary windows are deleted together with their Rust glue in one atomic switch-over task. Dragging uses slint feature `unstable-winit-030` → winit `drag_window()`; minimize uses `slint::Window::set_minimized` (both verified in locked slint 1.13.1 / winit 0.30.13).

**Tech Stack:** Slint 1.13 (fluent style, software renderer for screenshots), Rust, `check-ui-contract.py` source contracts, `ui-review` screenshot builds.

**Spec:** `docs/superpowers/specs/2026-08-24-ui-redesign-design.md`

## Global Constraints

- Production mock fallback stays forbidden; `ui-review` stays out of release packaging.
- No new functions/mutations; safety gates untouched; worker/controller/session clients unchanged (except deletion of dead updates/automation window glue in main.rs).
- Truthful states: disabled/pending/unavailable evidence semantics preserved verbatim.
- Main window contract after change: fixed 760×600 logical px; `check-ui-contract.py` bounds become 400≤w≤800 and 400≤h≤640.
- All builds/tests run with `--locked`. Commit scope `ui:`.

## File Map (final state)

- `ui/themes/dark.slint`, `ui/themes/light.slint` — Catppuccin values (names unchanged).
- `ui/assets/icons/{dashboard,fan,cpu,gear,window-minimize,window-close}.svg` — new.
- `ui/components/title-bar.slint`, `sidebar.slint`, `dropdown-row.slint` — new.
- `ui/audited/sections/{dashboard,fans,hardware,settings}.slint` — new section components.
- `ui/audited/main-window.slint` — becomes the shell (TitleBar + Sidebar + section switch).
- `ui/audited/model.slint` — gains `Section` enum.
- Deleted: `ui/audited/{fans,extra,preferences,diagnostics,automation,updates}-window.slint`, `ui/components/mode-card.slint`, `ui/app-window.slint` (legacy, not compiled).
- Kept: `ui/audited/preview-dialog-window.slint`, `fan-curve-editor-v3.slint`, `common.slint`, `ui/components/{segmented-control,value-slider,visual-controls,request-toggle-row}.slint`.
- `crates/orbis-ui/src/main.rs` — window glue reworked; `--ui-section` screenshot param; drag/minimize/close wiring.
- `crates/orbis-ui/Cargo.toml` — slint features `svg`, `unstable-winit-030`.
- `scripts/check-ui-contract.py`, `crates/orbis-ui/src/main_tests.rs` — contract updates.
- Docs: `docs/ui-reference.md`, `docs/current-state.md`, `docs/backend-completion-status.md`.

---

### Task 1: Catppuccin theme tokens

**Files:**
- Modify: `ui/themes/dark.slint`
- Modify: `ui/themes/light.slint`

**Interfaces:** Produces: same `Palette`/`LightPalette` token names with new values — no consumer changes.

- [ ] **Step 1: Replace dark values in `ui/themes/dark.slint`**

Keep file structure (`ThemeState`, `Palette` ternaries). Replace every dark-branch value and the header comment ("Catppuccin Mocha — https://catppuccin.com"):

```slint
    in property <color> window-background: ThemeState.mode == ThemeMode.Light ? LightPalette.window-background : #1E1E2E;
    in property <color> titlebar-background: ThemeState.mode == ThemeMode.Light ? LightPalette.titlebar-background : #181825;
    in property <color> surface-default: ThemeState.mode == ThemeMode.Light ? LightPalette.surface-default : #313244;
    in property <color> surface-hover: ThemeState.mode == ThemeMode.Light ? LightPalette.surface-hover : #45475A;
    in property <color> surface-pressed: ThemeState.mode == ThemeMode.Light ? LightPalette.surface-pressed : #181825;
    in property <color> surface-selected: ThemeState.mode == ThemeMode.Light ? LightPalette.surface-selected : #414356;
    in property <color> surface-disabled: ThemeState.mode == ThemeMode.Light ? LightPalette.surface-disabled : #262637;
    in property <color> border-default: ThemeState.mode == ThemeMode.Light ? LightPalette.border-default : #45475A;
    in property <color> border-strong: ThemeState.mode == ThemeMode.Light ? LightPalette.border-strong : #585B70;
    in property <color> text-primary: ThemeState.mode == ThemeMode.Light ? LightPalette.text-primary : #CDD6F4;
    in property <color> text-secondary: ThemeState.mode == ThemeMode.Light ? LightPalette.text-secondary : #A6ADC8;
    in property <color> text-disabled: ThemeState.mode == ThemeMode.Light ? LightPalette.text-disabled : #6C7086;
    in property <color> text-on-accent: ThemeState.mode == ThemeMode.Light ? LightPalette.text-on-accent : #11111B;
    in property <color> accent-default: ThemeState.mode == ThemeMode.Light ? LightPalette.accent-default : #89B4FA;
    in property <color> warning: ThemeState.mode == ThemeMode.Light ? LightPalette.warning : #F9E2AF;
    in property <color> error: ThemeState.mode == ThemeMode.Light ? LightPalette.error : #F38BA8;
    in property <color> success: ThemeState.mode == ThemeMode.Light ? LightPalette.success : #A6E3A1;
    in property <color> silent: ThemeState.mode == ThemeMode.Light ? LightPalette.silent : #A6E3A1;
    in property <color> balanced: ThemeState.mode == ThemeMode.Light ? LightPalette.balanced : #89B4FA;
    in property <color> turbo: ThemeState.mode == ThemeMode.Light ? LightPalette.turbo : #F38BA8;
    in property <color> eco: ThemeState.mode == ThemeMode.Light ? LightPalette.eco : #A6E3A1;
    in property <color> standard: ThemeState.mode == ThemeMode.Light ? LightPalette.standard : #89B4FA;
    in property <color> ultimate: ThemeState.mode == ThemeMode.Light ? LightPalette.ultimate : #FAB387;
    in property <color> optimized: ThemeState.mode == ThemeMode.Light ? LightPalette.optimized : #94E2D5;
```

- [ ] **Step 2: Replace light values in `ui/themes/light.slint`**

```slint
// Catppuccin Latte — https://catppuccin.com
export global LightPalette {
    in property <color> window-background: #EFF1F5;
    in property <color> titlebar-background: #E6E9EF;
    in property <color> surface-default: #CCD0DA;
    in property <color> surface-hover: #BCC0CC;
    in property <color> surface-pressed: #DCE0E8;
    in property <color> surface-selected: #C6CEE0;
    in property <color> surface-disabled: #E0E3EA;
    in property <color> border-default: #9CA0B0;
    in property <color> border-strong: #7C7F93;
    in property <color> text-primary: #4C4F69;
    in property <color> text-secondary: #6C6F85;
    in property <color> text-disabled: #9CA0B0;
    in property <color> text-on-accent: #EFF1F5;
    in property <color> accent-default: #1E66F5;
    in property <color> warning: #DF8E1D;
    in property <color> error: #D20F39;
    in property <color> success: #40A02B;
    in property <color> silent: #40A02B;
    in property <color> balanced: #1E66F5;
    in property <color> turbo: #D20F39;
    in property <color> eco: #40A02B;
    in property <color> standard: #1E66F5;
    in property <color> ultimate: #FE640B;
    in property <color> optimized: #179299;
}
```

- [ ] **Step 3: Build + screenshot**

Run: `cargo build -p orbis-ui --features ui-review --locked && ./target/debug/orbis-control --screenshot /tmp/opencode/t1-theme.png`
Expected: build PASS; screenshot background is Mocha base `#1E1E2E`.

- [ ] **Step 4: Commit**

```bash
git add ui/themes/dark.slint ui/themes/light.slint
git commit -m "ui: catppuccin mocha/latte theme tokens"
```

---

### Task 2: Cargo features + icon assets

**Files:**
- Modify: `crates/orbis-ui/Cargo.toml`
- Create: `ui/assets/icons/{dashboard,fan,cpu,gear,window-minimize,window-close}.svg`

**Interfaces:** Produces: slint features `svg` + `unstable-winit-030`; icons referenced later via `@image-url("/assets/icons/<name>.svg")` (`ui/` is the include root in `build.rs`).

- [ ] **Step 1: Extend slint features in `crates/orbis-ui/Cargo.toml`**

```toml
slint = { workspace = true, features = ["svg", "unstable-winit-030"] }
```

- [ ] **Step 2: Create six icons**

Common attributes for every file: `width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="#CDD6F4" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"`.

`dashboard.svg` body:
```svg
<rect x="1.5" y="1.5" width="5.5" height="5.5" rx="1.5"/><rect x="9" y="1.5" width="5.5" height="5.5" rx="1.5"/><rect x="1.5" y="9" width="5.5" height="5.5" rx="1.5"/><rect x="9" y="9" width="5.5" height="5.5" rx="1.5"/>
```

`fan.svg` body:
```svg
<circle cx="8" cy="8" r="1.8"/><path d="M8 6.2C8 3.5 9.5 2 11.5 2c1 2-.5 4-3.5 4.2"/><path d="M9.6 9c2.3 1.4 4.4 1 5.2-.8-1.3-1.8-3.8-1.6-5.2.8z"/><path d="M6.4 9c-2.3 1.4-2.7 3.5-1.4 4.9 2.1-.3 3-2.6 1.4-4.9z"/>
```

`cpu.svg` body:
```svg
<rect x="3" y="3" width="10" height="10" rx="2"/><rect x="6" y="6" width="4" height="4"/><path d="M8 1v2M8 13v2M1 8h2M13 8h2M5 1v2M11 1v2M5 13v2M11 13v2M1 5h2M1 11h2M13 5h2M13 11h2"/>
```

`gear.svg` body:
```svg
<circle cx="8" cy="8" r="2.2"/><path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M12.6 3.4l-1.4 1.4M4.8 11.2l-1.4 1.4"/>
```

`window-minimize.svg` body:
```svg
<path d="M3.5 8h9"/>
```

`window-close.svg` body:
```svg
<path d="M4 4l8 8M12 4l-8 8"/>
```

- [ ] **Step 3: Build**

Run: `cargo build -p orbis-ui --features ui-review --locked`
Expected: PASS (features accepted; icons not yet referenced).

- [ ] **Step 4: Commit**

```bash
git add crates/orbis-ui/Cargo.toml ui/assets/icons
git commit -m "ui: svg icon assets and slint svg/winit features"
```

---

### Task 3: Section enum + TitleBar + Sidebar + DropdownRow

**Files:**
- Modify: `ui/audited/model.slint`
- Create: `ui/components/title-bar.slint`
- Create: `ui/components/sidebar.slint`
- Create: `ui/components/dropdown-row.slint`

**Interfaces:**
- Produces:
  - `Section` enum: `export enum Section { Dashboard, Fans, Hardware, Settings }`
  - `TitleBar { in section-title: string; callback minimize-requested; callback close-requested; callback drag-started; }` — 40px.
  - `Sidebar { in-out property <Section> active; callback section-selected(Section); }` — 168px.
  - `DropdownRow { in label: string; in options: [string]; in-out property <int> current-index; in enabled-row: bool; in status: string; callback row-changed(int); }` — 30px.

- [ ] **Step 1: Add enum to `ui/audited/model.slint`**

```slint
export enum Section { Dashboard, Fans, Hardware, Settings }
```

- [ ] **Step 2: Create `ui/components/title-bar.slint`**

Declaration order matters: the drag `TouchArea` is declared first (bottom), the button row after (top), so buttons win clicks and the rest of the bar drags.

```slint
// Frameless title bar: section label, drag surface, minimize/close (spec §3).
import { Palette } from "../themes/dark.slint";

export component TitleBar inherits Rectangle {
    in property <string> section-title: "Dashboard";
    callback minimize-requested;
    callback close-requested;
    callback drag-started;

    height: 40px;
    background: Palette.titlebar-background;

    drag-touch := TouchArea {
        pointer-event(event) => {
            if (event.kind == PointerEventKind.down) { root.drag-started(); }
        }
    }

    HorizontalLayout {
        padding-left: 14px;
        padding-right: 6px;
        alignment: center;
        Text {
            text: root.section-title;
            font-size: 11px;
            font-weight: 600;
            color: Palette.text-secondary;
            vertical-alignment: center;
        }
        Rectangle { horizontal-stretch: 1; background: transparent; }
        Rectangle {
            width: 28px;
            property <bool> hover: min-touch.has-hover;
            background: root.hover ? Palette.surface-hover : transparent;
            border-radius: 6px;
            Image {
                x: (parent.width - self.width) / 2;
                y: (parent.height - self.height) / 2;
                source: @image-url("/assets/icons/window-minimize.svg");
                width: 16px; height: 16px;
            }
            min-touch := TouchArea { clicked => { root.minimize-requested(); } }
        }
        Rectangle {
            width: 28px;
            property <bool> hover: close-touch.has-hover;
            background: root.hover ? Palette.error : transparent;
            border-radius: 6px;
            Image {
                x: (parent.width - self.width) / 2;
                y: (parent.height - self.height) / 2;
                source: @image-url("/assets/icons/window-close.svg");
                width: 16px; height: 16px;
                colorize: root.hover ? #11111B : Palette.text-secondary;
            }
            close-touch := TouchArea { clicked => { root.close-requested(); } }
        }
    }
}
```

- [ ] **Step 3: Create `ui/components/sidebar.slint`** (explicit items — no `for` loops over mixed enum/icon data)

```slint
// Left navigation: four sections with line icons (spec §2).
import { Section } from "../audited/model.slint";
import { Palette } from "../themes/dark.slint";

component NavItem inherits Rectangle {
    in property <string> label;
    in property <image> icon;
    in property <bool> is-active;
    callback selected;
    height: 34px;
    border-radius: 8px;
    background: root.is-active ? Palette.surface-selected
        : touch.has-hover ? Palette.surface-hover : transparent;
    HorizontalLayout {
        padding-left: 10px;
        spacing: 10px;
        Image {
            source: root.icon;
            width: 16px; height: 16px;
            colorize: root.is-active ? Palette.accent-default : Palette.text-secondary;
            vertical-alignment: center;
        }
        Text {
            text: root.label;
            font-size: 11px;
            font-weight: 600;
            color: root.is-active ? Palette.text-primary : Palette.text-secondary;
            vertical-alignment: center;
        }
    }
    touch := TouchArea { clicked => { root.selected(); } }
}

export component Sidebar inherits Rectangle {
    in-out property <Section> active: Section.Dashboard;
    callback section-selected(Section);
    width: 168px;
    background: Palette.titlebar-background;
    VerticalLayout {
        padding: 10px;
        spacing: 4px;
        NavItem { label: "Dashboard"; icon: @image-url("/assets/icons/dashboard.svg"); is-active: root.active == Section.Dashboard; selected => { root.section-selected(Section.Dashboard); } }
        NavItem { label: "Fans"; icon: @image-url("/assets/icons/fan.svg"); is-active: root.active == Section.Fans; selected => { root.section-selected(Section.Fans); } }
        NavItem { label: "Hardware"; icon: @image-url("/assets/icons/cpu.svg"); is-active: root.active == Section.Hardware; selected => { root.section-selected(Section.Hardware); } }
        NavItem { label: "Settings"; icon: @image-url("/assets/icons/gear.svg"); is-active: root.active == Section.Settings; selected => { root.section-selected(Section.Settings); } }
        Rectangle { vertical-stretch: 1; background: transparent; }
    }
}
```

- [ ] **Step 4: Create `ui/components/dropdown-row.slint`**

```slint
// Label + ComboBox + trailing status: one compact 30px row (spec §2.1).
import { ComboBox } from "std-widgets.slint";
import { Palette } from "../themes/dark.slint";

export component DropdownRow inherits Rectangle {
    in property <string> label;
    in property <[string]> options;
    in-out property <int> current-index;
    in property <bool> enabled-row: false;
    in property <string> status: "";
    callback row-changed(int);

    height: 30px;
    HorizontalLayout {
        spacing: 8px;
        alignment: center;
        Text {
            width: 92px;
            text: root.label;
            font-size: 10px;
            font-weight: 600;
            color: root.enabled-row ? Palette.text-primary : Palette.text-disabled;
            vertical-alignment: center;
        }
        ComboBox {
            width: 132px;
            enabled: root.enabled-row;
            model: root.options;
            current-index <=> root.current-index;
            selected => { root.row-changed(self.current-index); }
        }
        Rectangle { horizontal-stretch: 1; background: transparent; }
        Text {
            text: root.status;
            font-size: 9px;
            color: Palette.text-secondary;
            vertical-alignment: center;
            horizontal-alignment: right;
            overflow: elide;
        }
    }
}
```

- [ ] **Step 5: Export new components from `ui/app-entry.slint`** (so slint-build compiles them now)

```slint
export { TitleBar } from "components/title-bar.slint";
export { Sidebar } from "components/sidebar.slint";
export { DropdownRow } from "components/dropdown-row.slint";
```

- [ ] **Step 6: Build**

Run: `cargo build -p orbis-ui --features ui-review --locked`
Expected: PASS (icons resolve; components compile).

- [ ] **Step 7: Commit**

```bash
git add ui/audited/model.slint ui/components/title-bar.slint ui/components/sidebar.slint ui/components/dropdown-row.slint ui/app-entry.slint
git commit -m "ui: section enum, title bar, sidebar, dropdown row"
```

---

### Task 4: Dashboard section

**Files:**
- Create: `ui/audited/sections/dashboard.slint`

**Interfaces:**
- Consumes: `UiState` (model.slint), `SegmentedTrack`/`Segment`, `DropdownRow`, `ValueSlider`, `SectionTitle`/`ActionButton`/`LocalStatus` (common.slint), `Palette`.
- Produces: `DashboardSection` with exactly the AppWindow quick-control surface (same callback names as today's AppWindow so Rust wiring only re-points, not renames):

```slint
in property <UiState> ui-state;
in property <bool> display-state-ready: false;
in property <bool> display-control-ready: false;
in property <int> display-mode: -1;
in property <string> display-status: "Unavailable";
callback display-mode-requested(int);
in property <bool> keyboard-state-ready: false;
in property <bool> keyboard-control-ready: false;
in property <int> keyboard-brightness: -1;
in property <string> keyboard-status: "Unavailable";
callback keyboard-brightness-requested(int);
callback perf-clicked(int);
callback gpu-clicked(int);
callback charge-changed(float);
```

- [ ] **Step 1: Create `ui/audited/sections/dashboard.slint`**

Move from the current `ui/audited/main-window.slint` (pre-Task-8 content) verbatim: `bit-set`, `perf-label`, `gpu-label`, `perf-detail`, `gpu-detail` functions and the Performance + GPU section blocks (SectionTitle + SegmentedTrack rows + LocalStatus rows). Replace the old Quick Controls `SectionCard` (Display/Keyboard chip rows) with two `DropdownRow`s, and the Battery `SectionCard` with one compact row:

```slint
import { UiState, ChargeLimitState, GpuHwState, PerformanceHwState, GpuModeHwState } from "../model.slint";
import { SegmentedTrack, Segment } from "../../components/segmented-control.slint";
import { DropdownRow } from "../../components/dropdown-row.slint";
import { ValueSlider } from "../../components/value-slider.slint";
import { SectionTitle, ActionButton, LocalStatus } from "../common.slint";
import { Palette } from "../../themes/dark.slint";

export component DashboardSection inherits Rectangle {
    in property <UiState> ui-state;
    in property <bool> display-state-ready: false;
    in property <bool> display-control-ready: false;
    in property <int> display-mode: -1;
    in property <string> display-status: "Unavailable";
    callback display-mode-requested(int);
    in property <bool> keyboard-state-ready: false;
    in property <bool> keyboard-control-ready: false;
    in property <int> keyboard-brightness: -1;
    in property <string> keyboard-status: "Unavailable";
    callback keyboard-brightness-requested(int);
    callback perf-clicked(int);
    callback gpu-clicked(int);
    callback charge-changed(float);

    function bit-set(mask: int, bit: int) -> bool {
        let shifted = Math.floor(mask / bit);
        return shifted - 2 * Math.floor(shifted / 2) == 1;
    }
    function perf-label() -> string {
        return root.ui-state.perf-state == PerformanceHwState.Unavailable ? "Performance · Unavailable" : "Performance";
    }
    function gpu-label() -> string {
        return root.ui-state.gpu-mode-state == GpuModeHwState.Unavailable ? "GPU Mode · Read only" : "GPU Mode";
    }
    function perf-detail() -> string { return "CPU " + root.ui-state.cpu-temp + " · Fan " + root.ui-state.cpu-fan-rpm; }
    function gpu-detail() -> string { return "GPU " + root.ui-state.gpu-temp + " · Fan " + root.ui-state.gpu-fan-rpm; }
    function display-row-status() -> string {
        if (!root.display-state-ready) { return "Unavailable"; }
        return root.display-status;
    }
    function keyboard-row-status() -> string {
        if (!root.keyboard-state-ready) { return "Unavailable"; }
        return root.keyboard-status;
    }
    function charge-detail() -> string {
        if (root.ui-state.charge-limit-state == ChargeLimitState.Loading) { return "Loading"; }
        if (root.ui-state.charge-limit-state != ChargeLimitState.Ready) { return "Unavailable"; }
        return root.ui-state.charge-limit + "%";
    }

    VerticalLayout {
        spacing: 10px;

        VerticalLayout {
            spacing: 4px;
            SectionTitle { title: root.perf-label(); detail: root.perf-detail(); }
            SegmentedTrack {
                Segment { label: "Silent"; accent: Palette.silent; selected: root.ui-state.perf-state == PerformanceHwState.Ready && root.ui-state.perf-selected == 0; segment-disabled: root.ui-state.perf-state != PerformanceHwState.Ready || !root.ui-state.perf-writable || !root.bit-set(root.ui-state.available-perf-mask, 1); activate => { root.perf-clicked(0); } }
                Segment { label: "Balanced"; accent: Palette.balanced; selected: root.ui-state.perf-state == PerformanceHwState.Ready && root.ui-state.perf-selected == 1; segment-disabled: root.ui-state.perf-state != PerformanceHwState.Ready || !root.ui-state.perf-writable || !root.bit-set(root.ui-state.available-perf-mask, 2); activate => { root.perf-clicked(1); } }
                Segment { label: "Turbo"; accent: Palette.turbo; selected: root.ui-state.perf-state == PerformanceHwState.Ready && root.ui-state.perf-selected == 2; segment-disabled: root.ui-state.perf-state != PerformanceHwState.Ready || !root.ui-state.perf-writable || !root.bit-set(root.ui-state.available-perf-mask, 4); activate => { root.perf-clicked(2); } }
            }
        }

        VerticalLayout {
            spacing: 4px;
            SectionTitle { title: root.gpu-label(); detail: root.gpu-detail(); }
            SegmentedTrack {
                Segment { label: "Eco"; accent: Palette.eco; selected: root.ui-state.gpu-mode-state == GpuModeHwState.Ready && root.ui-state.gpu-selected == 0; pending: root.ui-state.gpu-queued == 0; segment-disabled: root.ui-state.gpu-mode-state != GpuModeHwState.Ready || !root.ui-state.gpu-mode-writable || !root.bit-set(root.ui-state.available-gpu-mask, 1); activate => { root.gpu-clicked(0); } }
                Segment { label: "Standard"; accent: Palette.standard; selected: root.ui-state.gpu-mode-state == GpuModeHwState.Ready && root.ui-state.gpu-selected == 1; pending: root.ui-state.gpu-queued == 1; segment-disabled: root.ui-state.gpu-mode-state != GpuModeHwState.Ready || !root.ui-state.gpu-mode-writable || !root.bit-set(root.ui-state.available-gpu-mask, 2); activate => { root.gpu-clicked(1); } }
                Segment { label: "Ultimate"; accent: Palette.ultimate; selected: root.ui-state.gpu-mode-state == GpuModeHwState.Ready && root.ui-state.gpu-selected == 2; pending: root.ui-state.gpu-ultimate-pending || root.ui-state.gpu-queued == 2; segment-disabled: root.ui-state.gpu-mode-state != GpuModeHwState.Ready || !root.ui-state.gpu-mode-writable || root.ui-state.gpu-ultimate-disabled || !root.bit-set(root.ui-state.available-gpu-mask, 4); activate => { root.gpu-clicked(2); } }
                Segment { label: "Optimized"; accent: Palette.optimized; selected: root.ui-state.gpu-mode-state == GpuModeHwState.Ready && root.ui-state.gpu-selected == 3; segment-disabled: root.ui-state.gpu-mode-state != GpuModeHwState.Ready || !root.ui-state.gpu-mode-writable || !root.bit-set(root.ui-state.available-gpu-mask, 8); activate => { root.gpu-clicked(3); } }
            }
            if (root.ui-state.gpu-section-error) : LocalStatus { text: "GPU mode change was not confirmed"; accent: Palette.error; }
            if (root.ui-state.gpu-reboot-required && !root.ui-state.gpu-section-error) : LocalStatus { text: "GPU mode queued · applied at next shutdown/reboot"; accent: Palette.warning; }
        }

        DropdownRow {
            label: "Display";
            options: ["Auto", "60 Hz", "120 Hz"];
            current-index: root.display-mode < 0 ? 0 : root.display-mode;
            enabled-row: root.display-control-ready;
            status: root.display-row-status();
            row-changed(i) => { root.display-mode-requested(i); }
        }

        DropdownRow {
            label: "Keyboard";
            options: ["Off", "1", "2", "3"];
            current-index: root.keyboard-brightness < 0 ? 0 : root.keyboard-brightness;
            enabled-row: root.keyboard-control-ready;
            status: root.keyboard-row-status();
            row-changed(i) => { root.keyboard-brightness-requested(i); }
        }

        VerticalLayout {
            spacing: 4px;
            SectionTitle { title: "Battery Charge Limit"; detail: root.charge-detail(); }
            HorizontalLayout {
                height: 30px;
                spacing: 8px;
                alignment: center;
                if (root.ui-state.charge-limit-state == ChargeLimitState.Ready) : ValueSlider { horizontal-stretch: 1; min: 20; max: 100; step: 1; value: root.ui-state.charge-limit; disabled: !root.ui-state.charge-limit-writable; changed(v) => { root.charge-changed(v); } }
                if (root.ui-state.charge-limit-state != ChargeLimitState.Ready) : Text { horizontal-stretch: 1; text: root.ui-state.charge-limit-state == ChargeLimitState.Loading ? "Reading battery threshold…" : "Battery threshold unavailable"; font-size: 10px; color: Palette.text-secondary; vertical-alignment: center; }
                ActionButton { width: 58px; label: "100%"; disabled: root.ui-state.charge-limit-state != ChargeLimitState.Ready || !root.ui-state.charge-limit-writable; clicked => { root.charge-changed(100); } }
            }
            HorizontalLayout {
                height: 16px;
                Text { text: root.ui-state.battery-percent + " · " + root.ui-state.battery-health; font-size: 9px; color: Palette.text-secondary; vertical-alignment: center; overflow: elide; }
                Rectangle { horizontal-stretch: 1; background: transparent; }
                Text { text: root.ui-state.power-ac-mw; font-size: 9px; color: root.ui-state.telemetry-fresh ? Palette.text-secondary : Palette.text-disabled; vertical-alignment: center; horizontal-alignment: right; overflow: elide; }
            }
        }
    }
}
```

Note: `DropdownRow.current-index` is `in-out`; the `display-mode: -1` "not ready" mapping uses the ternary above so the ComboBox never shows a negative index. When `display-mode` updates from backend, the binding expression re-evaluates (one-way into the row is acceptable; user selection flows back through `row-changed` → Rust → `display-mode` property).

- [ ] **Step 2: Export from `ui/app-entry.slint`** (temporary, so it compiles before the shell lands)

```slint
export { DashboardSection } from "audited/sections/dashboard.slint";
```

- [ ] **Step 3: Build**

Run: `cargo build -p orbis-ui --features ui-review --locked`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add ui/audited/sections/dashboard.slint ui/app-entry.slint
git commit -m "ui: dashboard section with dropdown quick rows"
```

---

### Task 5: Fans section

**Files:**
- Create: `ui/audited/sections/fans.slint`

**Interfaces:**
- Consumes: `FanCurveEditor` (fan-curve-editor-v3.slint), `UiState`, `LocalStatus`/`ActionButton` (common.slint), `Palette`.
- Produces: `FansSection` — same surface as the old FansWindow minus the Window wrapper and WindowHeader:

```slint
in property <UiState> ui-state;            // bound directly to AppWindow's state
in property <bool> mutation-safety-blocked: true;
in property <bool> factory-reset-available: false;
in property <string> policy-status: "Fan writes are safety-blocked";
callback fan-changed(int);
callback fan-profile-changed(int);
callback fan-temp-point-changed(int, int);
callback fan-pwm-point-changed(int, int);
callback fan-apply-clicked(bool);
```

Note: `curve-enabled-known`/`curve-enabled` already live inside `UiState` (fields `fan-curve-enabled-known`/`fan-curve-enabled`), so the old separate properties disappear — the section reads them from `ui-state` (the old window duplicated them; `sync_fans_window` in main.rs will be deleted in Task 9).

- [ ] **Step 1: Create `ui/audited/sections/fans.slint`**

Copy `ui/audited/fans-window.slint` and apply exactly these changes:
1. `component FansWindow inherits Window` → `export component FansSection inherits Rectangle`; drop `width/height/title/background` window lines.
2. Drop the `WindowHeader` block (the shell's TitleBar already names the section).
3. Replace the two properties `curve-enabled-known`/`curve-enabled` with reads from `root.ui-state.fan-curve-enabled-known` / `root.ui-state.fan-curve-enabled` in `status-text()`/`status-color()` and the enabled toggle row.
4. Keep `ThemeBridge { }` (std-widgets color-scheme sync), `FanCurveEditor { height: 430px; ... }` and all callbacks verbatim.

- [ ] **Step 2: Export from `ui/app-entry.slint`**

```slint
export { FansSection } from "audited/sections/fans.slint";
```

- [ ] **Step 3: Build**

Run: `cargo build -p orbis-ui --features ui-review --locked`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add ui/audited/sections/fans.slint ui/app-entry.slint
git commit -m "ui: fans section from fans window"
```

---

### Task 6: Hardware section

**Files:**
- Create: `ui/audited/sections/hardware.slint`

**Interfaces:**
- Consumes: content of `ui/audited/extra-window.slint` (Hotkeys M1–M5, Aura Effect, Panel Overdrive `RequestToggleRow`, Boot sound `ToggleRow`, Power/CPU rows) + `SectionCard`/`SectionTitle`/`RequestToggleRow`/`ToggleRow`/`ComboBox`/`SpinBox`.
- Produces: `HardwareSection` — same properties/callbacks as ExtraWindow except the keyboard-brightness block (stays on Dashboard):

```slint
in property <UiState> ui-state;             // only if extra-window referenced it; keep whatever extra-window had
in property <bool> backend-ready: false;
in property <bool> applying: false;
in property <bool> keyboard-state-ready: false;      // for Aura effect row gating
in property <bool> aura-state-ready: false;
in property <int> keyboard-effect: 0;                // in-out, as today
in property <int> keyboard-speed: 0;                 // in-out, as today
in property <bool> panel-overdrive-state-ready: false;
in property <bool> panel-overdrive-control-ready: false;
in property <bool> panel-overdrive: false;           // in-out, as today
in property <bool> boot-sound-state-ready: false;
in property <bool> boot-sound: false;                // in-out, as today
in property <[string]> binding-actions;              // as today
in property <int> m1-action..m5-action;              // in-out, as today
in property <int> igpu-memory: 0;                    // in-out
in property <int> hibernate-after: 0;                // in-out
in property <int> p-cores: 0;                        // in-out
in property <int> e-cores: 0;                        // in-out
callback reload-requested;
callback apply-requested;
callback panel-overdrive-requested(bool);
callback keyboard-effect-requested(int);
callback keyboard-speed-requested(int);
callback binding-changed(int, int);                  // (slot 1..5, action index) — replaces per-slot in-out if extra-window used callbacks; copy whatever extra-window declared
callback power-row-changed(int, int);                // same rule
```

IMPORTANT: do not invent properties. Open `ui/audited/extra-window.slint` and carry over its EXACT property/callback list; only these changes are allowed:
1. Window → Rectangle (drop window chrome + WindowHeader).
2. Delete the "Brightness" chip row block (keyboard brightness lives on Dashboard now); keep the Aura "Effect"/speed ComboBox rows gated by `keyboard-state-ready`/`aura-state-ready` exactly as they are.
3. Keep `ThemeBridge { }` and every truthful disabled expression verbatim.

- [ ] **Step 1: Create `ui/audited/sections/hardware.slint`** per the rules above.
- [ ] **Step 2: Export from `ui/app-entry.slint`**

```slint
export { HardwareSection } from "audited/sections/hardware.slint";
```

- [ ] **Step 3: Build**

Run: `cargo build -p orbis-ui --features ui-review --locked`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add ui/audited/sections/hardware.slint ui/app-entry.slint
git commit -m "ui: hardware section from extra window"
```

---

### Task 7: Settings section

**Files:**
- Create: `ui/audited/sections/settings.slint`

**Interfaces:**
- Consumes: `RequestToggleRow` (request-toggle-row.slint), `ActionButton`/`SectionTitle`/`ThemeBridge` (common.slint), `ComboBox` (std-widgets).
- Produces: `SettingsSection` — union of PreferencesWindow + Diagnostics actions + About:

```slint
in property <bool> theme-light: false;               // was theme-changed(bool) source value
callback theme-changed(bool);
in property <bool> startup: false;
callback startup-changed(bool);
in property <bool> start-minimized: false;
callback start-minimized-changed(bool);
in property <bool> remember-position: false;
callback remember-position-changed(bool);
in property <int> close-action: 0;                   // 0 = hide to tray, 1 = quit
callback close-action-changed(int);
in property <bool> diagnostics-ready: false;
in property <string> diagnostics-freshness: "";      // short status text from diagnostics runtime
callback diagnostics-refresh-requested;
callback diagnostics-copy-requested;
callback diagnostics-export-requested;
in property <string> version: "0.0.0";
in property <string> mock-profile: "production";
```

- [ ] **Step 1: Create `ui/audited/sections/settings.slint`**

Layout (single `VerticalLayout { spacing: 10px; }`):
1. `SectionTitle { title: "General"; }` + `RequestToggleRow` for theme (label "Light theme", checked ↔ `theme-light`, changed → `theme-changed(checked)`).
2. `SectionTitle { title: "Startup and window"; }` + `RequestToggleRow` rows for autostart ("Run on startup"), start-minimized, remember-position — copy the exact rows from `ui/audited/preferences-window.slint` (labels, disabled expressions, status texts).
3. Close action row: `ComboBox { width: 200px; model: ["Hide to tray", "Quit"]; current-index <=> root.close-action; }` + `selected => { root.close-action-changed(self.current-index); }` — copy the disabled/status expressions from preferences-window's close-action row.
4. `SectionTitle { title: "Diagnostics"; detail: root.diagnostics-freshness; }` + `HorizontalLayout { spacing: 6px; ActionButton { label: "Refresh"; ... } ActionButton { label: "Copy"; ... } ActionButton { label: "Export"; ... } }` — copy disabled expressions from `ui/audited/diagnostics-window.slint`; do NOT port "Open Logs" (stays disabled-by-absence as decided in #111).
5. `SectionTitle { title: "About"; }` + `Text { text: "Orbis Control v" + root.version + " · profile: " + root.mock-profile; font-size: 10px; color: Palette.text-secondary; }`.
6. `ThemeBridge { }` first child (color-scheme sync).

- [ ] **Step 2: Export from `ui/app-entry.slint`**

```slint
export { SettingsSection } from "audited/sections/settings.slint";
```

- [ ] **Step 3: Build**

Run: `cargo build -p orbis-ui --features ui-review --locked`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add ui/audited/sections/settings.slint ui/app-entry.slint
git commit -m "ui: settings section (preferences + diagnostics + about)"
```

---

### Task 8: Shell switch-over (atomic)

**Files:**
- Modify: `ui/audited/main-window.slint` (full rewrite to shell)
- Modify: `ui/app-entry.slint`
- Delete: `ui/audited/fans-window.slint`, `ui/audited/extra-window.slint`, `ui/audited/preferences-window.slint`, `ui/audited/diagnostics-window.slint`, `ui/audited/automation-window.slint`, `ui/audited/updates-window.slint`, `ui/components/mode-card.slint`, `ui/app-window.slint`

**Interfaces:**
- Consumes: all four section components (Tasks 4–7).
- Produces: final `AppWindow` surface = union of section surfaces + shell callbacks:

```slint
in-out property <Section> active-section: Section.Dashboard;
callback section-selected(Section);        // UI-local (Rust may also listen for screenshots)
callback titlebar-minimize-requested;
callback titlebar-close-requested;
callback titlebar-drag-started;
// plus every property/callback from DashboardSection, FansSection, HardwareSection,
// SettingsSection (re-exported 1:1 — Rust keeps the same on_* handler names).
```

- [ ] **Step 1: Rewrite `ui/audited/main-window.slint` as the shell**

```slint
import { UiState, Section } from "model.slint";
import { TitleBar } from "../components/title-bar.slint";
import { Sidebar } from "../components/sidebar.slint";
import { DashboardSection } from "sections/dashboard.slint";
import { FansSection } from "sections/fans.slint";
import { HardwareSection } from "sections/hardware.slint";
import { SettingsSection } from "sections/settings.slint";
import { Palette } from "../themes/dark.slint";

export component AppWindow inherits Window {
    width: 760px;
    height: 600px;
    no-frame: true;
    background: transparent;
    title: "Orbis Control";

    in-out property <UiState> ui-state;   // keep the existing default struct literal from the old file verbatim
    in-out property <Section> active-section: Section.Dashboard;
    callback section-selected(Section);
    callback titlebar-minimize-requested;
    callback titlebar-close-requested;
    callback titlebar-drag-started;

    // ---- re-exported section surfaces (copy the full property/callback lists
    // from the four section files; forward them into the section instances) ----

    root-rect := Rectangle {
        border-radius: 10px;
        clip: true;
        background: Palette.window-background;

        VerticalLayout {
            TitleBar {
                section-title: root.active-section == Section.Dashboard ? "Dashboard"
                    : root.active-section == Section.Fans ? "Fans"
                    : root.active-section == Section.Hardware ? "Hardware" : "Settings";
                minimize-requested => { root.titlebar-minimize-requested(); }
                close-requested => { root.titlebar-close-requested(); }
                drag-started => { root.titlebar-drag-started(); }
            }
            HorizontalLayout {
                Sidebar {
                    active <=> root.active-section;
                    section-selected(s) => { root.active-section = s; root.section-selected(s); }
                }
                Rectangle {
                    horizontal-stretch: 1;
                    clip: true;
                    background: Palette.window-background;
                    if (root.active-section == Section.Dashboard) : DashboardSection {
                        // bind ui-state + all display/keyboard/perf/gpu/charge
                        // properties and callbacks 1:1 (forwarding syntax:
                        // property <=> root.<same-name> / callback => root.<cb>)
                    }
                    if (root.active-section == Section.Fans) : FansSection {
                        // ui-state <=> root.ui-state; fan callbacks forwarded 1:1;
                        // mutation-safety-blocked/policy-status bound to the same
                        // AppWindow properties the old FansWindow used (keep their
                        // Rust-side names: mutation-safety-blocked, factory-reset-available, policy-status)
                    }
                    if (root.active-section == Section.Hardware) : HardwareSection {
                        // forward every extra-window property/callback 1:1
                    }
                    if (root.active-section == Section.Settings) : SettingsSection {
                        // forward theme/startup/minimized/position/close-action/
                        // diagnostics/version/mock-profile 1:1
                    }
            }
        }
    }
}
```

Forwarding discipline: every property/callback that Rust wires today on AppWindow (grep `app.on_` and `set_` in `crates/orbis-ui/src/main.rs` before writing this file) must exist on the shell with the SAME name and be forwarded to the owning section. Nothing is renamed in this task — Task 9 only deletes handlers for removed surfaces.

- [ ] **Step 2: Update `ui/app-entry.slint`**

```slint
export { UiState, ChargeLimitState, GpuHwState, PerformanceHwState, GpuModeHwState, FanCurveHwState, Section } from "audited/model.slint";
export { ThemeState, ThemeMode } from "themes/dark.slint";
export { AppWindow } from "audited/main-window.slint";
export { PreviewDialogWindow } from "audited/preview-dialog-window.slint";
```

- [ ] **Step 3: Delete removed surfaces**

```bash
git rm ui/audited/fans-window.slint ui/audited/extra-window.slint \
  ui/audited/preferences-window.slint ui/audited/diagnostics-window.slint \
  ui/audited/automation-window.slint ui/audited/updates-window.slint \
  ui/components/mode-card.slint ui/app-window.slint
```

Pre-check first: `rg -ln "app-window.slint|mode-card|fans-window|extra-window|preferences-window|diagnostics-window|automation-window|updates-window" ui/ crates scripts` — every reference outside `ui/app-entry.slint` must be updated or deleted in this task (expected references: `scripts/check-ui-contract.py` and `crates/orbis-ui/src/main.rs`/`main_tests.rs`, both handled here/Task 9/10).

- [ ] **Step 4: Build (Rust will fail on deleted generated types — that is Task 9's entry point; slint-only check)**

Run: `cargo build -p orbis-ui --features ui-review --locked 2>&1 | rg "error\[" | head`
Expected: errors ONLY about missing `FansWindow`/`ExtraWindow`/`PreferencesWindow`/`DiagnosticsWindow`/`AutomationWindow`/`UpdatesWindow` types and removed callbacks in `main.rs`. Slint compilation itself (the `slint-build` step) must succeed. If slint errors — fix the shell before proceeding.

- [ ] **Step 5: Commit (together with Task 9 if you prefer a green tree per commit)**

This task plus Task 9 form one compile unit. Either commit both together (`git add -A` is forbidden — list explicit paths) or commit this task knowing the tree does not compile and Task 9 lands immediately after. Recommended: implement Task 9 before committing, then create both commits in order.

---

### Task 9: Rust wiring rework

**Files:**
- Modify: `crates/orbis-ui/src/main.rs`

**Interfaces:**
- Consumes: generated `AppWindow` with `Section`, `active-section`, `titlebar-*` callbacks; section properties forwarded 1:1.
- Produces:
  - `fn handle_quit(app: &AppWindow)` — extracted body of today's `on_quit_clicked` minus the deleted-window hides (keeps preview dialog hide + actual quit logic).
  - `fn wire_titlebar(app: &AppWindow)`: `on_titlebar_minimize_requested` → `app.window().set_minimized(true)`; `on_titlebar_close_requested` → `handle_quit(app)`; `on_titlebar_drag_started` → `app.window().with_winit_window(|w| { let _ = w.drag_window(); })`.
  - `--ui-section <dashboard|fans|hardware|settings>` CLI flag (ui-review builds only) → sets `active-section` before `--screenshot` rendering; `render_screenshot` canvas becomes `PhysicalSize::new(760, 600)` and `set_size(LogicalSize::new(760.0, 600.0))`.

- [ ] **Step 1: Delete secondary-window machinery**

Remove from `main.rs` (all references — use `rg -n "FANS_WINDOW|EXTRA_WINDOW|AUTOMATION_WINDOW|PREFERENCES_WINDOW|DIAGNOSTICS_WINDOW|UPDATES_WINDOW"` to enumerate): the six statics, `sync_fans_window`, `wire_fans_window`, `show_fans_window`, `show_extra_window`, `show_automation_window`, `sync_preferences_window` + its helpers, `wire_preferences_window`, `show_preferences_window`, `wire_diagnostics_window`, `show_diagnostics_window`, `wire_updates_window`, `show_updates_window`, `apply_theme_to_all`'s per-window loops (theme now applies to the single AppWindow; keep the ThemeState property write + ThemeBridge behavior), and the `on_extra_clicked`/`on_automation_clicked`/`on_updates_clicked`/`on_diagnostics_clicked`/`on_preferences_clicked`/`on_fans_clicked` handlers that opened windows.

Re-point handlers that must survive:
- `on_fans_clicked` → `app.set_active_section(Section::Fans);` (keep the handler so any residual caller is harmless).
- Preferences/Diagnostics/Updates/Extra handlers: delete entirely (no callers remain after Task 8).

Diagnostics actions: the old DiagnosticsWindow owned refresh/copy/export wiring. Re-wire the SAME runtime calls to the forwarded AppWindow callbacks `on_diagnostics_refresh_requested`, `on_diagnostics_copy_requested`, `on_diagnostics_export_requested` — copy the handler bodies from the old `wire_diagnostics_window` (they call into `diagnostics_runtime`/controller; do not change runtime semantics). Preferences persistence: re-wire `on_theme_changed`, `on_startup_changed`, `on_start_minimized_changed`, `on_remember_position_changed`, `on_close_action_changed` bodies from the old `wire_preferences_window` verbatim (they call controller/config persistence).

- [ ] **Step 2: Extract quit + wire titlebar**

```rust
fn handle_quit(app: &AppWindow) {
    // body = old on_quit_clicked minus FANS_WINDOW..UPDATES_WINDOW hides;
    // keep PREVIEW_DIALOG_WINDOW hide and the existing quit/tray decision path.
}

// inside the wiring function after build_app:
{
    let app_weak = app.as_weak();
    app.on_titlebar_minimize_requested(move || {
        if let Some(app) = app_weak.upgrade() {
            app.window().set_minimized(true);
        }
    });
}
{
    let app_weak = app.as_weak();
    app.on_titlebar_close_requested(move || {
        if let Some(app) = app_weak.upgrade() {
            handle_quit(&app);
        }
    });
}
{
    let app_weak = app.as_weak();
    app.on_titlebar_drag_started(move || {
        if let Some(app) = app_weak.upgrade() {
            let _ = app.window().with_winit_window(|winit_window| {
                let _ = winit_window.drag_window();
            });
        }
    });
}
```

- [ ] **Step 3: `--ui-section` flag + screenshot canvas**

In `parse_args` add `ui_section: Option<String>` (`--ui-section`), values `dashboard|fans|hardware|settings`. In the screenshot path (and only there), after `build_app`:

```rust
if let Some(section) = args.ui_section.as_deref() {
    let section = match section {
        "fans" => Section::Fans,
        "hardware" => Section::Hardware,
        "settings" => Section::Settings,
        _ => Section::Dashboard,
    };
    app.set_active_section(section);
}
```

Update `render_screenshot`: canvas 760×600 (`PhysicalSize::new(760, 600)`, `set_size(LogicalSize::new(760.0, 600.0))`). Remove/replace `window_height()` (old 441/466 budget helper) — delete it if only tests used it, or update its callers.

- [ ] **Step 4: Build + clippy + tests**

Run: `cargo check -p orbis-ui --all-targets --locked && cargo clippy -p orbis-ui --all-targets --locked -- -D warnings && cargo test -p orbis-ui --locked`
Expected: PASS (main_tests may still reference deleted files — those exact failures are fixed in Task 10; if a test fails on a removed file marker, apply the Task 10 edit for it now).

- [ ] **Step 5: Screenshots all sections**

```bash
cargo build -p orbis-ui --features ui-review --locked
for s in dashboard fans hardware settings; do
  ./target/debug/orbis-control --ui-state default --ui-section $s --screenshot /tmp/opencode/redesign-$s.png
done
```

Expected: four PNGs; visually verify shell chrome (title bar buttons, sidebar, rounded corners), section content, truthful disabled states.

- [ ] **Step 6: Commit (Tasks 8+9 together)**

```bash
git add ui/audited/main-window.slint ui/app-entry.slint crates/orbis-ui/src/main.rs
git add ui/audited/fans-window.slint ui/audited/extra-window.slint ui/audited/preferences-window.slint ui/audited/diagnostics-window.slint ui/audited/automation-window.slint ui/audited/updates-window.slint ui/components/mode-card.slint ui/app-window.slint
git commit -m "ui: single-window shell with sidebar sections"
```

---

### Task 10: Source contracts update

**Files:**
- Modify: `scripts/check-ui-contract.py`
- Modify: `crates/orbis-ui/src/main_tests.rs`

- [ ] **Step 1: `check-ui-contract.py`**

1. `check_main_geometry`: bounds → `if w > 800 or h > 640: fail(...)` / `if w < 400 or h < 400: fail(...)`.
2. `SECONDARY_MIN_HEIGHT`: remove the six deleted window entries; keep only `"ui/audited/preview-dialog-window.slint": 210`.
3. `PRODUCTION_SURFACES`: remove entries for the six deleted files; update `"ui/audited/main-window.slint"` markers to:
   `["display-mode-requested", "keyboard-brightness-requested", "startup-changed", "start-minimized-changed", "remember-position-changed", "close-action-changed", "diagnostics-copy-requested", "titlebar-close-requested", "titlebar-drag-started"]`.
   Add a new entry:
   `"ui/audited/sections/settings.slint": ["ThemeBridge {", "diagnostics-refresh-requested", "diagnostics-export-requested"]`.
4. Run `python3 scripts/check-ui-contract.py` — fix any marker that the final markup spells differently (markers are substring checks; adjust the marker, never the markup, unless the markup lost a required behavior).

- [ ] **Step 2: `main_tests.rs`**

- `main_window_renders_queued_target_and_reboot_state`: `include_str!` path stays `ui/audited/main-window.slint` but the queued/pending markup moved to `ui/audited/sections/dashboard.slint` — point the test at the dashboard file and keep all assertions verbatim (`gpu-queued == 0/1/2`, `shutdown/reboot`, Optimized line without `pending:`; the Optimized lookup becomes `label: \"Optimized\"` in the dashboard file — already true).
- Any test doing `include_str!` on deleted files (grep `include_str!` for `fans-window|extra-window|preferences-window|diagnostics-window|automation-window|updates-window`): move each surviving assertion to the corresponding `sections/*.slint` path; delete assertions that only described deleted chrome (WindowHeader text etc.). Do not delete behavioral assertions without an equivalent in the new markup — if one exists, the markup lost it: fix the markup instead.
- Add one shell contract test:

```rust
#[test]
fn shell_hosts_four_sections_and_frameless_chrome() {
    let shell = include_str!("../../../ui/audited/main-window.slint");
    assert!(shell.contains("no-frame: true"));
    assert!(shell.contains("Section.Dashboard"));
    assert!(shell.contains("Section.Fans"));
    assert!(shell.contains("Section.Hardware"));
    assert!(shell.contains("Section.Settings"));
    assert!(shell.contains("titlebar-close-requested"));
    assert!(!shell.contains("UpdatesWindow"));
    assert!(!shell.contains("AutomationWindow"));
}
```

- [ ] **Step 3: Full crate verification**

Run: `python3 scripts/verify-static && cargo test -p orbis-ui --locked && cargo clippy -p orbis-ui --all-targets --locked -- -D warnings && git diff --check`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add scripts/check-ui-contract.py crates/orbis-ui/src/main_tests.rs
git commit -m "ui: contracts for single-window redesign"
```

---

### Task 11: Docs + final verification

**Files:**
- Modify: `docs/ui-reference.md`, `docs/current-state.md`, `docs/backend-completion-status.md`

- [ ] **Step 1: `docs/ui-reference.md`** — replace normative section 0 with the new design: window 760×600 frameless (radius 10), TitleBar 40px with drag/minimize/close, Sidebar 168px (Dashboard/Fans/Hardware/Settings with SVG icons), section list with content summary, Catppuccin Mocha/Latte token tables (from Task 1), DropdownRow 30px, SegmentedTrack 48px, structural decisions (Updates/Automation removed from UI, Diagnostics as Settings actions). Mark the old "Orbis Control Center (2026-08-24)" subsection superseded by this one (keep as history).

- [ ] **Step 2: `docs/current-state.md`** — update the UI row(s): single-window shell, four sections, Updates/Automation UI removed (Rust automation layer untouched), Catppuccin theme, frameless title bar; note `unstable-winit-030` + `svg` slint features in the dependency notes.

- [ ] **Step 3: `docs/backend-completion-status.md`** — mark Updates and Automation UI surfaces as removed-from-UI (providers/typed layers retained); Diagnostics now reachable from Settings.

- [ ] **Step 4: Final verification (INTEGRATION tier)**

```bash
python3 scripts/verify-static
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Plus light-theme screenshot sanity: temporarily set `ThemeState.mode` default to Light in a scratch build (do NOT commit), render one screenshot, revert.

- [ ] **Step 5: Commit + push + issue comment**

```bash
git add docs/ui-reference.md docs/current-state.md docs/backend-completion-status.md
git commit -m "docs: single-window ui redesign reference and status"
git push origin development
```

Comment on #128: summary of the redesign, screenshots paths, executed checks, and the note that PR #129 merge gates remain unchanged.
