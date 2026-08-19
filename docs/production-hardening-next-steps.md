# Production hardening next steps

> Current queue only. Operational facts belong in [`current-state.md`](current-state.md).

## Completed foundations

- production hardening consolidated into `main`;
- typed privileged boundary established while the GUI remains unprivileged;
- capability evidence states are explicit and unsupported controls fail closed;
- Battery/UPower startup failures are capability-local;
- Rust 1.87 matches the locked dependency graph;
- preferences, window state, desired state and lifecycle domain foundations are integrated;
- production telemetry and fan read/control foundations are present;
- diagnostics snapshot/export/runtime/model foundations are integrated;
- release evidence, packaging metadata and support-matrix tooling are present.

## Next safe implementation order

1. Restore executable CI and obtain a green full flake validation on current `main`.
2. Finish XDG Run on Startup Rust lifecycle glue (#57).
3. Finish Diagnostics window Rust lifecycle/refresh glue (#101).
4. Add reconciliation policy/tests on top of Desired/Observed/Pending without automatic startup application by default.
5. Implement a useful read/diagnostic `orbisctl` CLI.
6. Continue evidence-driven work on fan acceptance, product GPU policy, power limits and extended controls.

## Restrictions

- Do not add generic privileged writer/proxy APIs.
- Do not infer runtime capability support from DMI model names.
- Do not turn `Accepted` into `Applied` without authoritative confirmation.
- Keep device-specific live mutation validation separate from hermetic CI.
