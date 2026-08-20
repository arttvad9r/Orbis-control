# Orbis Control cloud backlog consolidation — 2026-08-19

> **HISTORICAL SNAPSHOT — backlog topology is superseded.** This file records
> the Draft-PR stack and priorities as they existed on 2026-08-19. Most PR
> references below are no longer the current integration topology, Rust 1.85 is
> no longer the repository toolchain, and the preferences warning redaction
> source fix now exists. Use GitHub Issues, [`roadmap.md`](roadmap.md),
> [`current-state.md`](current-state.md) and [`history.md`](history.md) for active
> work. The original snapshot is retained below for provenance.

Baseline for this snapshot: `chatgpt/production-hardening-20260818` at `f0d610a04a8ff16501a02389568c7a4aee62aac8`.

This document is backlog/status only. Open Draft PRs are **not** treated as part of the baseline until they are reviewed/integrated. `TESTED`, `PACKAGED` and `LIVE-VALIDATED` evidence remain separate.

## 1. Cloud Draft work ready for review/integration

These slices have a concrete Draft PR or validated implementation/audit artifact. They are not merged by this backlog.

### Preferences / user state

- Preferences storage foundation — PR #19; follow-up durability correction — PR #51.
- Start Minimized runtime consumption — PR #70, targeted validation passed.
- Window-position state storage foundation — PR #54.
- User XDG autostart backend — PR #57.
- Run on Startup UI wiring to the real owned XDG entry — PR #71, targeted validation passed.
- Close Action remains excluded because a real tray backend is not yet present.

### Desired / lifecycle foundations

- Desired / Observed / Pending pure domain state — PR #59.
- Separate typed desired-state persistence — PR #64.
- Lifecycle event domain (`Startup`, `Resume`, `BackendRecovered`, `CapabilityChanged`) — PR #65.
- No reconciliation executor/loop is claimed by those foundations.

### Diagnostics

A substantial diagnostics stack exists as Draft work: typed domain/providers/adapters, capability/service/GPU/telemetry/display/ASUS observations, application collector and UI DTO. The collector/DTO slices were targeted-validated in their respective branches.

