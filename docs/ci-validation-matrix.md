# CI Validation Matrix

## Pull request baseline

Required:

- `nix flake check --print-build-logs`
- formatting check
- workspace compilation/tests where applicable

## Targeted development validation

Use the smallest relevant scope:

- changed crate check
- changed crate tests
- clippy for changed crate
- diff whitespace validation

## Integration changes

Run when changes cross service boundaries:

- cargo workspace checks
- workspace tests
- Nix validation

## Hardware safety

CI must not:

- access real ASUS hardware;
- execute hardware mutation methods;
- assume host services exist.

Live hardware validation remains a separate controlled process.
