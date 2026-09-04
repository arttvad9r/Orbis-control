# Orbis Control — product completion queue

This is the concise backlog for finishing the application. It is intentionally product-oriented. Do not split these items into document-only tasks; complete each as a working vertical slice.

`main` is the canonical branch. Source code and executable behavior override stale historical notes.

## P0 — keep `main` runnable

- [ ] On every substantial development session, first establish the current build/runtime state and fix blocking compile/startup failures before adding more architecture.
- [ ] `scripts/verify task` passes on the working revision.
- [ ] The normal GUI starts as an unprivileged user from the documented development environment without fixture-only setup.
- [ ] Remove or fix dead production modules, placeholder wiring and stale feature gates encountered on the path to a runnable application.

## P1 — finish the daily-control experience

Audit every visible section as a real user flow, not as individual backend functions. A visible control is complete only when UI → runtime/application → provider/service → authoritative result/error is connected.

- [ ] Dashboard presents real current state without mock fallback.
- [ ] Performance modes read and apply end to end.
- [ ] Battery charge limit reads and applies end to end.
- [ ] Cooling/fan reads and supported fan actions work end to end; unsupported hardware remains honestly unavailable.
- [ ] Graphics exposes the supported ASUS product GPU flow coherently, including pending/reboot semantics where required.
- [ ] Keyboard/backlight and supported ASUS extras are either fully wired or removed from the interactive UI until they are real.
- [ ] Every user action produces confirmed state, pending state, or a useful error; no optimistic fake success.
- [ ] Preferences, autostart, tray/close behavior and diagnostics work in the packaged application, not only tests.

## P2 — resolve unfinished feature shells

For each item below choose one of two valid outcomes: **finish it as a real product feature** or **remove the unused UI/runtime/design shell from the product**. Contract-only placeholders are not a finished state.

- [ ] Automation: either deliver a minimal safe, useful automation flow with real execution and UI, or remove the dormant shadow/executor scaffolding until there is a concrete product requirement.
- [ ] Display refresh: implement a real compositor/backend owner for supported environments, or remove the inactive mutation surface and keep display information read-only.
- [ ] Updates: implement a real signed release source and installation path, or remove self-update code/UI and rely on the package manager.
- [ ] Research/foundation modules with no production consumers: wire them into an actual user workflow or delete them. Do not keep unused abstractions merely because they already exist.

## P3 — simplify the codebase around the finished product

- [ ] Remove obsolete compatibility paths, duplicate state models and dead files discovered while completing P1/P2.
- [ ] Break up oversized modules only where doing so improves ownership or makes a live feature easier to maintain; do not refactor for line-count aesthetics alone.
- [ ] Replace brittle source-marker tests with behavioral Rust/Slint/integration tests when the behavior is important.
- [ ] Keep test-only fixtures out of the normal release dependency/runtime path.

## P4 — package and release

- [ ] `scripts/verify full` passes on the release candidate.
- [ ] Nix package/module installs the GUI, user/session pieces, `orbis-hardwared`, D-Bus policy and polkit policy correctly.
- [ ] Perform a packaged smoke test: launch, tray/lifecycle, diagnostics, read-only state and safe supported controls.
- [ ] Check desktop/AppStream metadata and application identity.
- [ ] Confirm no known unsafe write is enabled by default.
- [ ] Record live hardware validation only for features actually exercised on the named device; lack of another device does not block shipping software with honest capability detection.
- [ ] Bump the version and create the first release only after the application is usable as a daily-control tool.

## Working rule for agents

When asked to "continue", "finish the project", or equivalent, select the highest-priority unchecked item that can be advanced in the current environment and keep working through its directly related subtasks until the vertical slice works. If an item is already complete in source/runtime, mark it complete and immediately continue to the next one rather than ending the session with a status report.