The diagnostics window wiring remains an active separate validation stack (PR #60). Do not add overlapping UI wiring until that branch settles.

Safe text/JSON/clipboard export is not yet implemented and must be built only on the final typed snapshot/DTO with a privacy allowlist.

### Release/readiness documentation

- Release evidence taxonomy — PR #66.
- Beta acceptance checklist refresh — PR #67.
- ADR for `ApplyResult::Accepted != Applied` — PR #68.
- Support-matrix schema — PR #72.
- Security boundary audit — PR #73.
- Privacy audit — PR #74.
- Release metadata design — PR #75.
- Desktop/AppStream source metadata — PR #76.
- Multi-model probe-driven discovery research — PR #78.
- Power-limit readiness audit — PR #79.
- Extended ASUS controls readiness audit — PR #80.
- Dependency audit/blocker record — PR #81.
- ProviderStatus rustdoc cleanup — PR #82.
- Test-gap audit + hermetic core regressions — PR #84, targeted validation passed.

## 2. Audit complete / no implementation patch required

### Preferences strict schema

The current preferences foundation already rejects unknown fields, refuses future schema versions without rewriting the source, and preserves malformed/invalid source. No extra item-#20 patch was justified.

### `ApplyResult::Accepted` consumers

A full source audit found no production collapse of `Accepted` into `Applied`. Core `is_applied()` remains Applied-only; Aura uses config-confirmed `Accepted`; GPU UI handles unexpected Accepted fail-closed. No implementation patch was required for item #58.

### Updates UI

The current Updates window is already an honest local preview: it explicitly says the network backend is not connected, and its actions do not perform network/package operations. Item #82 needs no patch until a typed updater contract is designed.

## 3. Blocked by live hardware / authoritative evidence

### FA707NV support-matrix population

Old project notes contain model-specific statements, but the available records do not meet the newer provenance requirements for `LIVE-VALIDATED` support-matrix entries. Do not retroactively promote them. Populate the matrix only from an evidence record containing exact revision/environment/procedure/result.

### Power limits

Production support is blocked. Domain/trait scaffolding exists, but there is no production `PowerLimitProvider`, no typed Hardware1 mutation contract, and no authoritative per-field unit/range/default/read-back evidence. Attribute names alone are insufficient. See PR #79.

### Extended ASUS controls

Boot sound, MCU powersave, panel-HD-like controls and eGPU controls have no proven current Orbis production concept/provider contract. AniMe is only partially represented by legacy domain/provider vocabulary; Slash lacks a dedicated provider contract. Read-only evidence research must precede any write path. See PR #80.

### Product GPU modes

Eco / Standard / Ultimate / Optimized remain a product policy abstraction. Physical MUX, dGPU access and runtime power are independent observations and must not be collapsed into product mode. Mutation remains blocked until mapping/lifecycle/pending semantics are versioned and live-validated.

### Beta live acceptance

Packaged install, real session/system bus behavior and model-specific live read/write checks remain separate gates. CI/unit evidence cannot close them.

## 4. Requires local or stack integration

### Diagnostics UI composition

PR #60 is still an active validation stack. Finish/resolve that stack before adding export/copy behavior or another window-wiring implementation.

### Close-to-tray

`HideToTray` cannot be implemented honestly as `window.hide()` without a real tray restoration path. A tray backend and lifecycle ownership decision are prerequisites.

### Window-position runtime wiring

The storage foundation is isolated in PR #54. Runtime restore/save should be integrated only after the relevant preferences/UI stacks are consolidated, to avoid duplicating window lifecycle ownership.

### Nix desktop integration

PR #76 contains source `.desktop` and AppStream metadata only. Current `package.nix` excludes `data/**`, so actual installation needs an explicitly authorized packaging change. An icon asset is also still required. Do not claim desktop integration as `PACKAGED` before that work and packaged validation.

### hardwared/sessiond/Nix comment drift

The security audit found stale comments describing hardwared as Performance-only. Update them together with the locally owned subsystem/package work rather than touching those files from an unrelated cloud slice.

## 5. Cross-cutting dependency/toolchain blocker

Item #77 found unused direct `orbis-ui` dependencies (`clap`, `serde`, `serde_json`), but the cleanup candidate cannot yet receive normal green UI validation:

1. the hardening UI manifest contains an existing multi-line inline dependency table rejected by pinned Cargo/Rust 1.85;
2. after normalizing that syntax in validation, the UI build reaches external `fontdue 0.9.4`, which uses `integer_sign_cast` APIs unavailable on Rust 1.85.

Do not hide this by hand-editing `Cargo.lock` or casually bumping the toolchain in the cleanup PR. Resolve compatibility as a dedicated dependency/toolchain decision, then re-run the one-file cleanup candidate.

## 6. Confirmed privacy remediation still required

The privacy audit found that raw TOML/serde warning payloads can be formatted into UI logs and may include excerpts from a corrupted user preferences file.

Required next implementation:

- map warning variants to fixed redacted categories;
- log only the category, never the embedded parser/schema diagnostic string;
- add a hermetic secret-marker regression proving loggable output cannot contain arbitrary config contents.

This is the highest-priority small cloud-safe fix because it closes a concrete source-level privacy finding without touching hardware behavior.

## 7. Safe cloud-next queue

In recommended order:

1. **Preferences warning redaction + hermetic regression** — narrow privacy fix, no hardware/Nix changes.
2. **Finish diagnostics window stack review**, then add privacy-safe text export, JSON export and Copy Summary as pure snapshot/DTO projections.
3. **Dependency/toolchain compatibility investigation** — dedicated resolution for pinned Rust 1.85, existing UI manifest syntax and `fontdue`; no opportunistic lock edits.
4. **Support-matrix tooling/fixtures** — schema validation and empty/unknown examples only; no invented model support facts.
5. **Stack-level integration tests** after preferences/desired/diagnostics Draft chains are consolidated; avoid duplicating temporary interfaces.
6. **Documentation drift cleanup** where files are not locally owned; hardwared/sessiond/Nix comments wait for the corresponding local integration window.

## 8. Post-beta queue

Keep these behind evidence/integration prerequisites:

- product GPU policy mutation and automatic GPU switching;
- power-limit writes/default restoration;
- AniMe/Slash/boot sound/MCU/eGPU/new firmware controls;
- generalized automation privileged execution;
- broad multi-model enablement based on model strings;
- generic root/sysfs writer APIs;
- network auto-update/install behavior until a typed updater/security contract exists.

## 9. Immediate integration order

A practical merge/review order should preserve dependency direction rather than PR creation order:

1. review evidence/privacy/security documentation that does not alter runtime;
2. consolidate preferences storage + durability before UI consumers;
3. integrate theme/start-minimized/autostart/window-state consumers only onto the accepted preferences base;
4. consolidate Desired/Observed/Pending + desired-state storage + lifecycle events before designing reconciliation;
5. stabilize diagnostics provider/collector/DTO stack, then finalize window wiring;
6. only then add diagnostics exports and broader acceptance tests;
7. resolve package/toolchain issues in dedicated slices before claiming packaged beta readiness;
8. perform exact live validation last, per capability/read/write/model/revision.

## 10. Current non-claims

This consolidation does not claim that Draft PRs are merged, that desktop metadata is packaged, that diagnostics export exists, that tray behavior exists, that power limits are safe, that product GPU modes are proven, or that old FA707NV notes satisfy current `LIVE-VALIDATED` requirements.

No implementation code is changed by this backlog document. No merge and no live hardware writes are performed.
