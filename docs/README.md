# Documentation

Documentation in this directory is reference material. It is **not** a mandatory execution pipeline for development sessions.

For day-to-day work use, in order:

1. production source and executable behavior;
2. `AGENTS.md` for repository working rules;
3. `TODO.md` for unfinished product work;
4. this directory only when the task needs stable architecture, product intent, an ADR, or hardware evidence.

Do not spend a development session synchronizing status documents, historical plans, audits, or evidence vocabulary with one another.

## Stable references

- [`architecture.md`](architecture.md) — system boundaries and durable architecture decisions.
- [`product.md`](product.md) — product purpose and user-facing trust requirements.
- [`adr/`](adr/) — accepted architectural decisions.
- [`hardware-evidence/`](hardware-evidence/) — dated device-specific observations and validation records.
- [`support-matrix-schema.md`](support-matrix-schema.md) / support-matrix schema examples — support evidence format where needed.
- [`threat-model.md`](threat-model.md) — security/trust-boundary background.

Other dated plans, audits, research notes, status snapshots and superseded design documents are historical context only. They must not override current source, `TODO.md`, or the user's task.

## Update policy

Update documentation only when a completed change would otherwise make a stable reference materially wrong. Do not create a document merely to record that implementation work happened.

Hardware evidence is the exception: live device validation may add a new dated record when preserving the exact revision/device/result is useful. Never rewrite old hardware observations to make them match newer code.