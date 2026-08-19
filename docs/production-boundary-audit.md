# Production Boundary Audit

> **Historical snapshot.** This file records an earlier hardening review after the first Hardware1 mutation path. It is not the current operational or security source of truth. Use [`current-state.md`](current-state.md), [`architecture.md`](architecture.md) and [`threat-model.md`](threat-model.md) for current behavior and blockers.

## Durable rules retained from this audit

- GUI does not perform privileged hardware I/O directly.
- Session daemon remains unprivileged and read-oriented; it is not a mutation deputy.
- Hardware1 exposes only typed semantic mutations.
- No generic sysfs, shell, filesystem or arbitrary D-Bus proxy methods.
- Unsupported/Unavailable/PermissionDenied/Unknown evidence remains explicit.
- Every mutation needs a proven owner, bounded authorization and defined confirmation semantics.

## Historical reviewed areas

- Performance mutation established the Hardware1/polkit boundary.
- Battery charge-limit mutation added a typed owner/read-back path.
- GPU product mutation remained intentionally disabled.
- Fan mutation ownership was assigned to the typed asusd boundary rather than direct sysfs writes.

The last point is **ownership only, not current write acceptance**. Current fan mutation/default-reset is fail-closed because later audits found unresolved safety/evidence defects. See `current-state.md` and issues #104/#105/#109/#116/#120.

## Durable review checklist for new mutations

Before adding or re-enabling a mutation:

1. identify the actual write owner and non-mutating support evidence;
2. define authoritative read-back or explicit Accepted/Pending semantics;
3. define the authorization boundary for the original caller;
4. expose a narrow typed domain/API contract;
5. preserve unavailable/unsupported/permission-denied/unknown distinctions;
6. add deterministic tests without real hardware mutation;
7. obtain executable package/CI validation on the exact revision;
8. add controlled dated hardware evidence when the claim depends on real device semantics.

This historical file does not override later safety blocks or current capability status.