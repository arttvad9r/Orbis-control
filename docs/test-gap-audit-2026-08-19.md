# Orbis Control test gap audit — 2026-08-19

Scope: backlog item #79. Audit pure/unit/P2P coverage across config/core/application/UI and implement only safe hermetic regressions in this slice.

## Result

The repository has substantial targeted coverage, especially around provider read-back, capability separation, config parsing/storage and UI state reducers. Remaining gaps are concentrated at cross-layer boundaries and newer Draft stacks. This slice implements only a small core regression that is fully deterministic and hardware-independent.

## Implemented in this slice

`crates/orbis-core/src/provider.rs` now tests all `ProviderStatus` variants rather than only Healthy/Unavailable:

- `Healthy -> "healthy"`;
- `Degraded -> "degraded"`;
- `Unavailable -> "unavailable"`;
- serde round-trip for all variants, including exact snake_case labels.

This protects the diagnostic/state vocabulary used by higher layers and closes the prior omission of `Degraded` from direct regression coverage.

## Remaining config gaps

1. **Privacy redaction regression — high priority.** The privacy audit found that parser/schema warning payloads can be debug-formatted into logs. After the redacted-category implementation exists, add a malformed TOML fixture containing a synthetic secret marker and assert the production loggable representation never contains that marker.
2. **Stack integration for persistence features.** Preferences durability, window state, desired state, XDG autostart and startup preference wiring each have targeted tests in Draft stacks, but they are not yet one integrated acceptance suite. Do not duplicate them on hardening; validate after stack consolidation.
3. **Failure injection after rename/dir-sync.** Current durability tests cover the designed atomic path, but fully deterministic injection for a post-rename parent-directory sync failure would require an explicit filesystem abstraction. Do not add one only for a test unless failure-reporting semantics are first decided.

## Remaining core gaps

1. **ProviderStatus complete vocabulary** — closed by this slice.
2. **Desired/Observed/Pending integration semantics.** Draft domain tests cover independent state fields, but reconciliation behavior does not exist yet. Add tests only with the future planner/executor; do not invent expected transitions now.
3. **Extended ASUS concepts.** AniMe/Slash/power-limit readiness lacks authoritative production semantics, so test fixtures must not encode guessed model mappings/ranges merely to increase coverage.

## Remaining application gaps

1. **Lifecycle-to-reconciliation behavior** is not implemented yet; Startup/Resume/BackendRecovered/CapabilityChanged are currently inert domain events. Future application tests must prove event handling is idempotent and does not write when desired/observed state already agrees.
2. **Accepted semantics across future reconciliation.** Current core and UI audits prove `Accepted != Applied`, but future reconciliation tests must ensure `Accepted` does not clear pending/desired state or overwrite authoritative observed hardware state.
3. **Diagnostics integrated collection.** Collector/DTO components have focused Draft tests, but production composition/window wiring is still a separate active stack. Add end-to-end snapshot-to-window tests only after that stack settles to avoid duplicating temporary interfaces.

## Remaining UI gaps

1. **Preferences warning redaction** mirrors the config privacy gap and should be tested at the formatting/logging boundary, not through a real journal.
2. **Run-on-startup and Start Minimized** already have focused tests in their Draft stacks; they need one later integrated Preferences/startup regression after stack consolidation.
3. **Updates window** is intentionally local preview-only. A future network updater must start with tests that prove no install/download action is triggered by availability checks and that failures cannot present optimistic installed/current state.
4. **Diagnostics export/clipboard** is not implemented. Future safe text/JSON/copy tests must assert a strict allowlist and absence of serials/full env/raw journal/arbitrary file contents.

## Test strategy rules

- Prefer pure reducers/adapters and fake providers over live services.
- Do not perform hardware writes in tests or capability discovery.
- Keep `Unknown`, `Unavailable`, `Unsupported`, `PermissionDenied`, `ReadOnly`, `Accepted` and `Applied` distinct in assertions.
- Do not use mocks as proof of packaged/live hardware support.
- Add a regression when a concrete bug/contract exists; do not encode speculative product semantics to increase line coverage.

## Validation note

The core tests added here are hermetic by construction. Repository-wide Cargo validation is currently affected by the independently documented Rust 1.85/UI dependency graph blocker from item #77. A disposable validation environment may normalize only that manifest syntax to execute this core test; such normalization is not part of this implementation branch.

No live hardware, D-Bus, journal, Nix or service operation is required by any test in this slice.
