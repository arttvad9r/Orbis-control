# Roadmap

> Роль: **FUTURE PLAN**. Актуальная очередность работ без календарных обещаний.
> Фактическое состояние и evidence — в [`current-state.md`](current-state.md).
> Обновлено: **2026-08-19**.

## Ordering principles

1. Сначала green build/CI и чистая интеграционная линия.
2. Затем read-only evidence и capability-local failure handling.
3. Только потом privileged mutation для конкретной доказанной операции.
4. Read/write evidence всегда независимы.
5. Никаких runtime model-name tables вместо typed probes.
6. `Accepted` не считается `Applied`; authoritative observed state требует подтверждения.
7. Live hardware claims делаются только по dated revision-scoped evidence.

## Milestone 0 — Repository/integration baseline

**Status: ACTIVE**

Цель: один понятный production baseline вместо разросшегося дерева Draft/validation веток.

Done:

- Rust toolchain приведён к 1.87, совместимому с locked dependency graph;
- UPower startup coupling исправлен: Battery failure capability-local;
- NixOS UPower default добавлен без hard lifecycle dependency;
- preferences stack консолидирован;
- window-state foundation интегрирован;
- release evidence taxonomy и security-boundary docs интегрированы;
- устаревшие/одноразовые audit/validation PR закрываются как superseded.

Remaining gate:

- `nix flake check` должен быть green на актуальном hardening HEAD;
- после green CI hardening переводится в `main`;
- remote validation refs удаляются отдельно, когда доступен git/branch-delete интерфейс.

## Milestone 1 — Existing production vertical slices

**Status: COMPLETED with revision-scoped live evidence**

- Battery read.
- Battery controlled mutation with read-back.
- Performance read.
- Performance controlled mutation with read-back.
- GPU primitive reads: power / physical MUX / access policy.
- Narrow Hardware1 privilege boundary.

Эти результаты не означают поддержку product GPU mode или всех ASUS устройств.

## Milestone 2 — Production resilience and lifecycle

**Status: PARTIAL**

Completed:

- Session1 survives missing/unready UPower; Battery rediscovery is lazy.
- Independent capabilities do not depend on Battery startup success.

Next:

- reconcile Desired / Observed / Pending domain foundation with the integrated config baseline;
- integrate inert lifecycle events;
- design reconciliation separately: startup/resume compares authoritative observed state before any action;
- no retry/poll loops that mask races or ownership conflicts.

## Milestone 3 — User persistence and desktop integration

**Status: PARTIAL**

Completed:

- versioned safe `preferences.toml`;
- atomic + durable writes and permission preservation;
- persisted Dark/Light theme;
- persisted Start Minimized;
- redacted config warning diagnostics;
- independent XDG window-state store.

Next:

- resolve/integrate XDG Run on Startup stack against current preferences/UI baseline;
- integrate desktop/AppStream metadata only after packaged validation is green;
- keep automation policy and desired hardware state separate from UI preferences.

## Milestone 4 — Fans and telemetry

**Status: IMPLEMENTED/TESTED; acceptance incomplete**

Telemetry:

- production sysfs provider exists;
- worker-owned polling exists;
- failures preserve honest availability/freshness semantics.

Fans:

- active curve authority remains sysfs;
- profile-specific reads go through Session1 → sessiond → asusd;
- Hardware1 typed mutation path exists;
- Quiet/LowPower profile identity is preserved losslessly.

Gate before completion:

- dated live fan mutation validation with authoritative post-write read-back and restored final hardware state;
- no universal ranges/defaults inferred from one model.

## Milestone 5 — Diagnostics and supportability

**Status: ACTIVE in Draft stack**

Target architecture:

- application-owned immutable diagnostics snapshot;
- privacy-safe application/system/hardware identity sources;
- service presence independent from capability support;
- GPU primitives remain independent;
- telemetry freshness preserved;
- display observation read-only;
- presentation DTO separated from collectors;
- text/JSON export uses a strict allowlist and versioned schema.

Next:

- consolidate the already validated read-only diagnostics stack into the current hardening baseline;
- wire Diagnostics UI only from the typed snapshot;
- integrate privacy-bounded exporters and their end-to-end regression;
- never collect raw journals, arbitrary files, full environment dumps, serial/UUID/asset-tag fields or shell output by default.

## Milestone 6 — GPU product policy

**Status: BLOCKED**

Eco / Standard / Ultimate / Optimized are product policy, not aliases for one primitive.

Before implementation:

- prove mapping between product intent and independent MUX/access/power primitives;
- define pending/reboot/logout requirements explicitly;
- define owner and authoritative read-back;
- add P2P/fake-system tests;
- only then permit controlled live mutation.

Until then production product-mode controls remain unsupported/disabled.

## Milestone 7 — Power limits and extended ASUS controls

**Status: BLOCKED / UNKNOWN by concept**

Power limits:

- typed scaffolding exists;
- production provider, units/ranges/default evidence and Hardware1 per-field contract do not.

Extended controls:

- Panel OD / keyboard / Aura have narrower foundations;
- MiniLED / Screen Auto Brightness remain read-only where write evidence is absent;
- AniMe/Slash and boot sound/MCU powersave/panel HD/eGPU require concept-specific evidence/design.

Do not add generic firmware writers or enable features from DMI model names.

## Milestone 8 — CLI and release packaging

**Status: NOT COMPLETE**

- Replace the `orbisctl` stub with a real read/diagnostic CLI before calling CLI support complete.
- Integrate desktop/AppStream assets into the Nix package and validate installed metadata.
- Maintain support matrix evidence separately from runtime capability detection.
- Run full packaged acceptance after green CI and integration into `main`.

## Release gate

A beta/release candidate requires all of the following:

- `main` contains the intended production baseline;
- `cargo fmt/check/test/clippy` relevant integration tier is green;
- `nix flake check` is green;
- package builds and required metadata is installed/validated;
- no Draft branch is being treated as integrated functionality;
- hardware mutation claims have exact dated live evidence;
- remaining unsupported controls are explicitly disabled/unknown rather than simulated.
