# Production Hardening Acceptance Checklist

## Boundaries

- GUI stays unprivileged.
- Session daemon remains a read boundary.
- Hardware daemon exposes only typed semantic mutations.
- No generic sysfs, shell, or D-Bus passthrough APIs.

## Capability semantics

- `Ready` requires authoritative backend evidence.
- `Loading` is only transient startup state.
- `Unavailable` means backend path is unavailable.
- `Unsupported` means capability is known but intentionally not implemented.
- `Unknown` means evidence is insufficient.
- `ReadOnly` must not expose mutation controls.

## Mutation requirements

Before adding a mutation path:

1. Define backend ownership.
2. Define authorization boundary.
3. Validate wire input.
4. Perform authoritative read-back.
5. Add tests without requiring real hardware.

## Hardware safety

- Startup must not perform hardware writes.
- Tests must not require real ASUS mutation.
- Fan writes remain behind the approved owner boundary.
- GPU product controls require proven semantics before enabling.

## Release gate

Required before leaving hardening phase:

- CI validation passes.
- Documentation matches production code.
- No fake fallback states exist.
- No undocumented privileged surface exists.
