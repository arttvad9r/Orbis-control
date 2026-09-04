---
name: orbis-hardware-safety
description: Safety rules for Orbis hardware semantics. Use when changing providers, privileged mutations, capability evidence, sysfs/asusd/supergfxd behavior, or hardware-facing UI state.
---

# Orbis hardware safety

These rules protect real hardware. They must not turn ordinary software development into a stop condition.

## Invariants

- Read support and write support are independent.
- Probe capabilities at runtime; never infer support from a laptop model name alone.
- Validate values and ranges at the hardware boundary.
- Permission failures, missing backends, temporary failures and unsupported hardware remain distinct outcomes.
- Privileged mutation stays behind typed `Hardware1` operations and capability-specific authorization.
- Never add arbitrary privileged path, shell-command or generic D-Bus proxy execution.
- Unknown mutation outcome after possible dispatch must not be blindly retried.
- `Accepted` is not the same as authoritative `Applied` hardware state.

## Development versus live execution

You may freely implement, refactor, compile and test hardware-facing software using mocks, fixtures, private P2P D-Bus transports, fake sysfs, fake services, or disposable non-hardware integration environments.

Do not execute real hardware writes, `sudo`, real privileged mutation calls, or uncontrolled system-bus experiments unless the user's task explicitly authorizes that real-hardware operation.

Lack of physical hardware blocks only a **live-validation claim**. It does not block completing the production code path, UI wiring, error handling, mocks, tests, packaging integration, or fail-closed capability gating.

## When semantics are uncertain

First inspect the existing provider/backend, kernel or upstream API documentation, existing tests, and available read-only evidence. Choose the safest implementation consistent with those facts and keep unsupported paths fail-closed.

Ask the user only when the missing fact changes the product semantics materially and cannot be resolved from source, documentation, read-only probing, or a safe default. Do not stop merely because a hardware path has not yet been live-validated.

## Testing

Prefer behavior tests over source-marker assertions. Tests must not touch real hardware by default. Use the narrowest existing integration fixture that observes the actual boundary being changed.
