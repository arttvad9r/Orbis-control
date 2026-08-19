# Beta Acceptance Checklist

This checklist records what is currently proven for the hardening/preferences slices referenced by the beta-readiness backlog. It uses the release evidence taxonomy and deliberately does not treat targeted tests as packaged or live acceptance.

## Current evidence

| Slice | Repository state | Proven evidence | Beta acceptance still required |
|---|---|---|---|
| PR #12 — NixOS PolicyKit authority default | Merged | `IMPLEMENTED / TESTED`: targeted Nix module evaluation proved Orbis default enablement, host override, disabled-module behavior, and retained policy installation. | Packaged NixOS/module acceptance if this behavior is part of the beta install contract. No live authorization-agent claim is made by this PR. |
| PR #14 — hardwared keyboard LED sandbox allowlist | Merged | `IMPLEMENTED / TESTED`: exact writable-path assertions cover only `platform_profile` and keyboard brightness, with negative checks for `max_brightness` and broad LED paths. | Packaged service sandbox inspection on the beta package; live keyboard mutation remains a separate claim. |
| PR #16 — keyboard backlight capability probe | Merged | `IMPLEMENTED / TESTED`: zero-write structural probe and error classification are covered by targeted tests/clippy. | Live hardware evidence before claiming writable keyboard control. `Supported` remains structural evidence, not proof that a write succeeds. |
| PR #18 — profile-specific fan curve read via Session1 | Merged | `IMPLEMENTED / TESTED`: protocol/sessiond/session-client/UI read path and profile/fan separation have targeted regression coverage. | Packaged end-to-end read acceptance and live read evidence for the supported hardware/profile set. Fan mutation is outside this claim. |
| PR #19 — safe preferences storage | Draft, not merged | `IMPLEMENTED / TESTED` on its branch: versioned XDG preferences storage, strict/non-destructive loading, and atomic replacement tests. | Merge/integration with current hardening, then packaged startup/persistence acceptance. Durability follow-up PR #51 must be resolved with the preferences stack. |
| PR #21 — Dark/Light theme persistence | Draft, stacked on #19 | `IMPLEMENTED / TESTED` on its branch: startup ordering, persistence, multi-window inheritance, and failure handling are covered by targeted UI/config tests. | Parent stack integration plus packaged first-render/restart acceptance. No acceptance claim while the parent preferences stack is unmerged. |
| PR #23 — UPower default in NixOS module | Draft, not merged | `IMPLEMENTED / TESTED` on its branch: targeted Nix evaluation proves default enablement, host override, disabled-module behavior, and absence of hard daemon ordering. | Merge/integration and packaged NixOS module evaluation/service acceptance. This PR alone does not prove UPower runtime availability on a beta installation. |

## Beta gates

- [ ] All required Draft stacks are integrated without silently dropping their validated semantics.
- [ ] Target beta package/module builds successfully from the release candidate revision.
- [ ] Packaged startup validates PolicyKit/UPower/service integration without relying on developer-shell state.
- [ ] Preferences survive a packaged restart and invalid-source behavior remains non-destructive.
- [ ] Theme is applied before first visible render in the packaged application.
- [ ] Keyboard capability remains honest on live hardware: readable structural support is not reported as successful write evidence.
- [ ] Profile-specific fan reads are checked through the packaged Session1 path on supported live hardware before a `LIVE-VALIDATED` claim.
- [ ] Hardware-facing evidence records identify the exact model/environment and distinguish read from write validation.
- [ ] Any failed or unexecuted packaged/live gate remains `UNKNOWN` or `BLOCKED`; it is not promoted from unit/CI evidence.

## Explicit non-claims

This refresh does not claim that the current Draft preference/theme/UPower branches are merged, packaged, or live-validated. It does not claim keyboard mutation acceptance, fan mutation acceptance, or any hardware support beyond evidence already recorded elsewhere. No checklist item is completed merely because architecture or code exists.
