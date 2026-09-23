# Orbis v0.1 release-readiness execution design

## Objective

Close the verified v0.1 release-readiness gap from a clean current-plan baseline, without using the historical `agent/finish-v01` worktree as an execution base. The delivery is a truthful installed Arch/Linux laptop-control application: no enabled placeholder action, current evidence for every supported mutation, and one frozen candidate suitable for later user-authorized release actions.

D-012 is not weakened: complete G-Helper parity remains the approved P1–P6 program after v0.1. Those waves are not silently made v0.1 release blockers.

## Source of truth

- Raw user direction: research the existing application before building a task graph; use a fresh base rather than the old branch.
- Locked scope: `DECISIONS.md` D-012 and `docs/parity/g-helper-parity.md`.
- v0.1 product behavior: `SPEC.md`, `ACCEPTANCE.md`, `acceptance-contract.yaml`, `interaction-contract.md`.
- Current constraints: `TODO.md`, `docs/architecture.md`, `docs/capability-state-contract.md`.
- Backend reference: `/home/artt/g-helper-linux` under D-011; research only, with no code/assets copied.
- Current research: source/test inspection on 2026-09-23 and `docs/research/comparative-projects.md`.

## Implementation strategy

The earlier S5 GPU/MUX queue was not a valid representation of remaining work. Current source and test evidence show typed boot sound, iGPU memory, PCIe ASPM, clamshell, fan reset, GPU mode, power-limit and diagnostics flows already exist. Some other System controls are deliberately disabled/read-only because a safe owner has not been proven; that is correct only while the UI makes the capability state honest.

v0.1 therefore needs a source-driven enabled-action audit and a bounded repair only when the audit finds an enabled action without a genuine typed/session owner and truthful read-back, pending, partial or error outcome. The audit covers Dashboard, Performance, Power, Cooling, Graphics, Backlight, Display, System, tray/preferences and diagnostics. It preserves Hardware1 for privileged writes, session ownership for session behavior, desired/observed/pending separation and fail-closed states.

The local G-Helper Linux reference establishes later Linux routes for M-key firmware binding, FnLock/uinput remapping, status LEDs, ASPM, APU memory and GPU tuning. It does not authorize a generic root/sysfs/shell fallback in Orbis: each provider still needs typed ownership and read-back.

## Execution topology and rationale

One bounded developer audit/repair worker is the minimum reliable topology because the actions share UI-to-session-to-Hardware1 semantics; parallel writers would create needless contract overlap. One independent QA worker verifies the frozen candidate. An installed-target hardware-validation worker follows QA only after explicit user authorization because it can write actual laptop settings. Planner terminal release acceptance is last.

No separate integration stage is needed: the audit worker is the sole writer and freezes the assembled candidate. Candidate-bound downstream work is deliberately materialized only after that candidate exists.

## Requirement traceability

- REQ-LAUNCH, REQ-HONESTY, REQ-NO-RESTORE and REQ-BACKEND-GONE → W1 and W2.
- REQ-PROFILE, REQ-FAN, REQ-POWER, REQ-BOOST and REQ-GPU-MODE → W1, W2 and W3 for controls the target reports writable.
- REQ-VISUAL and REQ-REGRESSION → W1 and W2.
- REQ-PACKAGE and REQ-VERIFY → W3 and W4.

## Repository / environment baseline

- Worktree: `/home/artt/Orbis-control-base-20260923`.
- Base branch: `planner/orbis-base-20260923`.
- Base commit: `05064b4ff5c9ad2a500956f04f81d36237cb2c43`.
- The worktree was clean before planning changes; `git diff --check` passed.
- Exact-source evidence: `cargo test --workspace --all-targets --locked` passed 259 unit tests plus 1 diagnostics integration test; `scripts/verify quick` passed.
- Local commits are allowed for candidate custody. Push, merge to `main`, tag, release and remote mutation are not authorized.

## Shared contracts and invariants

