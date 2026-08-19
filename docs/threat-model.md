# Threat Model — Orbis Control

> Роль: **CURRENT SECURITY DESIGN**. Документ описывает текущие trust boundaries,
> реализованные mitigations и отдельно открытые security work items. Фактическая
> готовность функций — в [`current-state.md`](current-state.md), architecture — в
> [`architecture.md`](architecture.md).
>
> Обновлено: 2026-08-19.

## 1. Trust boundaries

```text
TL0  user session / orbis-ui
  │ session D-Bus reads
  ▼
TL1  orbis-sessiond (unprivileged user daemon)
  │ read-only system D-Bus / kernel reads
  ▼
TL2  system services: UPower / asusd / supergfxd / logind
  │
  └──────────────► kernel ABI / sysfs / hwmon

Separate privileged mutation boundary:

TL0 original application caller
  │ system D-Bus Hardware1
  ▼
TL4 orbis-hardwared (root, sandboxed)
  │ per-capability polkit + bounded backend
  ▼
TL2/TL3 mutation owner / fixed kernel ABI
```

Critical rule: `orbis-sessiond` is never a privileged mutation deputy. A second
D-Bus hop would lose the original application caller identity used by polkit.

## 2. Assets

| Asset | Main risk |
|---|---|
| Hardware settings | unsafe/incorrect writes, thermal/stability impact |
| GPU/display lifecycle | loss of display/session, reboot/logout requirements |
| User preferences / desired state | unintended automatic application |
| Capability evidence | false writable/supported state |
| Diagnostics/export | privacy leakage |
| D-Bus/system services | spoofed/malformed/stale backend data |
| Root helper | privilege escalation / generic write primitive |

## 3. Current attack surfaces and mitigations

### 3.1 `orbis-ui`

Implemented expectations:

- normal user process; no generic root execution path;
- Slint callbacks route hardware work through application/worker/service boundaries rather than direct privileged sysfs writes;
- backend-derived controls use explicit Loading/Ready/Unavailable and typed capability evidence;
- unsupported/incomplete write paths can be disabled fail-closed;
- current fan writes are disabled in UI because known write-contract issues are unresolved;
- mock fixtures are not accepted as authoritative production hardware evidence.

Remaining risk:

