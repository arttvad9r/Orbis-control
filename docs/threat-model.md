# Threat Model — Orbis Control

> Роль: **CURRENT SECURITY DESIGN**. Реализованные mitigations и открытые
> security work items должны описываться отдельно. Фактическая готовность
> функций — в [`current-state.md`](current-state.md).
>
> Обновлено: 2026-08-19.

## 1. Trust boundaries

```text
TL0  user session / orbis-ui / orbisctl
  │ session D-Bus reads
  ▼
TL1  orbis-sessiond (unprivileged user daemon)
  │ read-only system D-Bus / kernel reads
  ▼
TL2  UPower / asusd / supergfxd / kernel ABI

Separate privileged mutation boundary:

TL0 original application caller
  │ system D-Bus Hardware1
  ▼
TL4 orbis-hardwared (root, sandboxed)
  │ per-capability polkit + bounded backend
  ▼
mutation owner / fixed kernel ABI
```

Critical rule: `orbis-sessiond` is never a privileged mutation deputy. Hardware1
must authorize the original system-bus sender, not a second session-daemon hop.

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

Implemented:

- hardware work routes through typed application/service boundaries, not generic privileged sysfs operations;
- backend-derived controls use explicit state/evidence instead of fake defaults;
- incomplete product controls are disabled/preview-only;
- fan and extended ASUS writes are fail-closed in current UI/policy.

Open:

