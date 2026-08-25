# Current State

> Роль: **CURRENT STATUS**.
> Обновлено: **2026-08-25**.
> Source snapshot: development branch (consolidates `asus-hardware-validation-20260821` / PR #129 head `e8b611e`).
>
> `main` остаётся последней консолидированной baseline до отдельной интеграции этой ветки. Claims используют [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md): `IMPLEMENTED / TESTED / PACKAGED / LIVE-VALIDATED / BLOCKED / UNKNOWN`.

## Executive summary

Активная интеграционная ветка содержит существенно более новый UI/runtime/backend слой, чем `main`. Source-level production вертикали существуют для Battery, Performance, независимых GPU read primitives, telemetry, fan reads, preferences/autostart/window lifecycle, diagnostics и read-only CLI.

Privileged mutation architecture остаётся узкой: original caller → typed `Hardware1` → capability-specific polkit → bounded backend/read-back. `sessiond` — user-session/read boundary и не является privileged deputy.

Production product path включает только доказанные writes. Performance, Battery,
FA707NV keyboard backlight, ASUS fan curves and ASUS product GPU queue are live
mutation owners; raw supergfxd GPU, Panel and Aura remain product/policy
blocked. Automation execution promotion=false, Display modeset не имеет
concrete owner, Updates не имеет canonical signed feed/installer owner.

GitHub Actions is optional/manual for this single-owner project and is not a release gate. Canonical executable evidence is the local flake workflow: locked Cargo fmt/check/test/clippy, `python3 scripts/verify-static` and full `nix flake check --max-jobs 1 --cores 4`. These remain revision-scoped evidence; live hardware evidence remains separate.

## Repository status

- Active integration branch: `development` (pushed to `origin/development`; consolidates the former `asus-hardware-validation-20260821` line).
- Draft integration PR: #129 was closed on 2026-08-25 as superseded: its head `e8b611e` is a verified direct ancestor of `development` (`merge-base --is-ancestor`), so no unique work was lost. `development` remains the single integration line.
- `main` — previous consolidated baseline; required hosted checks are intentionally not used by project policy.
- Единственный canonical workflow — `.github/workflows/ci.yml`.
- Удалён оставшийся одноразовый `.github/keyboard-backlight-probe-validation-trigger` artifact.
- Canonical docs hierarchy определена в `docs/README.md`; historical plans/audits отделены через `history.md`.
- Obsolete remote refs are fully pruned (#118): the merged PR-head branch `chatgpt/ui-refresh-ghelper-20260820` (`e8b611e`, verified ancestor of `development`) was deleted on 2026-08-25; no `agent/*` or probe refs remain. Only `main` and `development` exist.
- Read-only ASUS FA707NV evidence is recorded in [`hardware-evidence/asus-fa707nv-baseline.md`](hardware-evidence/asus-fa707nv-baseline.md).

## Current areas

| Area | Status | Current fact |
|---|---|---|
| Repository baseline | CLEANED / IMPLEMENTED | One-off validation artifact удалён; canonical workflow/document hierarchy сохранены; интеграция консолидирована в ветку `development` (PR #129 head `e8b611e` сохранён как исходная история). |
| Rust/build contract | IMPLEMENTED | Workspace MSRV/toolchain contract — Rust 1.87. |
| Hosted CI | OPTIONAL / MANUAL — #106 closed by policy | GitHub Actions is retained as optional infrastructure only; it is not required for merges or release acceptance. |
| Main protection | NOT REQUIRED BY POLICY — #114 closed | Single-owner project uses local canonical checks and explicit review; no hosted required-check dependency. |
| Remote branches | CLEANUP COMPLETE — #118 | На remote остаются только `main` и `development`; obsolete refs удалены. |
| Application identity | RESOLVED — #124 | ADR 0013 фиксирует постоянную canonical identity `io.github.orbiscontrol.Orbis`, независимую от GitHub hosting owner. |
| Core/domain | IMPLEMENTED | Typed capability/state/action/lifecycle models, `Accepted != Applied`, Desired/Observed/Pending foundations. |
| Preferences/config | HARDENED / COMPAT CLEANUP CLOSED — #113 | Checked XDG paths, inert defaults, atomic/durable stores; deprecated legacy path symbols (`config_dir`, `config_file`, `state_dir`, `cache_dir`, `config_dir_with`) removed with zero external consumers confirmed; hardened stores/checked resolvers are the only remaining API. |
| Autostart | IMPLEMENTED SOURCE — #110 closed | Owned XDG desktop entry read/write/read-back wired to Preferences. |
| Window/tray lifecycle | IMPLEMENTED SOURCE — #121 closed | Start Minimized, X11-style position restore/save, Wayland fail-closed behavior, StatusNotifier tray gating and explicit Quit lifecycle. |
| Diagnostics | IMPLEMENTED SOURCE — #111 closed | Runtime initialization, capability generation replacement, Refresh, privacy-bounded JSON export and Copy Summary wired; Open Logs intentionally disabled. |
| Battery read | IMPLEMENTED / ASUS+SYSFS CONSENSUS | Current charge/status come from UPower; configured/effective charge-limit state is accepted when ASUS/asusd and kernel sysfs agree. UPower threshold is diagnostic only. |
| Battery mutation | HISTORICAL + CURRENT LIVE-VALIDATED / CLOSED — #107 | Controlled Hardware1 path with dynamic asusd owner-liveness requery (`NameHasOwner` non-activating, fresh uncached `Properties.Get`, interface-drift → `TemporarilyUnavailable`). Proven live on 2026-08-25: real-asusd owner present → `Supported` per query (stale revision showed boot-race `TemporarilyUnavailable`), plus a full charge-limit round-trip with Confirmed consensus; see [`hardware-evidence/fa707nv-live-validation-20260825.md`](hardware-evidence/fa707nv-live-validation-20260825.md). Fake-system `battery-mutation-vm` executes supported → owner-loss → restore → interface-drift → heal generations locally. |
| Performance read/write | HISTORICAL LIVE-VALIDATED / CURRENT SOURCE IMPLEMENTED | Session1 read + Hardware1/polkit write + read-back. |
| GPU primitives | HISTORICAL LIVE-VALIDATED READS | Power, physical MUX and access policy remain separate read concepts. |
| GPU product mode read | IMPLEMENTED SOURCE / ASUSD ARMOURY | Read-only `dgpu_disable + gpu_mux_mode` decoder reports Hybrid/Integrated/Ultimate; Optimized is not inferred. Current host reports Hybrid (fresh read-only Armoury snapshot 2026-08-24: `CurrentValue` pair `(0,1)`, both `QueuedGpuValue=-1`). |
| ASUS product GPU queue operation | LIVE-VALIDATED FA707NV / PRODUCTION WIRED | Typed `Hardware1.SetProductGpuMode` paired queue backend is production-wired with active-session polkit. Live Hybrid no-op and Integrated→Hybrid staged restore passed on FA707NV; current/queued pairs and `reboot_required=false` read back authoritatively. No automatic reboot is performed; raw supergfxd path remains disabled because the service is absent. |
| GPU raw mutation | BLOCKED | Raw `Hardware1.SetGpuMode` remains disabled because supergfxd is absent. ASUS product GPU queue is separately live-validated and production-wired. |
| Capability registry | IMPLEMENTED / RESILIENCE COVERED | Whole-swap immutable generations; explicit and periodic refresh share canonical mutation-status requery (#112 closed with an executed P2P integration test proving a real status transition through explicit refresh); backend loss/timeout and recovery publish capability-local availability transitions. UI/Diagnostics/support-policy source audit #120 is complete; Battery owner evidence is now dynamic (#107 owner liveness). |
| Provider execution | EXECUTED LOCALLY — #123 closed | Canonical `bounded_provider_call`; public probes, CLI and main worker read refreshes use provider deadlines; telemetry owns provider identity/deadline (`provider_id`/`snapshot_timeout`); Hardware1 status requery bounded (`HARDWARE1_STATUS_DEADLINE`); mutation unknown outcome classified `Unconfirmed` without retry. Full local flake evidence is green. |
| CLI | IMPLEMENTED READ-ONLY / LOCALLY EXECUTED — #119 closed | `status` + versioned `status --json` (schema 3 includes battery threshold evidence); typed states; no mutation commands. Executable local validation + service-absent integration test recorded. |
| Telemetry | IMPLEMENTED / FIELD EVIDENCE COMPLETE — #117 closed | Partial metrics are supported. UI freshness honors snapshot quality: an `Ok` snapshot with no observed field is absence evidence (`telemetry_fresh=false`, last-good values preserved); provider contracts for empty root, all-sources-failing and permission-denied field-local degradation are pinned by tests. Field-local gap evidence is on the provider API and presented in Settings → Diagnostics, Copy Summary and JSON export. |
| UI shell | REDESIGNED / SOURCE-COMPLETE + LOCALLY EXECUTED — #128, sidebar redesign 2026-08-25 | Frameless preferred-1200×800 window (min 980×680): 230px sidebar with ten `Section` pages (Dashboard, Performance, Power, Cooling, Graphics, Backlight, Display, System, Settings, About), 44px title bar (drag via slint `unstable-winit-030` → winit `drag_window()`, minimize/maximize, close through the shared close-action tray/quit path). Calm light/dark token pair (off-white/white cards, one blue accent; WCAG AA pinned by `check-ui-contract.py`). Dashboard carries device header, performance modes, telemetry, read-only fan curve, charge limit and keyboard lighting; capability-honest pages hide unsupported controls. Updates and Automation editing surfaces remain removed; Diagnostics actions live in System. Full local flake evidence is green. |
| Fan reads | IMPLEMENTED / AGGREGATE TRUTH + STORED ENABLED CARRIED | Per-fan Session1 read exists; stored `FanCurveData.enabled` is carried through Session1/client/UI evidence (#116 source-complete) and rendered from `UiState` in the Fans section; aggregate FanCurves requires both CPU and GPU read contracts (#109 source-complete). |
| Fan writes/reset | LIVE-VALIDATED FA707NV / PRODUCTION WIRED | Typed asusd backend is production-wired with active-session fan polkit. Hardware1 Quiet reset returned Applied with fresh CPU/GPU curves; production same-value CPU write returned Applied and preserved exact points/enabled, GPU untouched. Unknown-outcome and profile-containment contracts remain fail-closed. Evidence: [`hardware-evidence/fa707nv-live-validation-20260825.md`](hardware-evidence/fa707nv-live-validation-20260825.md). |
| Panel write | HARD-BLOCKED | Typed API may exist; production backend status remains Unsupported/default-deny. |
| Keyboard write | LIVE-VALIDATED — current revision | FA707NV `/sys/class/leds/asus::kbd_backlight` promoted through typed Hardware1/polkit with exact sandbox paths; `3→0→3` returned Applied and authoritative read-back matched; bounded settle handles the device's asynchronous write. |
| Aura write | HARD-BLOCKED | Static RGB typed writer is not promoted into product/unattended execution. |
| Automation | IMPLEMENTED SHADOW/EXECUTOR CONTRACT, EXECUTION BLOCKED | Worker owns policy/lifecycle/debounce/recovery/serialization; resume performs read-only provider/capability refresh and never restores desired hardware state. Performance executor compiled but promotion=false. Other executors disabled. |
| Display Refresh | IMPLEMENTED CONTRACT / NO MUTATION OWNER | Typed target/request/read-back design exists; no concrete compositor configuration owner is enabled. |
| Updates | FAIL-CLOSED | Installation owner detection + typed blockers exist; no invented release feed/downloader/installer. |
| Release dependency graph | EXECUTED LOCALLY — #115 closed | GUI release graph contains no `orbis-test-support` (re-proven by `cargo tree -e normal --locked` on this revision): production startup uses `UiState::production_initial`; fixture bootstrap is dev-only + optional `ui-review` feature for screenshot builds (`cargo check -p orbis-ui --features ui-review` green). |
| GUI root boundary | EXECUTED LOCALLY — #125 closed | Interactive launch rejects euid 0 before preferences/runtime/bus setup (`LaunchContext::validate`, rustix geteuid); screenshot/offscreen paths remain explicit exceptions; decision unit tests executed green on this revision. |
| Hardwared sandbox | STRUCTURALLY MINIMIZED / LIVE-VALIDATED — #126 closed | Intended direct sysfs write surface is `platform_profile` only. All three system-integration VM checks green (`hardwared-lifecycle`, `performance-mutation-vm`, `battery-mutation-vm` incl. #107 status generations); source-level parity contract (`scripts/check-hardwared-unit-parity.py`) pins the standalone unit to the identical sandbox. Live-host validation executed 2026-08-25 on FA707NV at `7db695c4` through the canonical NixOS module deployment: effective unit shows `ProtectSystem=strict`, `NoNewPrivileges=yes`, `ReadWritePaths` limited to `platform_profile`; the standalone script path is intentionally refused on this host (NixOS-owned unit) and stays covered by VM+parity evidence. |
| ASUS FA707NV live read baseline | OBSERVED / READ-ONLY | `platform_profile`, asusd/asusctl profile, UPower, DRM/sysfs GPU, hwmon, thermal and power_supply reads were observed on FA707NV; Session1/Hardware1/hardwared and supergfxd were unavailable. This does not promote write support. |
| ASUS FA707NV platform profile mutation | LIVE-VALIDATED (current revision) | Controlled Hardware1 apply/read-back/restore re-validated on-device at `7db695c4` (deployed store package `96xcnyn6…` via the NixOS module), including the canonical `orbisctl validate platform-profile --apply-test` PASS; see [`hardware-evidence/fa707nv-live-validation-20260825.md`](hardware-evidence/fa707nv-live-validation-20260825.md). Evidence is scoped to that revision; it does not extend to later changes or other capabilities. |
| ASUS FA707NV battery mutation | LIVE-VALIDATED (current revision) | `SetChargeLimit` 100→80→100 through production Hardware1/polkit with kernel read-back and Confirmed AsusBackend+Sysfs consensus at both ends; dynamic `BatteryMutationStatus=Supported` observed against the real asusd owner (#107). Same evidence doc as above. |
| Research foundations (policy/preset/reconciliation/fan-policy/transaction/readiness/system-telemetry) | FOUNDATION | Merged modules are exported from `orbis-core`/`orbis-config`/`orbis-providers` public APIs but have no runtime consumers: worker, UI, sessiond and hardwared do not call them. Loading config remains hardware-inert. See "Research-foundation status" below. |

## Bounded execution status (#123)

Implemented in source:

- `orbis_providers::bounded_provider_call(provider, operation, future)` owns provider deadlines;
- no retry is performed by the generic timeout primitive;
- public capability probes use bounded adapters;
- Performance probe gives `profiles()` and `current_profile()` separate deadlines;
- probe timeout becomes local `TemporarilyUnavailable`, not false `Unsupported`;
- `orbisctl` reuses the same primitive;
- worker Battery, Performance, Fan and GPU authoritative refresh reads are bounded;
- GPU power/MUX/access reads execute independently with `tokio::join!`;
- explicit and periodic capability refresh use the same canonical refresh helper.

Closure status (2026-08-24, #123 closed):

1. telemetry owns canonical provider identity/deadline through `TelemetryServiceRuntime` (`provider_id`/`snapshot_timeout`); snapshot reads stay bounded via `provider.timeout()` (unit-tested);
2. Hardware1 mutation-status requery is bounded (`HARDWARE1_STATUS_DEADLINE`, timeout → `Unknown`);
3. mutation timeout after possible dispatch is classified across interactive Performance/Battery, the Automation Performance executor and the (gated) Fan Factory Reset path as `CommandError::Unconfirmed`/recovery: never success, never retried, rollback not auto-triggered; the outcome is obtained only through a subsequent authoritative read-back that confirms or refutes the desired state. Successful factory reset returns `ApplyResult::Accepted` (not `Applied`) because an independent default-evidence source does not exist;
4. `orbisctl validate` bus connects and status query are bounded (`VALIDATE_BUS_CONNECT_DEADLINE`/`VALIDATE_STATUS_DEADLINE`); the interactive confirmation and the confirmed mutation itself remain intentionally outside generic timeout (unknown-outcome contract);
5. hosted CI is optional by project policy; local workspace and full flake checks are the canonical executable evidence.

## Effective product-policy truth (#120 complete)

The current source contract is deliberately conservative:

- Performance/Battery/Fan UI writability derives from `operations.write.status`;
- Diagnostics carries and renders read/write statuses independently;
- Panel request paths require explicit `ProductWriteStatus::Supported`; keyboard is now live-wired on FA707NV and still uses operation-level status;
- support-matrix schema requires separate read/write evidence and the repository currently contains only `empty`/`unknown` examples, not optimistic model claims;
- current deliberately disabled/unvalidated product writes remain effective `Unsupported` with a reason rather than a generic `DisabledByPolicy` status that could imply already-proven hardware support.

These invariants are protected by the static backend completion contract and the local full flake check.

## Fan truth status (#109/#116)

The old claim that a requested CPU read requires both CPU and GPU curves is no longer true in this branch: Session1 profile-specific reads target a concrete `(profile, fan)` and strict-decode the returned fan.

Remaining fan evidence defects:

- aggregate `FeatureId::FanCurves` is published only when both CPU and GPU read contracts are proven; a single-fan read failure suppresses the aggregate, so CPU support never overstates UI-visible GPU support (#109 source-complete);
- stored `FanCurveData.enabled` is now carried end-to-end into `UiState` and the FansWindow (`fan-curve-enabled-known`/`enabled`) (#116 source-complete);
- custom write preservation of `enabled` is source-hardened in hardwared (#104 source-complete): the setter reads the authoritative `FanCurveData.enabled` before the write, passes it through to asusd, and the post-write read-back confirms enabled did not drift; factory-reset profile restoration remains a mandatory future-write blocker (#105, Orbis-side source contract enforced).

No fan writes should be enabled while these remain unresolved.

## Production boundaries

### Reads

```text
authoritative backend
→ Session1 / typed read provider
→ bounded application/worker read
→ GUI / CLI / diagnostics
```

A read failure is local evidence. Missing/denied/unavailable/unknown are not interchangeable and must not produce fake hardware defaults.

### Enabled mutations

```text
original GUI/application caller
→ Hardware1 system bus
→ capability-specific polkit
→ typed Performance or Battery backend
→ authoritative read-back / explicit result
```

Current product block remains defense-in-depth: UI gating + Hardware1 disabled backend + polkit/default sandbox where applicable.

## Local verification policy

Local executable Rust checks are available in the flake devShell and were executed green on this revision (`a752be6`, 2026-08-24, NixOS flake devShell, rustc 1.97.1):

```text
python3 scripts/verify-static                              → PASS
cargo fmt --all -- --check                                 → PASS
cargo check --workspace --all-targets --locked             → PASS
cargo test --workspace --locked                            → PASS (502 passed / 0 failed / 3 ignored)
cargo clippy --workspace --all-targets --locked -- -D warnings → PASS
git diff --check                                           → PASS
cargo tree -p orbis-ui -e normal --locked                  → no orbis-test-support (#115)
cargo check -p orbis-ui --features ui-review --locked      → PASS
```

Nix system-integration checks were executed locally on 2026-08-24 at `ce9ddab`:

```text
nix build .#checks.x86_64-linux.hardwared-lifecycle     → PASS (fake-system Hardware1 lifecycle/sandbox/policy)
nix build .#checks.x86_64-linux.performance-mutation-vm → PASS (polkit active session + typed profile mutation/read-back)
nix build .#checks.x86_64-linux.battery-mutation-vm     → PASS (after ce9ddab readiness gate; see below)
```

All three were re-executed green on 2026-08-24 at the #107/#126 revision
(`battery-mutation-vm` including the drift generations). Re-running
`performance-mutation-vm` exposed a pre-existing fixture race: getty autologin
respawns the runner script after PASS, so a fresh generation replayed
mutations while the driver read final state (`platform_profile` observed as
`quiet`). Both mutation fixtures now write a completion sentinel before
publishing PASS; later autologin generations idle (`exec sleep infinity`)
instead of replaying the scenario.

`battery-mutation-vm` initially failed at `a752be6`: the fake asusd unit was
Type=simple "started" before python acquired `xyz.ljones.Asusd`, so the one-shot
hardwared owner preflight lost that race and stayed fail-closed for the whole run.
`ce9ddab` adds an `ExecStartPost` readiness gate (bounded 10 s name-ownership wait);
the re-run shows no preflight warning and exactly one clean runner pass. This is a
test-fixture ordering fix only; product startup semantics are unchanged.

On the #107 contract-probe revision `battery-mutation-vm` was extended and
re-executed green: supported → owner-loss demote → restore → live interface-drift
demote (mis-serving daemon raising `UnknownProperty` while ownership stays
confirmed) → heal, all observed through `Hardware1.BatteryMutationStatus`
without restarting hardwared.

This is revision-scoped `TESTED` evidence. Hosted Actions are optional and do not replace full local flake or live hardware evidence.

Safe source work may continue when it does not widen unvalidated hardware writes:

- docs/source-of-truth cleanup;
- stale issue/repository artifact cleanup;
- static contracts;
- read-only/evidence improvements;
- fail-closed runtime hardening whose source semantics are locally reviewable;
- preparation of tests that will execute later.

Do not promote raw GPU/Panel/Aura writes, unattended Automation, Display modeset or self-update on static or local-test evidence alone; fan, keyboard and ASUS product-GPU promotion now have FA707NV live evidence.

## Research-foundation status (FOUNDATION, unwired)

The consolidated `development` branch includes domain modules merged from the research-mechanics line. They are honest foundations, not production features:

- policy/desired-state presets (`orbis-config::policy_desired`, `orbis-core` preset/preset-bundle);
- reconciliation decision/scheduling primitives;
- software fan-policy primitives (EMA, hysteresis, PWM rate limiting, interpolation);
- mutation transaction phases and audit model;
- readiness model with bounded probe helpers;
- system telemetry (Linux memory/PSI/zram/zswap parsers);
- telemetry history/export/statistics primitives;
- thermal-control pure computations;
- hardware-validation evidence state machine.

All of them are exported from crate public APIs but have **no runtime consumers**: the worker, UI composition, sessiond and hardwared do not call them. No preset import, policy selection or reconciliation pass can dispatch a hardware action today; configuration loading remains hardware-inert. Wiring any of these into production requires its own design step and must preserve Desired/Observed/Pending separation, `Accepted != Applied`, and the existing fail-closed product gates.

## Active blockers / next work

1. Fan production promotion gate: #104/#105 source + controlled live success-path evidence complete; production writes remain independently policy-gated.

## Historical live evidence retained

Revision-scoped evidence exists for controlled Battery and Performance mutations, independent GPU primitive reads and Hardware1/original-caller authorization on documented hardware. It is historical evidence, not a universal ASUS support guarantee and not proof for the current unexecuted branch.

## References

- [`architecture.md`](architecture.md)
- [`verification.md`](verification.md)
- [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md)
- [`backend-completion-status.md`](backend-completion-status.md)
- [`beta-acceptance-checklist.md`](beta-acceptance-checklist.md)
- [`roadmap.md`](roadmap.md)
- [`history.md`](history.md)

Update this file whenever active production behavior, capability evidence or release blockers change. Source inspection proves at most `IMPLEMENTED`; stronger claims require executed evidence.
