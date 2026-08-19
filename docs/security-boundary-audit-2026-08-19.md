# Orbis Control security boundary audit — 2026-08-19

Scope: read-only audit of the current `chatgpt/production-hardening-20260818` security boundary. No runtime, policy, service, package or hardware behavior is changed by this document.

## Result

No privilege-escalation or generic-write gap was found in the audited surface.

The current boundary is capability-specific:

- `Session1` is getter-only: charge limit, GPU power, GPU MUX, GPU access, performance and profile-specific fan-curve reads. It exposes no mutation method.
- `Hardware1` exposes typed mutation methods for Performance, Battery threshold, raw GPU backend mode, fan curves/default reset, Panel Overdrive, keyboard brightness and Aura Static RGB, plus read-only mutation-status methods.
- Every active Hardware1 mutation handler resolves the D-Bus message sender and authorizes that system-bus identity through polkit before invoking its backend.
- The raw GPU method remains in the stable ABI but production composition uses a disabled backend and returns `Unsupported`; this does not constitute an enabled GPU product mutation path.
- D-Bus policy permits callers to address Hardware1, while mutation authorization is deliberately enforced inside hardwared through per-capability polkit actions.
- Polkit declares separate actions for Performance, Battery, GPU, fan curves, Panel Overdrive, keyboard backlight and Aura Static RGB. Defaults are `allow_any=no`, `allow_inactive=no`, `allow_active=yes`.
- NixOS hardwared runs as the system service with `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, `ProtectControlGroups`, `RestrictAddressFamilies=AF_UNIX`, `MemoryDenyWriteExecute`, and an empty capability bounding set.
- `/sys` is read-only except two exact optional paths: `/sys/firmware/acpi/platform_profile` and `/sys/class/leds/asus::kbd_backlight/brightness`.
- Battery effective-threshold and Panel Overdrive authoritative reads remain read-only. Fan, Battery configured threshold, Panel setter and Aura RGB mutations that use asusd go through typed D-Bus backends rather than broad filesystem-write access.
- There is no generic client-supplied filesystem path or arbitrary D-Bus method forwarding surface in the audited Hardware1 API.

## Boundary notes

### Hardware1

Mutation inputs are constrained wire/domain values rather than client-provided paths or commands. Performance performs fresh choices/read-back. Battery, fan curves, Panel Overdrive and keyboard brightness require typed validation/read-back semantics. Aura Static RGB intentionally returns config-confirmed `Accepted`, not hardware-applied success, because its hardware RGB ABI is write-only.

Read-only `*_mutation_status` methods are intentionally unauthenticated capability metadata. Their presence does not perform a write and does not grant mutation authority.

### Session1

The protocol crate explicitly defines a getter-only interface. Performance mutation is documented as Hardware1-owned, and current Session1 fan access is a read method. No Session1 mutation delegation was found in the audited protocol surface.

### Polkit and caller identity

`PolkitAuthorizer` constructs a `system-bus-name` subject from the actual zbus message sender. This preserves original-caller authorization rather than authorizing hardwared itself.

### Filesystem sandbox

The writable sysfs allowlist matches the direct filesystem writers currently composed by hardwared: Performance and keyboard brightness. Other currently composed mutations use typed service backends and therefore do not require broadening `ReadWritePaths`.

## Findings requiring follow-up, not privileged changes in this slice

1. **Documentation drift:** `crates/orbis-hardwared/src/lib.rs` module-level text and `packaging/nix/module.nix` comments still describe hardwared as Performance-only, despite the larger typed Hardware1 surface. This is stale documentation, not an effective security-boundary expansion.
2. **D-Bus policy comment drift:** the Hardware D-Bus policy comment names only the Performance polkit action although hardwared now has separate actions for multiple capabilities. Enforcement itself remains per-capability in code.
3. **Raw GPU ABI:** `SetGpuMode` remains publicly addressable and separately polkit-protected even though its production backend is disabled. Keep it disabled until product-level GPU semantics are proven; do not interpret ABI presence as supported mutation.

## Evidence classification

This audit is source-level `TESTED/IMPLEMENTED` evidence about boundary structure only. It is not packaged-runtime or live-hardware security validation. No service was started and no system/session D-Bus or hardware mutation was executed.
