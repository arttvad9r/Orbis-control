# FA707NV Full Capability Promotion Design

## Goal

Довести текущую production-сборку Orbis Control до максимально полного
безопасного функционала на ASUS FA707NV, не добавляя неподтверждённые
hardware assumptions и не затрагивая другие модели.

## Decision

Promotion выполняется capability-by-capability через существующий путь:

```text
original UI caller → Hardware1 → capability polkit → typed backend
→ authoritative read-back / Pending / fail-closed error
```

Порядок: keyboard backlight, fan curves, ASUS product GPU mode, затем display
refresh только после появления конкретного compositor owner. Raw GPU через
supergfxd не включается, пока сервис отсутствует на FA707NV.

## Safety invariants

- Every live mutation is read-before-write, range-validated and read-back verified.
- Permission/owner loss/malformed data never becomes `Supported` or fake success.
- No generic sysfs, shell, compositor-command or D-Bus proxy is added.
- Fan reset/custom writes retain unknown-outcome semantics and remain disabled
  until their independent production-promotion decision.
- GPU product queue preserves current/queued pair and reboot requirement; no
  automatic reboot or logout.
- Display mutation requires a typed compositor owner and exact internal-panel
  identity proof; Wayland read-only observation is not a write owner.

## Acceptance

Each capability requires focused source/P2P tests, local `verify-static`, full
`nix flake check --max-jobs 1 --cores 4`, current NixOS deployment, controlled
FA707NV live validation with restore/final-state evidence, and UI writability
derived from operation-level `Supported` evidence. GitHub Actions is optional.
