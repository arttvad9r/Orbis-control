# Production Boundary Audit

## Scope

This document tracks production-hardening boundaries after the first Hardware1 mutation path.

## Rules

- GUI never performs privileged hardware I/O.
- Session daemon remains unprivileged and read-oriented.
- Hardware1 exposes only typed semantic mutations.
- No generic sysfs, shell, filesystem, or arbitrary D-Bus proxy methods.
- Unsupported capability states remain explicit.

## Current verified areas

- Performance mutation: Hardware1 path with authorization boundary.
- Battery charge-limit mutation: typed backend path.
- GPU product mutation: intentionally disabled until semantics are proven.
- Fan curve writes: remain owned by asusd boundary.

## Review checklist for new mutations

Before adding a mutation:

1. Identify hardware owner.
2. Define authoritative read-back source.
3. Define authorization requirement.
4. Add typed domain API.
5. Add unavailable/unsupported behavior.
6. Add tests without requiring live hardware.

## Not in scope

- Direct hardware writes from UI.
- New raw backend passthrough methods.
- Enabling disabled GPU controls without evidence.
