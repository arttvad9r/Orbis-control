# Orbis visual refresh — execution design

## Objective

Deliver a TARGET_DESIGN visual refresh of the existing Orbis Control desktop UI from the ten preserved reference screenshots, while preserving the real page inventory and actual backend semantics.

## Source of truth

- User brief and locked decisions: `docs/visual-refresh-2026-09-24.md`.
- Product decision register: `docs/product-decisions.md`.
- Reference screenshots: `docs/references/orbis-visual-2026-09-24/`.
- Current UI source: `ui/`, `crates/orbis-ui/`.
- Existing product acceptance: `acceptance-contract.yaml` (`REQ-VISUAL`, `AC-VISUAL`, `SC-VISUAL-980`, `SC-VISUAL-STATES`) and `interaction-contract.md`.

## Implementation strategy

Refresh reusable visual primitives first, then adapt the existing destinations in two coherent sequential UI slices. Use the supplied screenshots as design evidence, not shipping assets. Preserve Orbis branding and existing navigation; do not add `Profiles` or `Devices` pages.

Real controls keep their existing backend/read-back, pending, error, unsupported and read-only behavior. The explicitly approved future-only visual exception provides normal local press, selection or toggle feedback, resets on navigation or restart, and makes no backend call, persistence write, hardware change or success claim. No mock disclaimer is shown.

## Execution topology and rationale

Two sequential UI workers are the minimum reliable topology: the first owns shared visual primitives plus the highest-density Dashboard/Performance/Cooling surfaces; the second owns the remaining reference-aligned sections after that visual foundation is committed. This avoids concurrent writes to shared Slint theme/window files. Each worker self-verifies; then independent QA evaluates the assembled frozen candidate. A separate integration card is unnecessary because the second UI worker is the explicit integration/candidate owner and runs the liveness smoke.

## Requirement traceability

- `REQ-VISUAL` / `AC-VISUAL` / `SC-VISUAL-980`, `SC-VISUAL-STATES` → W1, W2 and independent QA.
- Existing real-control semantics in `REQ-HONESTY`, `REQ-REGRESSION` → W1/W2 must preserve, QA spot-checks affected surfaces.
- Visual brief and reference manifest → W1/W2 visual-runtime evidence and QA reference-to-actual review.

## Interaction contract

| Control / gesture | User action | Observable result | Implementation owner | Runtime verification owner | QA scenario |
| --- | --- | --- | --- | --- | --- |
| Existing navigation | Select an existing section | Active navigation and matching existing page render without a new destination | W1/W2 UI developers | W1/W2 | SC-VISUAL-980 |
| Existing real control | Use a backend-wired control | Existing authoritative/pending/error/read-only result remains intact | W1/W2 UI developers | W1/W2 | SC-VISUAL-STATES, regression spot-check |
| Future-only visual control | Hover, press, choose, toggle or drag | Normal local visual feedback; any state resets on page leave or app restart | W1/W2 UI developers | W1/W2 | visual interaction spot-check |

## Repository / environment baseline

- Repository: `/home/artt/Orbis-control-base-20260923`.
- Baseline code SHA: `63c014e95ec086de5f4a8d99b2a62e6d1ff30879`.
- Local commits are permitted for candidate custody; push, tag, merge and remote mutation are not authorized.
- Existing unrelated dirty files are out of scope and must not be included in visual commits.

## Shared contracts and invariants

- Russian UI, TARGET_DESIGN reference authority, no pixel-equality requirement.
- No new top-level `Profiles` or `Devices` sections.
- Do not ship supplied screenshots, ASUS/ROG logos or imagery as product assets.
- Preserve actual backend behavior for real capabilities; do not fabricate telemetry, read-back or hardware success.
- User-approved visual exception is limited to future-only controls: no explanatory status, no side effect and no success copy.
- Do not change privileged write contracts, Hardware1 ABI, session-client composition or device-mutation behavior.

## Workstream ownership

### W1 — UI developer: visual foundation and primary monitoring/control pages

W1 UI developer owns a task worktree, reusable theme/window/navigation/component styling, and the Dashboard, Performance and Cooling surfaces. It may make only directly necessary Rust/Slint wiring for local visual feedback. It must not alter real backend contracts or implement remaining page surfaces as a shortcut.

### W2 — UI developer and integration owner: remaining existing pages and assembled liveness

W2 UI developer owns Power, Graphics, Backlight, Display, System and Settings surfaces plus necessary non-overlapping page styling after W1's committed candidate. W2 is the explicit integration/candidate owner: it performs the assembled User-Liveness Smoke and freezes the candidate. It must not rewrite W1 visual primitives except for an evidenced compatibility correction.