- D-012 remains locked; full parity is capability-gated P1–P6 work after v0.1, not scope deletion.
- GUI remains unprivileged. Privileged mutations use typed Hardware1 and polkit; no generic root, sysfs or shell mutation proxy.
- Requested, observed, pending/reboot-required, partial, unknown, unsupported, unavailable and read-only are distinct states.
- A request is not success until required authoritative read-back proves it. An uncertain write is never blindly retried.
- A visible control is operational with approved behavior or visibly non-actionable; no tappable placeholder.
- G-Helper code, assets and protocol implementation are not copied without a separate license/ownership decision.

## Interaction contract

| Control / gesture | User action | Observable result | Implementation owner | Runtime verification owner | QA scenario |
| --- | --- | --- | --- | --- | --- |
| Performance profile control | choose a supported profile | desired state becomes authoritative observed state or explicit error | W1 | W1/W3 | SC-PROFILE-SET |
| Fan curve / Factory Defaults buttons | edit/apply/reset a supported fan/profile | validated curve or truthful Accepted/Applied/unknown result; never fake reset success | W1 | W1/W3 | SC-FAN-CPU, SC-FAN-GPU, SC-FAN-RESET |
| Power / Dynamic Boost control | change a capability-writable setting | typed authorization plus read-back, pending, partial or error state | W1 | W1/W3 | SC-POWER-APPLY, SC-BOOST |
| GPU mode control | select a supported mode | observed/pending/reboot-required state, including partial capability | W1 | W1/W3 | SC-GPU-MODE, SC-GPU-PARTIAL |
| Advanced System control | click/toggle an enabled Advanced control | genuine owner/read-back; otherwise disabled/read-only with a reason | W1 | W1/W3 | SC-UNSUPPORTED, SC-READONLY |
| Lifecycle / diagnostics navigation | launch and use tray/preferences/diagnostics | normal unprivileged behavior and privacy-bounded export | W1 | W1/W3 | SC-LAUNCH, SC-BACKEND-GONE |

## Workstream ownership

- W1 owns an audit/repair worktree, `crates/orbis-ui`, directly necessary session/client/hardware tests and corresponding evidence. It must not redesign the 10-screen UI, add parity-wave scope, add generic privileged fallbacks, push/tag/release, or modify code absent an audit finding.
- W2 is the independent QA owner for the exact W1 candidate. It owns evidence only, not implementation repairs or requirements.
- W3 owns the exact installed-package and target-Arch validation after QA and explicit physical-write authorization. It must not perform unapproved irreversible writes or broad experimentation.
- W4 is Planner terminal acceptance and candidate/evidence custody only.

## Asset and reference readiness

No external visual asset is needed. The checked-in canonical 10-screen UI and existing snapshot/review artifacts are the visual reference. G-Helper Linux is a technical behavior reference only; no screenshot, icon, artwork, code or protocol is imported.

## Workstreams and ordering

### W1: enabled-action integrity audit and bounded repair

Developer starts from the stated fresh base, inventories every enabled user action, repairs only a demonstrated integrity defect, runs deterministic checks and freezes a committed candidate with action-inventory evidence.

### W2: independent candidate QA

QA independently verifies the exact W1 candidate against launch, honesty, profile, fan, power/boost, GPU partial/pending, no-restore, backend-gone, visual and regression scenarios. QA reports defects; Planner owns any repair topology.

### W3: installed Arch target validation

After W2 PASS, developer builds and installs the exact candidate package, checks service/D-Bus/polkit/normal-user GUI liveness and, only with explicit user permission, makes bounded reversible round trips for controls reported writable.

### W4: terminal release acceptance

Planner re-verifies candidate identity, evaluates W1–W3 evidence and performs an independent highest-risk spot-check. Tagging and publishing remain out of scope until separately requested.

After v0.1 terminal acceptance, P1–P6 each receive their own approved execution design; they are not merged into this graph.

## Dependency graph

