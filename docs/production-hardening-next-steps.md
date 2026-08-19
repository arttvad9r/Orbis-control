# Production hardening next steps

## Completed scope

- privileged Hardware1 boundary established;
- GUI remains outside privileged hardware access;
- capability states are explicit;
- unsupported mutation paths stay disabled;
- CI validates flake checks.

## Next safe implementation order

1. Add invariant tests around capability state transitions.
2. Add provider contract tests using fake backends only.
3. Verify D-Bus DTO validation boundaries.
4. Verify UI handling of Unknown, Unsupported, Unavailable and ReadOnly states.
5. Keep real hardware mutation validation separate from CI.

## Restrictions

- No direct sysfs fan writes.
- No generic hardware proxy APIs.
- No GPU mutation until semantics are proven.
- No production hardware writes during development automation.