### W3 — QA owner: independent visual QA

W3 QA owner independently compares representative reference-to-actual states, measures geometry and reports PASS/FAIL for the exact W2 candidate. It owns evidence only and does not repair implementation.

## Workstreams and ordering

### W1 — UI developer: visual foundation and primary pages

Reusable visual system, navigation, Dashboard, Performance and Cooling.

### W2 — UI developer/integration: remaining pages and assembled liveness

Power, Graphics, Backlight, Display, System and Settings; responsive assembly, User-Liveness Smoke and candidate freeze.

### W3 — QA: independent visual QA

Independent reference-to-actual and geometry review of the exact W2 candidate.

## Dependency graph

```text
W1 visual-foundation candidate
  -> W2 remaining-pages + liveness candidate
  -> W3 independent visual QA
  -> Planner terminal release acceptance
```

## Acceptance strategy

- Each UI worker runs applicable Slint/Rust formatting and targeted verification plus `git diff --check`.
- Each UI worker captures representative dark-mode screenshots and produces measured geometry evidence for 980×680 and 1200×800; screenshot-only evidence is insufficient.
- W2 performs the User-Liveness Smoke: launch → Dashboard → Performance/Cooling → a secondary existing page → return, with visible navigation/active-state changes and no clipping.
- QA independently reviews composition, hierarchy, major positions, spacing, typography character, navigation/control treatment and material deviations against the preserved references; it also confirms the absence of `Profiles`/`Devices` navigation.
- QA verifies that real capability states remain honest and that the explicit future-only visual exception has no backend side effect or hardware-success copy.
- QA includes exploratory interaction testing of navigation, at least one real-capability state and each future-only visual-control category; it reports defects and does not repair them.
- The acceptance matrix projects `acceptance_id -> check_id -> method -> expected_observable -> evidence_kind` as follows: `AC-VISUAL -> visual-geometry -> visual_runtime -> clean 980×680 and 1200×800 geometry with reference-to-actual captures -> measured geometry JSON plus captures`; `AC-REGRESSION -> ui-regression -> unit_test -> affected existing UI paths remain functional -> targeted command output`; `AC-VISUAL -> visual-qa-matrix -> visual_runtime -> independent reference comparison and interaction inventory PASS -> QA geometry report and captures`.
- After QA PASS, Planner performs terminal release acceptance on the exact candidate, including identity custody and an independent visual/navigation spot-check.

## User-Liveness Smoke

On the W2 candidate, perform the primary interaction: launch the application, open Dashboard, navigate to Performance and Cooling, and operate a representative future-only visual control where present. Then exercise the secondary navigation path by opening one secondary existing page and returning to Dashboard. Observable postcondition: active navigation and local visual feedback appear, while no clipping, backend mutation or hardware-success feedback appears.

## Asset and reference readiness

All ten source PNGs are preserved with SHA-256 in `docs/references/orbis-visual-2026-09-24/` and inventoried by `docs/visual-refresh-2026-09-24.md`. They are not production assets. The implementation uses the existing icon/component system and may not substitute ASUS/ROG logos or reference artwork.

## Visual / product acceptance

At both 980×680 and 1200×800, dark and light modes remain legible without clipping, overlap, horizontal main-content scrolling or phantom whitespace. Dark mode must compare REFERENCE → ACTUAL against the supplied target compositions for the matching existing pages. Composition, hierarchy, major positions, proportions/spacing, typography character, navigation/components and material language are reviewed; every material deviation is reported. Empty, partial and full visual-state content remain readable. Any adaptation caused by absent Orbis functionality is reported rather than silently replaced with a new page or shipped reference asset.

## Execution recovery policy

A worker timeout is inspected against its worktree and evidence. A coherent near-complete UI task resumes once in the same task/worktree. Split only when a proven surface/ownership boundary is too broad. Environment or capture-tool failure cannot turn missing runtime/visual verification into PASS. No wall-clock limit reduces the quality gate.

## Benchmark budget

No benchmark or wall-clock ceiling was explicitly user requested. Timeout or step exhaustion never weakens, waives or converts missing verification into PASS.

## Kanban mapping

- The authoritative task graph is materialized on the existing `orbis-v01` board, which was created with `hermes kanban boards create --from-contract`; no duplicate board is created.
- W1: materialize now as one `ui` task for a developer, with Planner wake/subscription and a task-owned worktree.
- W2: materialize after W1 freezes an exact candidate.
- W3: materialize after W2 freezes an exact candidate for QA.
- No dependency is added to the blocked W2 hardware-validation chain; this is an independent user-priority visual line.
