---
name: orbis-system-integration
description: Use for Orbis D-Bus, sessiond/client/protocol, hardwared, systemd, polkit, Nix packaging/module, or VM integration work.
---

# Orbis system integration

## Boundaries

- Keep session protocol/client/daemon, application services and privileged hardwared responsibilities explicit.
- Treat D-Bus DTOs as untrusted input and validate them at the wire/domain boundary.
- UI/domain code must not perform direct privileged system or hardware I/O.
- Prefer existing typed interfaces over new cross-layer plumbing.
- Never introduce a generic privileged execution path for implementation convenience.

## Implementation style

Complete integration work end to end. A D-Bus or system-service task is not done when only a DTO, trait, policy file, or daemon method exists: wire the producer, consumer, error mapping, runtime ownership and user-visible behavior that the feature requires.

Cross-crate changes are normal. Do not stop after modifying only one boundary if the requested vertical slice still does not work.

## Testing

- Use existing private P2P/fake-system/VM integration paths by default.
- Do not use a real system/session bus or real hardware mutation in automated tests unless the user explicitly authorized it.
- Run the targeted test during implementation, then broader `scripts/verify task` or `scripts/verify full` once the coherent integration slice is complete.
- VM/fake-sysfs behavior proves software integration, not physical ASUS hardware behavior.

For changes to real hardware semantics, also apply `orbis-hardware-safety`.