# Production Hardening Status

> Historical hardening summary. For current operational status use [`current-state.md`](current-state.md).

## Integrated hardening

The production-hardening line is now part of `main`.

Completed areas include:

- capability-local Hardware1/Session1 startup behavior;
- disabled/unproven product GPU mutation;
- typed Battery and Performance contracts with authoritative confirmation semantics;
- bounded numeric conversions for hardware-adjacent data;
- production telemetry provider and worker-owned polling;
- profile-specific fan reads and typed fan control/reset paths;
- Rust 1.87 locked-build compatibility;
- safe preferences/window/desired-state storage foundations;
- diagnostics domain/provider/collector/export/runtime foundations;
- desktop/AppStream packaging and support-matrix tooling;
- refreshed privilege/evidence/multi-model documentation.

## Current hardening gaps

1. GitHub Actions currently fails before executing/exposing workflow steps; release still requires an actually executed green `nix flake check`.
2. XDG Run on Startup needs the final Rust Preferences lifecycle glue; backend and Slint contract are already integrated.
3. Diagnostics window needs the final Rust open/refresh lifecycle glue; runtime/model/Slint layers are already integrated.
4. Desired/Observed/Pending and lifecycle persistence foundations are integrated, but reconciliation/execution policy is intentionally not implemented yet.
5. Product GPU policy, power limits and extended ASUS controls remain evidence-gated.
6. `orbisctl` remains a stub.

## Safety constraints preserved

- Development validation does not perform live hardware mutation by default.
- No generic privileged proxy is introduced.
- GUI remains unprivileged.
- Capability support is probe/evidence-driven rather than inferred from model names.
- `Accepted` is not treated as authoritative `Applied` state.

This file does not claim new device-specific live support; such claims remain revision-scoped and belong in explicit evidence records.
