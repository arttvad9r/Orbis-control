# Verification Contract — Orbis Control

> Роль: **VERIFICATION CONTRACT**.
> Обновлено: 2026-08-20.
>
> Этот документ определяет, какой evidence разрешает claims о source, tests, package/runtime и hardware. Operational status — [`current-state.md`](current-state.md), current CI map — [`ci-validation-matrix.md`](ci-validation-matrix.md), release vocabulary — [`release-evidence-taxonomy.md`](release-evidence-taxonomy.md).

## 1. Core rule

Verification claim должен опираться на сохранённый/наблюдаемый evidence, а не на фразу агента.

```text
verification effort ∝ scope + risk + affected boundary
```

Docs cleanup, read-only provider change and privileged mutation change require different evidence. Never weaken a safety invariant merely to obtain green output.

## 2. Project-status vocabulary

Current project docs use:

- `IMPLEMENTED` — source implementation is present/reviewable;
- `TESTED` — relevant executable tests/checks passed for the exact revision/environment;
- `PACKAGED` — installed/package behavior was accepted;
- `LIVE-VALIDATED` — relevant behavior was observed on named live hardware/environment;
- `BLOCKED` / `UNKNOWN` — required evidence is unavailable or inconclusive.

Source inspection and stdlib static scripts support at most `IMPLEMENTED`. They do not create a green Rust/Nix/Slint claim.

## 3. Check result states

| State | Meaning |
|---|---|
| `PASS` | Check actually ran and the required condition held |
| `FAIL` | Check ran and the condition did not hold |
| `BLOCKED` | Required check cannot run because of a named blocker |
| `DEFERRED` | Check intentionally postponed with an explicit reason |
| `REQUIRES_USER` | Physical device/manual/privileged/user-only action is required |
| `NOT_RUN` | No execution evidence exists |

Queued workflow objects, YAML definitions, source assertions and missing logs are not implicit PASS.

## 4. Layer 0 — stdlib static source contracts

Available without Rust/Nix:

```bash
python3 scripts/verify-static
```

Current suite covers source-level invariants for:

- UI request-only/fake-success boundaries;
- Automation shadow/executor/recovery contracts;
- Display Refresh fail-closed contract;
- cross-surface backend completion markers;
- provider timeout and canonical capability-refresh ownership;
- documentation/current-status consistency and repository artifact hygiene.

A successful `verify-static` means the checked source markers/invariants are internally consistent. It does **not** prove:

- Rust compilation;
- Slint compilation;
- D-Bus/runtime behavior;
- Nix packaging;
- physical hardware behavior.

## 5. Rust verification

For repository/release evidence, commands are lockfile-strict:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

For a narrow crate while iterating:

```bash
python3 scripts/verify-static
cargo check -p <crate> --all-targets --locked
cargo test -p <crate> --locked
cargo clippy -p <crate> --all-targets --locked -- -D warnings
```

Do not turn an unlocked/local dependency resolution into release evidence.

## 6. Nix/package verification

Relevant package/service/release changes require targeted Nix checks and ultimately:

```bash
nix flake check --max-jobs 1 --cores 4
```

Current flake intends to validate workspace/package behavior plus fake-system Hardware1 lifecycle, Performance mutation and Battery mutation VMs. VM/fake-system validation proves software/service/policy properties only; it is not `LIVE-VALIDATED` laptop evidence.

## 7. GitHub Actions

Canonical workflow: `.github/workflows/ci.yml`.

Intended order:

1. Checkout;
2. `python3 scripts/verify-static`;
3. install Nix;
4. Nix/Rust/VM checks;
5. final full flake check.

#106 currently blocks trustworthy Actions execution: earlier jobs did not reach repository steps or fresh pushes produced no run. Until actual steps execute successfully, GitHub-hosted validation remains `BLOCKED`, not `PASS` or `FAIL` for the source itself.

## 8. Runtime and hardware evidence

`PACKAGED`/runtime evidence should record at least:

- exact revision/build;
- package/environment;
- command/action performed;
- observed result;
- relevant service/policy state;
- restoration/final state when mutation is involved.

`LIVE-VALIDATED` hardware evidence additionally requires:

- exact device/model/environment;
- authoritative read/write observation;
- date/revision;
- no substitution of mock/default values;
- clear separation of read support from write support.

Old live evidence remains historical/revision-scoped. It does not automatically validate a changed branch.

## 9. Mutation-specific verification

Privileged/hardware mutation has a stronger burden than read-only code.

Before promotion, verify:

1. typed owner/target identity;
2. capability-specific authorization;
3. input validation;
4. no caller-controlled arbitrary path/shell command;
5. pre-read when needed;
6. one deliberate mutation;
7. authoritative read-back or explicit Pending semantics;
8. timeout/transport unknown outcome enters recovery/observation, not blind retry;
9. final/restored state is recorded for destructive/reversible validation.

`Accepted` transport/config confirmation is not enough to claim `Applied` hardware state.

Fan/GPU/Panel/Keyboard/Aura/Display/Automation product gates remain independent. A successful generic Hardware1/service test does not authorize a blocked product control.

## 10. Read-only/evidence verification

Read-only work still needs truthfulness tests:

- structural absence vs transient failure;
- permission denied vs unsupported;
- malformed wire/value handling;
- partial/empty success semantics where applicable;
- stale/recovery behavior;
- independent concepts do not infer support from each other.

Examples of current open evidence work:

- #117 telemetry useful/partial/empty freshness;
- #109 CPU/GPU fan aggregate support;
- #116 stored fan enabled-state transport;
- #107 dynamic Battery owner/interface liveness.

## 11. Documentation-only verification

A docs/repository-status task should run at least:

```bash
python3 scripts/verify-static
# plus, when a checkout/tooling environment is available:
git diff --check
```

The docs contract specifically protects against reintroducing known stale lifecycle claims, stale Draft-PR beta status and one-off `.github` validation artifacts.

## 12. Evidence record minimum

For a command-based check record:

```text
revision
environment
command
exit status
short summary
relevant failure excerpt (if any)
full log/artifact location (if available)
supported claim level
```

For manual/runtime/hardware checks, replace command fields as appropriate with explicit observations and target identity.

## 13. Completion policy

A source task may be closed as source-complete when its implementation scope is actually present and remaining execution is tracked by a global validation blocker such as #106. The closure text must not claim executable success that was not observed.

A release/milestone cannot use that shortcut. Release acceptance requires the exact candidate to pass the required executable/package/live gates in [`beta-acceptance-checklist.md`](beta-acceptance-checklist.md).

Unresolved mandatory checks remain visible as `BLOCKED`, `DEFERRED` or `REQUIRES_USER`; they are never silently promoted to PASS.