- interactive GUI launch rejects euid 0 before preferences, runtime, or bus setup (#125). Screenshot/offscreen rendering remains explicitly exempt for test workflows;
- mock/test-support remains in the default release graph (#115);
- Run on Startup and Diagnostics lifecycle glue remain incomplete (#110/#111).

### 3.2 `orbis-sessiond`

Implemented:

- unprivileged user daemon;
- read/getter boundary for privileged concepts;
- no Hardware1 mutation delegation;
- Battery/UPower discovery is lazy and capability-local;
- malformed/new protocol values become typed failure rather than fake state;
- historical unimplemented `mockDevice` / `readOnlyEmpty` Nix options were removed instead of being silently ignored (#122 resolved).

### 3.3 `orbis-hardwared`

Implemented boundary:

- root system service, narrow typed Hardware1 API;
- no caller-supplied filesystem paths, shell commands or arbitrary D-Bus forwarding;
- strict wire/input validation before mutation;
- per-capability polkit actions use the original Hardware1 sender's `system-bus-name`;
- `allow_any=no` and `allow_inactive=no` for all packaged mutation actions;
- `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, `ProtectControlGroups`, `MemoryDenyWriteExecute`, `RestrictAddressFamilies=AF_UNIX`, empty `CapabilityBoundingSet`;
- `/sys` read-only except the single exact currently enabled direct-write path `/sys/firmware/acpi/platform_profile`;
- Performance performs bounded typed write + authoritative read-back;
- Battery uses typed asusd ownership plus configured/effective confirmation and does not require broad sysfs write access.

Current packaged polkit defaults intentionally allow only the two historical
live-validated production mutation slices:

| Mutation | Default active-user policy |
|---|---|
| Performance profile | allowed |
| Battery charge limit | allowed |
| Raw GPU mode | **blocked** |
| Fan curve/default reset | **blocked** |
| Panel Overdrive | **blocked** |
| Keyboard backlight | **FA707NV live-validated** |
| Aura Static RGB | **FA707NV config-accepted** |

The code-level presence of a typed backend is not permission to enable its
product write path. #120 tracks the remaining requirement that capability and
diagnostics evidence must represent these product/policy blocks rather than
publishing backend `Supported` as effective writability.

### 3.4 Known privileged-write blockers

- fan custom write must preserve upstream `CurveData.enabled` (#104);
- fan Factory Defaults must restore the previous performance profile on all failure paths (#105);
- fan CPU/GPU evidence must not be cross-inferred (#109);
- fan `enabled` state must survive Session1/UI transport (#116);
- Battery/Panel/Aura write-owner liveness still needs stronger non-mutating proof (#107); Panel/Aura stay blocked while this is unresolved;
- Keyboard write is restricted to the exact ASUS LED paths and remains model/revision-scoped to FA707NV evidence;
- product GPU mutation remains blocked until product policy/confirmation semantics are proven.

## 4. External service and kernel evidence

Backend data is protocol input even when the service/kernel is trusted as part
of the host TCB. Orbis must handle service restart, version drift, malformed
values, permission changes and competing ownership without inventing support.

Rules:

- typed adapters and strict decode;
- authoritative reads avoid implicit cache where correctness requires freshness;
- capability-local errors;
- read and write evidence remain separate;
- confirmed mutations require read-back when possible;
- model name is not runtime support evidence.

Battery discovery source hardening for #108 is implemented in `main`: broken
power-supply entries no longer silently become “not a battery”; if no valid
candidate exists, remembered permission/I/O evidence wins over structural
`Unsupported`. Executable validation uses the local full flake workflow; hosted Actions are optional by project policy.

Explicit capability refresh still needs to re-query mutation evidence first (#112).

## 5. Confused-deputy model

Forbidden production flow:

```text
GUI → Session1/sessiond → Hardware1
```

Required flow:

```text
original application caller
→ Hardware1
→ polkit(system-bus-name)
→ bounded mutation
```

Caller-provided UID/PID/session metadata, executable paths and user-unit names
are not authorization trust anchors.

## 6. Capability/evidence spoofing

A false `Supported`/`Applied` claim is a security/product risk because users or
future automation may act on incorrect state.

Rules:

- file/object presence alone is not effective write support;
- validation success alone is not effective write support;
- backend `Supported` plus product/policy blocked is **not writable** (#120);
- UI must consume operation-specific write evidence, not only overall capability status;
- `Accepted` is not `Applied`;
- persisted Desired is not Observed;
- empty/partial telemetry is not automatically proof of useful fresh data (#117);
- stored fan points are not proof the custom curve is enabled/active (#116).

## 7. Persistence and automation

Production preferences/window/desired-state stores are separated by concern.
Loading configuration never performs hardware mutation by itself.

Legacy compatibility hardening under #113 is largely implemented:

- defaults are hardware-inert (`automation=false`, no implicit AC/Battery policy, no implicit charge limit);
- legacy writes use exclusive same-directory temp files, fsync + atomic rename + parent fsync and private new-file permissions;
- checked config/state/cache resolvers reject missing/relative HOME/XDG;
- Orbis' own legacy loader uses the checked resolver and cannot fall back to CWD;
- historical CWD-fallback helpers remain deprecated pending compatibility removal.

Reconciliation remains intentionally unimplemented and requires its own policy,
evidence and executable tests.

## 8. Diagnostics/privacy

Diagnostics remains allowlist-based and read-only. Do not export by default:
serials, machine UUIDs, asset tags, arbitrary environment, arbitrary files,
journals, credentials/secrets or full home paths.

Diagnostics does not activate stopped services. Current Diagnostics window stays
fail-closed until lifecycle/refresh glue is complete (#111).

## 9. Runtime availability and hangs

The worker is deliberately ordered, but provider timeout declarations are not
yet generically enforced (#123). A stuck backend must eventually become typed
Timeout/Unavailable and must not indefinitely block unrelated commands,
telemetry or capability refresh.

## 10. Packaging/system boundary

Full NixOS and standalone hardwared deployment use the same product policy and
minimal direct sysfs write intent. Standalone deployment refuses to overwrite a
NixOS-owned symlink unit and validates the effective write-path list; runtime
acceptance is recorded as VM + live module-unit evidence under #126.

Current release blockers:

- hosted GitHub Actions is optional/manual by single-owner project policy (#106 closed);
- `main` intentionally has no hosted required checks (#114 closed by policy);
- obsolete remote agent refs were pruned (#118);
- stable reverse-DNS application identity is fixed by ADR 0013 (#124).

## 11. Security acceptance rules

Before enabling a privileged capability:

1. define the exact user-visible concept;
2. prove the real write owner and non-mutating support evidence;
3. define a bounded typed API and validate untrusted input;
4. preserve original caller authorization;
5. define confirmation/read-back or explicit Accepted/Pending semantics;
6. test denial, unavailable, malformed and partial-failure cases;
7. validate product policy and systemd sandbox on the exact package revision;
8. perform controlled live hardware validation when real device semantics are part of the claim.

Known-unsafe or insufficiently evidenced functionality stays disabled rather
than enabled merely because partial backend code exists.

## 12. Assumptions

- attacker does not already control root or the system bus;
- kernel and installed system services are host TCB, but their data/API may be unavailable, malformed or version-incompatible from Orbis' perspective;
- attacker may run arbitrary same-user processes and call user-accessible D-Bus APIs;
- package/repository integrity and host NixOS/polkit configuration are trusted at installation time.

Changes to these assumptions require a new security review/ADR.
