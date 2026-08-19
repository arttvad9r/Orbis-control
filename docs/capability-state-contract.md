# Capability State Contract

Production code must distinguish evidence states.

## States

- Ready: backend evidence is available and validated.
- Loading: value acquisition is in progress.
- Unavailable: capability cannot currently provide a value.
- Unsupported: backend exists but operation is not implemented/proven.
- Unknown: evidence is insufficient.
- ReadOnly: state can be observed but not mutated.

## Rules

- Never replace Unknown with a guessed value.
- Never expose disabled mutation controls as successful operations.
- Read-back defines final mutation state.
- Backend implementation details must not leak into UI semantics.