- development/mock code still exists in the default UI feature graph (#115);
- some product controls are visual/preview only and must remain clearly labelled until connected;
- Run on Startup and Diagnostics lifecycle glue are incomplete (#110/#111) and therefore their controls stay disabled.

### 3.2 `orbis-sessiond`

Implemented expectations:

- unprivileged user daemon;
- getter/read boundary only for privileged concepts;
- no Hardware1 mutation delegation;
- fresh/no-cache D-Bus reads where authoritative state matters;
- Battery/UPower discovery is lazy and capability-local;
- malformed wire/backend state is returned as typed failure rather than a fake successful value;
- D-Bus name ownership prevents two services owning the same Session1 name.

Security consequence: compromise of `sessiond` is limited to the user/session boundary and read access granted to that user; it must not grant root mutation through Orbis.

### 3.3 `orbis-hardwared`

This is the most sensitive component.

Implemented mitigations:

- system service runs as root but exposes a narrow typed Hardware1 API;
- no caller-supplied filesystem path, shell command or generic D-Bus proxy API;
- D-Bus input is decoded/validated before mutation;
- each mutation method uses a dedicated polkit action;
- polkit subject is the original Hardware1 sender's `system-bus-name`;
- package policy defaults use `allow_any=no` and `allow_inactive=no`;
- systemd sandbox includes `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, `ProtectControlGroups`, `MemoryDenyWriteExecute`, `RestrictAddressFamilies=AF_UNIX`, read-only `/sys` and a minimal explicit writable path list;
- `CapabilityBoundingSet` is empty in the current NixOS service;
- fixed-path Performance and keyboard writes use authoritative read-back;
- Battery mutation uses typed asusd ownership plus configured/effective confirmation;
- production raw GPU mutation is disabled.

Known security/correctness blockers:

- fan custom write does not currently preserve upstream `CurveData.enabled` (#104);
- upstream Factory Defaults may fail before restoring the previous performance profile (#105);
- fan capability/read evidence is over-aggregated across CPU/GPU (#109);
- fan `enabled` evidence is lost before UI presentation (#116).

Mitigation currently deployed: fan mutation/default-reset is disabled in the Fans UI and the packaged fan polkit action has `allow_active=no`. Do not remove this block until those contracts are fixed, executable tests pass and required live evidence is recorded.

### 3.4 External services and kernel interfaces

Orbis treats D-Bus/backend data as untrusted protocol input even when the service itself is system-trusted.

Threats:

- service missing or restarting;
- version/interface drift;
- malformed/new enum values;
- permission changes;
- stale assumptions about write support;
- competing ownership of a hardware setting.

Mitigations:

- typed adapters and strict decode;
- no implicit property cache for authoritative reads;
- capability-local errors;
- read/write evidence separated;
- read-back after confirmed mutations where possible;
- no model-name inference as runtime support evidence.

Open evidence hardening:

- Battery/Panel/Aura write status must prove the actual owner/interface is reachable (#107);
- Battery discovery must not collapse permission/transient errors into structural Unsupported (#108);
- explicit capability refresh must re-query write evidence (#112).

## 4. Confused-deputy model

Forbidden production flow:

```text
GUI → Session1/sessiond → Hardware1
```

Reason: Hardware1 would see `sessiond` as the caller instead of the originating application process. A same-UID background/SSH/linger process must not gain a privileged mutation merely because another active session exists.

Required flow:

```text
original application caller
→ Hardware1
→ polkit(system-bus-name)
→ bounded mutation
```

Caller-provided UID/PID/session metadata, executable paths and user-unit names are not trust anchors.

## 5. Capability/evidence spoofing

A major product-security risk is not only unauthorized write, but a false `Supported`/`Applied` claim that causes the user or later automation to take an unsafe action.

Rules:

- file/object presence alone is not write support;
- validation success alone is not write support;
- overall capability status must not substitute for `operations.write.status`;
- `Accepted` is not `Applied`;
- persisted Desired state is not Observed state;
- an empty/partial telemetry attempt is not automatically proof of useful fresh data (#117);
- stored fan curve points are not proof that the custom curve is enabled or currently active (#116).

## 6. Persistence and automation

New production stores are separated by concern: preferences, window state, autostart and desired-state foundation.

Security rules:

- loading a config/default never performs a hardware write by itself;
- missing/corrupt config never becomes a synthetic desired hardware action;
- Desired/Observed/Pending remain separate;
- reconciliation is not implemented until its policy and evidence are explicit;
- legacy AppConfig/path helpers must not become reconciliation or new state/cache foundations until hardened (#113).

## 7. Diagnostics/privacy

Current diagnostics design is allowlist-based and read-only.

Allowed categories include package/build metadata, non-unique system/session metadata, privacy-safe hardware summary, service presence, capability evidence, GPU primitives, telemetry state and display observations.

Explicitly avoid collecting/exporting by default:

- serial numbers, machine UUIDs, asset tags;
- arbitrary environment dumps;
- arbitrary files/logs/journals;
- shell command output;
- credentials/secrets;
- full home paths.

Diagnostics does not activate stopped services. The UI remains fail-closed until its lifecycle/refresh glue is complete (#111).

## 8. Dangerous operations

Operations with high user/system impact require concept-specific safety policy before exposure, for example:

- GPU/MUX changes with reboot/logout or display-loss requirements;
- power limits / undervolting;
- fan writes/default resets;
- CPU core disablement;
- firmware operations;
- service ownership/policy changes.

A future confirmation flow must explain what changes, which owner/backend is used, whether a reboot/logout is required, what confirmation/read-back exists and what happens on failure. A dialog is not a substitute for backend validation or authorization.

## 9. Packaging/system boundary

Nix packaging installs the Hardware1 D-Bus policy and per-capability polkit policy used by the service. Standalone/policy-only packaging uses the same policy source, so a blocked action must remain blocked consistently across deployment paths.

Current release blockers:

- GitHub Actions jobs fail before first step (#106), so executable repository verification is unavailable;
- `main` is not protected until CI can become a real required check (#114);
- obsolete remote agent branches require later pruning with branch-delete access (#118).

## 10. Security acceptance rules

Before enabling a new privileged capability:

1. define the exact user-visible concept;
2. prove the real write owner and non-mutating support evidence;
3. define a bounded typed API and validate untrusted input;
4. preserve original caller authorization;
5. define confirmation/read-back or explicit Accepted/Pending semantics;
6. test failure, denial, unavailable and malformed cases;
7. validate packaging/policy on the exact revision;
8. perform controlled live hardware validation when the claim depends on real device semantics.

Known-unsafe functionality must be disabled rather than left enabled because a partial implementation exists.

## 11. Assumptions

- attacker does not already control root or the system bus;
- kernel and installed system services are part of the trusted computing base, but their data/API versions may be malformed, unavailable or incompatible from Orbis' point of view;
- attacker may run arbitrary processes as the same user and may call user/session D-Bus APIs available to that user;
- package/repository integrity and host NixOS/polkit configuration are trusted at installation time.

Changes to these assumptions require a new security review/ADR.