```text
W1 candidate
  -> W2 independent QA on W1 SHA
  -> W3 installed-target validation on QA-passed SHA (requires explicit physical-write authorization)
  -> W4 Planner terminal release acceptance
```

## Integration strategy

No separate integration stage is needed: W1 is the single writer and freezes the assembled candidate. Before QA, W1 executes the User-Liveness Smoke on its own candidate; QA does not replace this first runtime interaction verification.

## Acceptance strategy

- W1 runs `cargo test --workspace --all-targets --locked`, `scripts/verify quick`, targeted P2P/fake checks for touched owners, a UI action inventory and `git diff --check`.
- W1 runs the User-Liveness Smoke: launch the UI/snapshot target and exercise every touched enabled control against a fake/P2P owner, proving no unowned callback or false-success result.
- W2 is independent QA: it runs the mapped acceptance scenarios and an exploratory control-inventory pass rather than relying only on scripted checks.
- W3 validates the immutable installed package, services, D-Bus, polkit and normal-user launch; with user consent it proves bounded reversible writable-control behavior and restoration where possible.
- W4 performs terminal release acceptance: exact SHA/artifact custody, evidence review and an independent release spot-check of launch, a primary interaction and a secondary navigation/control path.
- Visual acceptance covers the current reference UI at 980×680: visible controls are complete, empty/full and disabled states are honest, and clipping/overlap is absent.

## User-Liveness Smoke

On the exact W1 or installed W3 candidate, perform the core user flow: normal-user launch → open Performance or Graphics → inspect authoritative state → use one supported primary interaction where a fake/P2P owner or authorized hardware is available → return to Dashboard/System. The observable result must be the authoritative state change or explicit pending/error state, never an unqualified success claim. Then open a secondary control/navigation path (tray/preferences or diagnostics) and confirm it remains usable.

## Visual / product acceptance

The canonical checked-in 10-screen UI, not a pixel-constrained external mockup, is the visual reference. Review the 980×680 layout for clipping and overlap and ensure a visible control is either complete and operational or visibly disabled/read-only/unsupported. Error and pending messages must remain visible and must not misrepresent a mutation as applied.

## Acceptance matrix

| ID | prerequisite | action | expected observable result | evidence | severity |
| --- | --- | --- | --- | --- | --- |
| AR-1 | W1 baseline | inspect each enabled action | typed/session owner or truthful non-writable state | action inventory + tests | blocker |
| AR-2 | fake/P2P owner | change profile/fan/power/GPU control | read-back, pending, partial or explicit error; no fake success | test/runtime log | blocker |
| AR-3 | W1 candidate | launch and traverse surfaces | unprivileged launch; no stale writable state after owner loss | runtime evidence | blocker |
| AR-4 | W1 candidate | deterministic verification | tests and quick verification pass; no whitespace errors | command output | blocker |
| AR-5 | W2 | independent scenario and exploratory QA | verdict covers all mapped v0.1 scenarios | QA evidence | blocker |
| AR-6 | W3 authorization | install and exercise exact package | package/services/polkit/GUI work; writable round trips are bounded/restored | installed-target evidence | blocker |
| AR-7 | W4 | custody and spot-check | all gates name one candidate; no unsafe mutation is hidden | Planner release record | blocker |

## Execution recovery policy

A failure is classified before action. A coherent near-complete W1 run resumes once in the same worktree. An oversized or structurally wrong run is split and re-planned. A bounded verified defect gets one Planner-created repair card. An environment or hardware absence blocks W3 with the exact missing evidence; it cannot convert incomplete physical validation into PASS. No time limit weakens a verification gate.

## Kanban mapping

- W1 is the only pre-freeze developer implementation card; it uses `planner/orbis-base-20260923` at `05064b4ff5c9ad2a500956f04f81d36237cb2c43` plus the planning commit.
- W2, W3 and W4 are identity-bound and are created after W1 freezes its candidate, not from archived S5/S5-R cards.
- Existing historical blocked/archived cards on `orbis-v01` remain untouched and are not dependencies of W1.
