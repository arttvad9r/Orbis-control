# Production Hardening Status

## Scope

This document records the status of the production-hardening pass in PR #1.

## Completed in this pass

- Hardware1 startup keeps capability failures isolated instead of failing unrelated capabilities.
- Raw GPU mutation remains disabled until Orbis product-level GPU semantics are proven.
- Battery charge-limit configuration validation matches the Hardware1 contract: `20..=100`, with `None` meaning unmanaged.
- Sysfs telemetry and fan parsing reject narrowing conversions instead of wrapping values.
- Fan editor input validation rejects values that cannot be represented by wire/domain types.
- Deployment parsing keeps machine-readable Nix output separated from progress stderr.

## Safety constraints preserved

- No production hardware writes are performed by development validation.
- No generic privileged proxy is introduced.
- GUI remains unprivileged.
- Hardware mutation requires typed contracts, authorization, and read-back semantics.

## Remaining next steps

1. Documentation normalization against current runtime behavior.
2. Runtime capability registry expansion.
3. Real telemetry vertical slice.
4. Fan RPM/read model expansion.

This file is status documentation only and does not claim new hardware support.
