# Product contract — Orbis Control

Orbis Control gives Linux users of supported ASUS ROG/TUF/Zephyrus laptops one normal user-session application for understanding device state and performing supported controls without pretending unsupported hardware works.

This document describes durable product intent only. Current implementation state lives in the source and `TODO.md`, not in status tables here.

## User outcomes

A useful Orbis installation should let the user:

- inspect relevant laptop state from a normal Wayland/X11 user session;
- understand whether a value/action is supported, read-only, temporarily unavailable, permission-denied, pending or unknown;
- change supported settings such as performance, charging, cooling, graphics or lighting through a bounded system integration path;
- see authoritative state, an explicit pending requirement, or a useful failure after an action;
- keep using unaffected parts of the application when one backend/capability is missing;
- install/run the application without agent-only fixture setup.

## Product rules

- **Truthful state.** Never display fixture/default/guessed state as if it came from the device.
- **Capability honesty.** Read support does not imply write support. A backend object or laptop model name does not prove an operation is supported.
- **No optimistic success.** Requested state is not observed state. `Accepted` is not automatically `Applied`.
- **Safe degradation.** Missing or failing providers remain localized and explicit rather than producing fake fallback state.
- **Bounded privilege.** The GUI remains unprivileged. Privileged mutations use narrow typed operations and capability-specific authorization, never a generic root/sysfs/shell proxy.
- **Concept separation.** Different meanings such as GPU product mode, physical MUX, runtime power and pending reboot state must not be collapsed into one misleading value.
- **Practical completeness.** A feature is not complete because domain types, a backend method, tests or a design document exist. The user-visible flow must work end to end, or the unfinished product surface should be removed until it does.

## Non-goals

Orbis is not intended to:

- claim universal ASUS feature support;
- expose arbitrary privileged hardware/system operations;
- use mock/test state as production state;
- hide uncertainty behind a successful-looking UI;
- automatically apply persisted values merely because configuration was loaded;
- keep nonfunctional UI or architecture shells indefinitely for hypothetical future features.

## Completion target

The product is ready for a first release when the core daily-control experience is coherent, the packaged application starts and behaves correctly, supported actions have honest confirmation/error semantics, known unsafe writes are not enabled, and `scripts/verify full` passes for the release candidate.

The concrete unfinished queue is [`../TODO.md`](../TODO.md). Stable implementation boundaries are described in [`architecture.md`](architecture.md) and accepted ADRs.