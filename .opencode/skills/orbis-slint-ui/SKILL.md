---
name: orbis-slint-ui
description: Current Slint UI paths and implementation rules for Orbis Control. Use for substantial UI work.
---

# Orbis Slint UI

## Current source layout

- Slint entrypoint: `ui/app-entry.slint`.
- Main shell and sections: `ui/audited/main-window.slint` and `ui/audited/sections/`.
- Shared components: `ui/components/`.
- Themes: `ui/themes/dark.slint` and `ui/themes/light.slint`.
- Rust UI/runtime: `crates/orbis-ui/src/`.
- Active worker runtime: `crates/orbis-ui/src/worker_runtime.rs`.

Do not resurrect removed legacy window/component files just because an old document mentions them.

## Implementation rules

- UI is a presentation layer; privileged hardware/system I/O stays in application/provider/service boundaries.
- Wire a control all the way to production runtime behavior or leave it honestly disabled/absent. Do not create fake-success UI.
- Keep state ownership in Rust/domain/runtime where it belongs; use Slint for presentation and interaction.
- Reuse existing components/tokens when they fit, but refactor or replace them when they obstruct a coherent design.
- Avoid preserving obsolete layouts only because screenshots or old measurements exist.

## Verification

After a meaningful UI batch, run an affected-crate check/test. Before completing a substantial UI slice, run at least `scripts/verify crate orbis-ui` or broader task verification.

When visual output materially changed and a screenshot/offscreen path is available, inspect the rendered result. A text-marker script is not a substitute for compiling or rendering the UI.